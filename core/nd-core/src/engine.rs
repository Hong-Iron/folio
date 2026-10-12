//! The session clock: turns committed states into segments, decides nudges,
//! pauses when you leave, and tells the Swift shell what to sense next.

use std::collections::{HashMap, HashSet};

use crate::analytics::{self, SegmentView, SessionSummary};
use crate::classify::{classify, onset, Activity};
use crate::config::{ms, Config};
use crate::error::{NdError, Result};
use crate::features::{Extractor, Features, Identity};
use crate::identity::FaceTemplate;
use crate::rules::Rules;
use crate::signal::SignalFrame;
use crate::smooth::Smoother;
use crate::store::{SegmentRow, SessionRow, Store};
use crate::zone::{Calibration, Zone};

#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionMode {
    Idle,
    Running,
    Break,
    Paused,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct Nudge {
    pub activity: Activity,
    pub detail: Option<String>,
    pub drift_ms: u64,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct SensingPlan {
    pub tick_ms: u32,
    pub landmarks: bool,
    pub body: bool,
    pub hands: bool,
    pub objects: bool,
    pub verify: bool,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct EngineUpdate {
    pub mode: SessionMode,
    pub activity: Option<Activity>,
    pub raw: Activity,
    pub zone: Zone,
    pub detail: Option<String>,
    pub focus_ms: u64,
    pub elapsed_ms: u64,
    pub present_ms: u64,
    pub goal_ms: Option<u64>,
    pub break_left_ms: Option<u64>,
    pub nudge: Option<Nudge>,
    pub ask_category: Option<String>,
    pub welcome_back: bool,
    pub too_dark: bool,
    /// The person in front of the camera isn't the enrolled user.
    pub not_you: bool,
    pub plan: SensingPlan,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Mode {
    Running,
    Break { until: u64 },
    Paused,
}

#[derive(Clone, Debug)]
struct Seg {
    db_id: Option<i64>,
    activity: Activity,
    start: u64,
    end: u64,
    detail: Option<String>,
    claimed: bool,
}

#[derive(Debug)]
struct Live {
    id: i64,
    label: Option<String>,
    goal_min: Option<u32>,
    start_t: u64,
    start_wall: i64,
    tz: i32,
    closed: Vec<Seg>,
    open: Option<Seg>,
    mode: Mode,
    last_t: u64,
    last_persist_t: u64,
    drift_since: Option<u64>,
    nudged: bool,
    last_nudge_t: Option<u64>,
    neutral_ms: HashMap<String, u64>,
    asked: HashSet<String>,
}

impl Live {
    fn wall(&self, t: u64) -> i64 {
        self.start_wall + t.saturating_sub(self.start_t) as i64
    }

    fn sum(&self, t: u64, pred: impl Fn(&Seg) -> bool) -> u64 {
        let closed: u64 = self.closed.iter().filter(|s| pred(s)).map(|s| s.end.saturating_sub(s.start)).sum();
        let open = self.open.as_ref().filter(|s| pred(s)).map_or(0, |s| t.max(s.end).saturating_sub(s.start));
        closed + open
    }

    fn focus_ms(&self, t: u64) -> u64 {
        self.sum(t, |s| s.activity.is_focus() || (s.activity == Activity::Unclear && s.claimed))
    }

    fn present_ms(&self, t: u64) -> u64 {
        self.sum(t, |s| !matches!(s.activity, Activity::Away | Activity::Break))
    }
}

pub const LIVE_SESSION_ID: i64 = 0;
const PERSIST_EVERY_MS: u64 = 2_000;

pub struct EngineCore {
    pub cfg: Config,
    pub cal: Calibration,
    pub rules: Rules,
    pub store: Option<Store>,
    pub template: Option<FaceTemplate>,
    extractor: Extractor,
    smoother: Smoother,
    live: Option<Live>,
    pub last_storage_error: Option<String>,
}

fn detail_for(a: Activity, f: &Features) -> Option<String> {
    match a {
        Activity::Social | Activity::ScreenFocus => f.app.detail.clone(),
        Activity::Unclear => Some(if f.zone == Zone::Desk { "desk" } else { "screen" }.to_string()),
        _ => None,
    }
}

impl EngineCore {
    pub fn new(store: Option<Store>) -> Result<Self> {
        let (cfg, cal, rules, template) = match &store {
            Some(s) => (
                s.get_setting("config")?.and_then(|v| serde_json::from_str(&v).ok()).unwrap_or_default(),
                s.get_setting("calibration")?.and_then(|v| serde_json::from_str(&v).ok()).unwrap_or_default(),
                s.load_rules()?,
                s.get_setting("face_template")?.and_then(|v| serde_json::from_str(&v).ok()),
            ),
            None => (Config::default(), Calibration::default(), Rules::new(Rules::defaults()), None),
        };
        Ok(Self {
            cfg,
            cal,
            rules,
            store,
            template,
            extractor: Extractor::default(),
            smoother: Smoother::default(),
            live: None,
            last_storage_error: None,
        })
    }

    fn note<T>(&mut self, r: Result<T>) -> Option<T> {
        match r {
            Ok(v) => Some(v),
            Err(e) => {
                self.last_storage_error = Some(e.to_string());
                None
            }
        }
    }

    pub fn session_active(&self) -> bool {
        self.live.is_some()
    }

    pub fn start_session(&mut self, label: Option<String>, goal_min: Option<u32>, t: u64, wall: i64, tz: i32) -> Result<i64> {
        if self.live.is_some() {
            return Err(NdError::Invalid { msg: "a session is already running".into() });
        }
        let label = label.map(|l| l.trim().to_string()).filter(|l| !l.is_empty());
        let id = match &self.store {
            Some(s) => s.create_session(wall, tz, label.as_deref(), goal_min)?,
            None => LIVE_SESSION_ID,
        };
        self.extractor.reset();
        self.smoother.reset();
        self.live = Some(Live {
            id,
            label,
            goal_min,
            start_t: t,
            start_wall: wall,
            tz,
            closed: Vec::new(),
            open: None,
            mode: Mode::Running,
            last_t: t,
            last_persist_t: t,
            drift_since: None,
            nudged: false,
            last_nudge_t: None,
            neutral_ms: HashMap::new(),
            asked: HashSet::new(),
        });
        Ok(id)
    }

    /// Ends the open segment at `at` and opens a new one.
    fn transition(&mut self, at: u64, t: u64, to: Activity, detail: Option<String>) {
        let Some(live) = self.live.as_mut() else { return };
        let mut writes: Vec<Result<()>> = Vec::new();
        let at = match &live.open {
            Some(o) => at.clamp(o.start, t),
            None => at.min(t),
        };
        if let Some(mut o) = live.open.take() {
            o.end = at;
            if o.end > o.start {
                if let (Some(s), Some(id)) = (&self.store, o.db_id) {
                    writes.push(s.update_segment_end(id, live.start_wall + o.end.saturating_sub(live.start_t) as i64));
                }
                live.closed.push(o);
            } else if let (Some(s), Some(id)) = (&self.store, o.db_id) {
                writes.push(s.delete_segment(id));
            }
        }
        let mut seg = Seg { db_id: None, activity: to, start: at, end: t, detail, claimed: false };
        if let Some(s) = &self.store {
            match s.insert_segment(live.id, live.wall(at), live.wall(t), to, seg.detail.as_deref()) {
                Ok(id) => seg.db_id = Some(id),
                Err(e) => writes.push(Err(e)),
            }
        }
        if to.is_drift() {
            live.drift_since.get_or_insert(at);
        } else {
            live.drift_since = None;
            live.nudged = false;
        }
        live.open = Some(seg);
        live.last_persist_t = t;
        for w in writes {
            self.note(w);
        }
    }

    fn persist_progress(&mut self, t: u64, force: bool) {
        let Some(live) = self.live.as_mut() else { return };
        if !force && t.saturating_sub(live.last_persist_t) < PERSIST_EVERY_MS {
            return;
        }
        live.last_persist_t = t;
        let wall = live.wall(t);
        let (id, open_id) = (live.id, live.open.as_ref().and_then(|o| o.db_id));
        if let Some(s) = &self.store {
            let r1 = open_id.map_or(Ok(()), |oid| s.update_segment_end(oid, wall));
            let r2 = s.touch_session(id, wall);
            self.note(r1);
            self.note(r2);
        }
    }

    fn idle_update(&self, raw: Activity, zone: Zone) -> EngineUpdate {
        EngineUpdate {
            mode: SessionMode::Idle,
            activity: None,
            raw,
            zone,
            detail: None,
            focus_ms: 0,
            elapsed_ms: 0,
            present_ms: 0,
            goal_ms: None,
            break_left_ms: None,
            nudge: None,
            ask_category: None,
            welcome_back: false,
            too_dark: false,
            not_you: false,
            plan: SensingPlan { tick_ms: 5_000, landmarks: false, body: false, hands: false, objects: false, verify: false },
        }
    }

    pub fn ingest(&mut self, frame: &SignalFrame) -> EngineUpdate {
        let Some(live) = self.live.as_ref() else {
            return self.idle_update(Activity::Away, Zone::Absent);
        };
        // Never let time run backwards, whatever the caller sends.
        let t = frame.t_ms.max(live.last_t);
        let mut frame_t = frame.clone();
        frame_t.t_ms = t;
        let feats = self.extractor.update(&frame_t, &self.cal, &self.cfg, &self.rules);
        let raw = classify(&feats, &self.cfg);
        let mut welcome_back = false;
        let mut nudge = None;
        let mut ask = None;

        let mode = self.live.as_ref().map(|l| l.mode).unwrap_or(Mode::Running);
        match mode {
            Mode::Break { until } if t < until => {
                if let Some(o) = self.live.as_mut().and_then(|l| l.open.as_mut()) {
                    o.end = t;
                }
            }
            _ => {
                if let Mode::Break { .. } = mode {
                    if let Some(l) = self.live.as_mut() {
                        l.mode = Mode::Running;
                    }
                    self.smoother.force(t, raw);
                    self.transition(t, t, raw, detail_for(raw, &feats));
                } else if let Some(tr) = self.smoother.push_with_onset(t, raw, onset(raw, &feats), &self.cfg) {
                    self.transition(tr.at_ms, t, tr.to, detail_for(tr.to, &feats));
                } else {
                    // A drift that changes cause (one site to another) gets its
                    // own segment, so the breakdown names the right culprit.
                    let split = self.live.as_ref().and_then(|l| l.open.as_ref()).is_some_and(|o| {
                        o.activity == Activity::Social && raw == Activity::Social && o.detail != feats.app.detail && t > o.start
                    });
                    if split {
                        self.transition(t, t, Activity::Social, feats.app.detail.clone());
                    }
                }

                let cfg = self.cfg.clone();
                let live = self.live.as_mut().expect("live session");
                let dt = t.saturating_sub(live.last_t).min(5_000);
                if let Some(o) = live.open.as_mut() {
                    o.end = t;
                }
                let open = live.open.clone();

                if let Some(o) = &open {
                    if o.activity == Activity::Away && live.mode == Mode::Running && t.saturating_sub(o.start) >= ms(cfg.auto_pause_away_s) {
                        live.mode = Mode::Paused;
                    } else if live.mode == Mode::Paused && o.activity != Activity::Away {
                        live.mode = Mode::Running;
                        welcome_back = true;
                    }

                    if o.activity.is_drift() && live.mode == Mode::Running && cfg.nudges && !live.nudged {
                        let since = *live.drift_since.get_or_insert(o.start);
                        let drift_ms = t.saturating_sub(since);
                        let cooled = live.last_nudge_t.is_none_or(|n| t.saturating_sub(n) >= ms(cfg.nudge_cooldown_s));
                        if drift_ms >= ms(cfg.nudge_after_s) && cooled {
                            live.nudged = true;
                            live.last_nudge_t = Some(t);
                            nudge = Some(Nudge { activity: o.activity, detail: o.detail.clone(), drift_ms });
                        }
                    }

                    if o.activity == Activity::ScreenFocus && !feats.app.matched {
                        if let Some(host) = &feats.app.host {
                            let acc = live.neutral_ms.entry(host.clone()).or_default();
                            *acc += dt;
                            if *acc >= ms(cfg.ask_category_after_s) && live.asked.insert(host.clone()) {
                                ask = Some(host.clone());
                            }
                        }
                    }
                }
            }
        }

        self.persist_progress(t, false);
        let live = self.live.as_mut().expect("live session");
        live.last_t = t;

        let face_lost = !feats.face_visible;
        let desk = feats.zone == Zone::Desk;
        let tick_ms: u64 = if live.mode == Mode::Paused { 5_000 } else { 1_000 };
        let due = |last: Option<u64>, every: u64| last.is_none_or(|l| t.saturating_sub(l) + tick_ms >= every);
        let plan = SensingPlan {
            tick_ms: tick_ms as u32,
            landmarks: feats.face_visible,
            body: face_lost,
            hands: (desk || face_lost) && due(self.extractor.last_hands_run, 2_000),
            objects: due(self.extractor.last_objects_run, if desk || face_lost { 2_000 } else { 10_000 }),
            // Check at the start, after the face returns, every couple of
            // minutes, and every few seconds while a mismatch is unresolved.
            verify: self.template.is_some()
                && feats.face_visible
                && (feats.identity == Identity::Unknown
                    || ((feats.identity == Identity::Other || self.extractor.identity_in_doubt())
                        && due(self.extractor.last_verify, 3_000))
                    || due(self.extractor.last_verify, ms(self.cfg.verify_every_s))),
        };

        let (mode, break_left_ms) = match live.mode {
            Mode::Running => (SessionMode::Running, None),
            Mode::Paused => (SessionMode::Paused, None),
            Mode::Break { until } => (SessionMode::Break, Some(until.saturating_sub(t))),
        };
        EngineUpdate {
            mode,
            activity: live.open.as_ref().map(|o| o.activity),
            raw,
            zone: feats.zone,
            detail: live.open.as_ref().and_then(|o| o.detail.clone()),
            focus_ms: live.focus_ms(t),
            elapsed_ms: t.saturating_sub(live.start_t),
            present_ms: live.present_ms(t),
            goal_ms: live.goal_min.map(|m| m as u64 * 60_000),
            break_left_ms,
            nudge,
            ask_category: ask,
            welcome_back,
            too_dark: feats.too_dark,
            not_you: feats.identity == Identity::Other && feats.face_visible,
            plan,
        }
    }

    /// Sleep, lid close or screen lock: you're away from this moment, with
    /// no grace period, and the session pauses until you're back.
    pub fn system_pause(&mut self, t: u64) {
        let Some(live) = self.live.as_mut() else { return };
        if matches!(live.mode, Mode::Break { .. }) {
            return;
        }
        let t = t.max(live.last_t);
        live.mode = Mode::Paused;
        live.last_t = t;
        if live.open.as_ref().map(|o| o.activity) != Some(Activity::Away) {
            self.transition(t, t, Activity::Away, None);
        }
        self.smoother.force(t, Activity::Away);
        self.persist_progress(t, true);
    }

    pub fn start_break(&mut self, minutes: u32, t: u64) -> Result<()> {
        let live = self.live.as_mut().ok_or(NdError::Invalid { msg: "no session".into() })?;
        let t = t.max(live.last_t);
        live.mode = Mode::Break { until: t + minutes.clamp(1, 120) as u64 * 60_000 };
        live.last_t = t;
        self.transition(t, t, Activity::Break, None);
        self.smoother.force(t, Activity::Break);
        Ok(())
    }

    pub fn end_break(&mut self, t: u64) -> Result<()> {
        let live = self.live.as_mut().ok_or(NdError::Invalid { msg: "no session".into() })?;
        if let Mode::Break { .. } = live.mode {
            live.mode = Mode::Break { until: t.max(live.last_t) };
        }
        Ok(())
    }

    fn live_rows(&self) -> Option<(SessionRow, Vec<SegmentRow>)> {
        let live = self.live.as_ref()?;
        let rows = live
            .closed
            .iter()
            .chain(live.open.iter())
            .enumerate()
            .filter(|(_, s)| s.end > s.start)
            .map(|(i, s)| SegmentRow {
                id: s.db_id.unwrap_or(-(i as i64) - 1),
                session_id: live.id,
                start_wall: live.wall(s.start),
                end_wall: live.wall(s.end),
                activity: s.activity,
                detail: s.detail.clone(),
                claimed: s.claimed,
                tz_offset_min: live.tz,
            })
            .collect();
        let sess = SessionRow {
            id: live.id,
            started_wall: live.start_wall,
            ended_wall: Some(live.wall(live.last_t)),
            tz_offset_min: live.tz,
            label: live.label.clone(),
            goal_min: live.goal_min,
            closed: false,
        };
        Some((sess, rows))
    }

    pub fn live_segments(&self) -> Vec<SegmentView> {
        self.live_rows().map(|(_, rows)| rows.iter().map(analytics::view).collect()).unwrap_or_default()
    }

    pub fn end_session(&mut self, t: u64) -> Result<Option<SessionSummary>> {
        let Some(live) = self.live.as_mut() else { return Ok(None) };
        let t = t.max(live.last_t);
        if let Some(o) = live.open.as_mut() {
            o.end = t;
        }
        live.last_t = t;
        self.persist_progress(t, true);
        let (mut sess, rows) = self.live_rows().expect("live session");
        sess.closed = true;
        if let Some(s) = &self.store {
            s.close_session(sess.id, sess.ended_wall.unwrap_or(sess.started_wall))?;
        }
        self.live = None;
        self.smoother.reset();
        self.extractor.reset();
        let summary = match &self.store {
            Some(s) => analytics::session_summary(&sess, &s.session_segments(sess.id)?),
            None => analytics::session_summary(&sess, &rows),
        };
        Ok(Some(summary))
    }

    pub fn resolve_unclear(&mut self, segment_id: i64, worked: bool, wall: i64) -> Result<()> {
        if let Some(live) = self.live.as_mut() {
            for s in live.closed.iter_mut().chain(live.open.iter_mut()) {
                if s.db_id == Some(segment_id) {
                    s.claimed = worked;
                }
            }
        }
        match &self.store {
            Some(s) => s.set_claimed(segment_id, worked, wall),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::{self, Scenario};

    fn run(core: &mut EngineCore, frames: &[SignalFrame]) -> Vec<EngineUpdate> {
        frames.iter().map(|f| core.ingest(f)).collect()
    }

    #[test]
    fn idle_without_session() {
        let mut core = EngineCore::new(None).unwrap();
        let u = core.ingest(&SignalFrame::default());
        assert_eq!(u.mode, SessionMode::Idle);
    }

    #[test]
    fn nudges_once_per_drift_and_respects_cooldown() {
        let mut core = EngineCore::new(None).unwrap();
        let cal = core.cal.clone();
        core.start_session(None, None, 0, 0, 0).unwrap();
        let mut frames = sim::frames(Scenario::Typing, 0, 60, &cal, 1);
        frames.extend(sim::frames(Scenario::Instagram, 60_000, 90, &cal, 2));
        frames.extend(sim::frames(Scenario::Typing, 150_000, 30, &cal, 3));
        frames.extend(sim::frames(Scenario::Instagram, 180_000, 90, &cal, 4));
        let ups = run(&mut core, &frames);
        let nudges: Vec<_> = ups.iter().filter_map(|u| u.nudge.clone()).collect();
        assert_eq!(nudges.len(), 1, "second drift is inside the cooldown");
        assert_eq!(nudges[0].activity, Activity::Social);
        assert_eq!(nudges[0].detail.as_deref(), Some("instagram.com"));
        let at = ups.iter().position(|u| u.nudge.is_some()).unwrap() as u64;
        assert!((88..=92).contains(&at), "nudged {at}s in, drift began at 60s");
    }

    #[test]
    fn auto_pause_and_welcome_back() {
        let mut core = EngineCore::new(None).unwrap();
        let cal = core.cal.clone();
        core.start_session(None, None, 0, 0, 0).unwrap();
        let mut frames = sim::frames(Scenario::Typing, 0, 30, &cal, 1);
        frames.extend(sim::frames(Scenario::LeftDesk, 30_000, 400, &cal, 2));
        frames.extend(sim::frames(Scenario::Typing, 430_000, 30, &cal, 3));
        let ups = run(&mut core, &frames);
        assert!(ups.iter().any(|u| u.mode == SessionMode::Paused));
        assert!(ups.iter().any(|u| u.welcome_back));
        assert_eq!(ups.last().unwrap().mode, SessionMode::Running);
        assert!(ups.iter().all(|u| u.nudge.is_none()), "leaving the desk is not a drift");
    }

    #[test]
    fn someone_else_needs_two_failed_checks() {
        let mut core = EngineCore::new(None).unwrap();
        core.template = Some(crate::identity::FaceTemplate { mean: vec![1.0, 0.0], exemplars: vec![] });
        let cal = core.cal.clone();
        core.start_session(None, None, 0, 0, 0).unwrap();
        let mut frames = sim::frames(Scenario::Typing, 0, 80, &cal, 1);
        frames[1].is_user = Some(true);
        frames[60].is_user = Some(false);
        frames[63].is_user = Some(false);
        let ups = run(&mut core, &frames);
        assert!(ups[0].plan.verify, "check at the start until confirmed");
        assert!(ups[2..59].iter().all(|u| !u.plan.verify), "confirmed, so no check for a while");
        assert!(!ups[60].not_you && !ups[61].not_you, "one failed check isn't enough");
        assert!(ups[62].plan.verify, "a failed check is retried within seconds");
        assert!(ups[63].not_you, "two in a row is someone else");
        let s = core.end_session(80_000).unwrap().unwrap();
        assert!(s.totals.away_ms >= 15_000, "time with someone else in the chair isn't yours: {}", s.totals.away_ms);
    }

    #[test]
    fn system_pause_is_immediate_and_resumes() {
        let mut core = EngineCore::new(None).unwrap();
        let cal = core.cal.clone();
        core.start_session(None, None, 0, 0, 0).unwrap();
        run(&mut core, &sim::frames(Scenario::Typing, 0, 60, &cal, 1));
        core.system_pause(60_000);
        // Laptop asleep for an hour; the first frame after wake is typing.
        let ups = run(&mut core, &sim::frames(Scenario::Typing, 3_660_000, 30, &cal, 2));
        assert!(ups.iter().any(|u| u.welcome_back));
        let s = core.end_session(3_690_000).unwrap().unwrap();
        assert!(s.totals.away_ms >= 3_600_000);
        assert!(s.totals.focus_ms <= 90_000);
    }

    #[test]
    fn break_excludes_time_and_resumes() {
        let mut core = EngineCore::new(None).unwrap();
        let cal = core.cal.clone();
        core.start_session(None, None, 0, 0, 0).unwrap();
        run(&mut core, &sim::frames(Scenario::Typing, 0, 60, &cal, 1));
        core.start_break(5, 60_000).unwrap();
        let ups = run(&mut core, &sim::frames(Scenario::Phone, 60_000, 400, &cal, 2));
        assert!(ups[..300].iter().all(|u| u.nudge.is_none() && u.mode == SessionMode::Break), "no nudges during a break");
        assert!(ups[300..].iter().all(|u| u.mode == SessionMode::Running));
        let summary = core.end_session(460_000).unwrap().unwrap();
        assert_eq!(summary.totals.break_ms, 300_000);
        assert!(summary.totals.focus_ms >= 59_000 && summary.totals.focus_ms <= 60_000);
        assert!(summary.totals.drift_ms >= 99_000);
    }

    #[test]
    fn persists_and_reports() {
        let store = Store::open_in_memory().unwrap();
        let mut core = EngineCore::new(Some(store)).unwrap();
        let cal = core.cal.clone();
        let wall0 = crate::time::parse_date("2026-10-12").unwrap() * crate::time::DAY_MS;
        let id = core.start_session(Some("Chem".into()), Some(90), 0, wall0, 0).unwrap();
        let mut frames = sim::frames(Scenario::Typing, 0, 600, &cal, 1);
        frames.extend(sim::frames(Scenario::StillAtDesk, 600_000, 300, &cal, 2));
        for f in frames.iter_mut() {
            f.wall_ms = wall0 + f.t_ms as i64;
        }
        run(&mut core, &frames);
        let summary = core.end_session(900_000).unwrap().unwrap();
        assert_eq!(summary.session_id, id);
        assert_eq!(summary.label.as_deref(), Some("Chem"));
        let unclear = summary.unclear.first().expect("still-at-desk time becomes unclear");
        assert!(unclear.at_desk);
        let before = summary.totals.focus_ms;
        core.resolve_unclear(unclear.segment_id, true, wall0 + 1_000_000).unwrap();
        let rows = core.store.as_ref().unwrap().session_segments(id).unwrap();
        let after = crate::analytics::session_summary(&core.store.as_ref().unwrap().session(id).unwrap().unwrap(), &rows);
        assert_eq!(after.totals.focus_ms, before + (unclear.end_wall - unclear.start_wall));
        assert!(core.store.as_ref().unwrap().open_sessions().unwrap().is_empty());
    }
}
