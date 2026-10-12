//! Folio core: every decision about focus lives here, behind a small
//! UniFFI surface the Swift shell calls once per second.

uniffi::setup_scaffolding!();

pub mod analytics;
pub mod classify;
pub mod config;
pub mod demo;
pub mod engine;
pub mod error;
pub mod eval;
pub mod face;
pub mod features;
pub mod identity;
pub mod rules;
pub mod signal;
pub mod sim;
pub mod smooth;
pub mod store;
pub mod time;
pub mod zone;

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Arc, Mutex};

pub use analytics::{Cause, DayReport, RangeReport, RibbonPiece, SegmentView, SessionSummary, Totals, UnclearSpan};
pub use classify::Activity;
pub use config::Config;
pub use engine::{EngineCore, EngineUpdate, Nudge, SensingPlan, SessionMode};
pub use error::NdError;
pub use identity::FaceTemplate;
pub use rules::{Category, Rule, RuleKind};
pub use signal::{Detection, FaceSig, ObjectsSig, Point, Rect, SignalFrame};
pub use zone::{Calibration, Zone};

use error::Result;
use time::DAY_MS;

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct FaceMatch {
    pub score: f32,
    pub is_user: bool,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct InterruptedSession {
    pub session_id: i64,
    pub label: Option<String>,
    pub goal_min: Option<u32>,
    pub started_wall: i64,
    pub ended_wall: i64,
}

/// The face recognition model, loaded once from the app bundle.
#[derive(uniffi::Object)]
pub struct FaceModel {
    net: face::FaceNet,
}

#[uniffi::export]
impl FaceModel {
    #[uniffi::constructor]
    pub fn new(path: String) -> Result<Arc<Self>> {
        Ok(Arc::new(Self { net: face::FaceNet::load(&path)? }))
    }

    /// Embedding for the face whose five landmarks (pixel coordinates, top-left
    /// origin: left eye, right eye, nose tip, left and right mouth corner, as
    /// seen in the image) are given, in an RGBA frame.
    pub fn embed(&self, rgba: Vec<u8>, width: u32, height: u32, landmarks: Vec<Point>) -> Result<Vec<f32>> {
        match catch_unwind(AssertUnwindSafe(|| self.net.embed(&rgba, width as usize, height as usize, &landmarks))) {
            Ok(r) => r,
            Err(_) => Err(NdError::Internal { msg: "face model panicked".into() }),
        }
    }
}

#[derive(uniffi::Object)]
pub struct Engine {
    inner: Mutex<EngineCore>,
}

impl Engine {
    /// Runs `f` with the lock held. A panic inside never crosses the FFI
    /// boundary; it comes back as an error and the engine stays usable.
    fn with<T>(&self, f: impl FnOnce(&mut EngineCore) -> Result<T>) -> Result<T> {
        let mut guard = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        match catch_unwind(AssertUnwindSafe(|| f(&mut guard))) {
            Ok(r) => r,
            Err(p) => {
                let msg = p
                    .downcast_ref::<&str>()
                    .map(|s| s.to_string())
                    .or_else(|| p.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "panic".into());
                Err(NdError::Internal { msg })
            }
        }
    }

    fn save_json<T: serde::Serialize>(core: &EngineCore, key: &str, v: &T) -> Result<()> {
        match &core.store {
            Some(s) => s.set_setting(key, &serde_json::to_string(v)?),
            None => Ok(()),
        }
    }
}

#[uniffi::export]
impl Engine {
    /// `db_path` None keeps everything in memory (previews, tests).
    #[uniffi::constructor]
    pub fn new(db_path: Option<String>) -> Result<Arc<Self>> {
        let store = db_path.map(|p| store::Store::open(&p)).transpose()?;
        Ok(Arc::new(Self { inner: Mutex::new(EngineCore::new(store)?) }))
    }

    pub fn integrity_ok(&self) -> bool {
        self.with(|c| Ok(c.store.as_ref().is_none_or(|s| s.integrity_ok()))).unwrap_or(false)
    }

    pub fn last_storage_error(&self) -> Option<String> {
        self.with(|c| Ok(c.last_storage_error.take())).ok().flatten()
    }

    pub fn config(&self) -> Config {
        self.with(|c| Ok(c.cfg.clone())).unwrap_or_default()
    }

    pub fn set_config(&self, cfg: Config) -> Result<()> {
        self.with(|c| {
            c.cfg = cfg;
            Self::save_json(c, "config", &c.cfg)
        })
    }

    pub fn calibration(&self) -> Calibration {
        self.with(|c| Ok(c.cal.clone())).unwrap_or_default()
    }

    pub fn set_calibration(&self, cal: Calibration) -> Result<()> {
        self.with(|c| {
            c.cal = cal;
            Self::save_json(c, "calibration", &c.cal)
        })
    }

    pub fn has_face(&self) -> bool {
        self.with(|c| Ok(c.template.is_some())).unwrap_or(false)
    }

    /// Builds the face template from enrollment embeddings.
    pub fn set_face_samples(&self, samples: Vec<Vec<f32>>) -> Result<()> {
        self.with(|c| {
            let t = identity::build_template(&samples, 5)
                .ok_or(NdError::Invalid { msg: "no usable face samples".into() })?;
            c.template = Some(t);
            Self::save_json(c, "face_template", &c.template)
        })
    }

    /// "Add a look": keeps the current template and widens it.
    pub fn add_face_samples(&self, samples: Vec<Vec<f32>>) -> Result<()> {
        self.with(|c| {
            let mut all: Vec<Vec<f32>> = c.template.as_ref().map(|t| t.exemplars.clone()).unwrap_or_default();
            all.extend(samples);
            let t = identity::build_template(&all, 8).ok_or(NdError::Invalid { msg: "no usable face samples".into() })?;
            c.template = Some(t);
            Self::save_json(c, "face_template", &c.template)
        })
    }

    pub fn clear_face(&self) -> Result<()> {
        self.with(|c| {
            c.template = None;
            match &c.store {
                Some(s) => s.delete_setting("face_template"),
                None => Ok(()),
            }
        })
    }

    /// None when no face is enrolled. Strong matches slowly adapt the template.
    pub fn match_face(&self, embedding: Vec<f32>) -> Option<FaceMatch> {
        self.with(|c| {
            let threshold = c.cfg.face_match_threshold;
            let Some(t) = c.template.as_mut() else { return Ok(None) };
            let score = identity::score(t, &embedding);
            if score >= threshold + 0.2 {
                identity::adapt(t, &embedding, 0.02);
            }
            Ok(Some(FaceMatch { score, is_user: score >= threshold }))
        })
        .ok()
        .flatten()
    }

    pub fn rules(&self) -> Vec<Rule> {
        self.with(|c| Ok(c.rules.list().to_vec())).unwrap_or_default()
    }

    pub fn set_rule(&self, rule: Rule) -> Result<()> {
        self.with(|c| {
            c.rules.set(rule.clone());
            let saved = c.rules.list().iter().find(|r| r.kind == rule.kind && rules::normalize_host(&r.pattern) == rules::normalize_host(&rule.pattern)).cloned();
            match (&c.store, saved) {
                (Some(s), Some(r)) => s.save_rule(&r),
                _ => Ok(()),
            }
        })
    }

    pub fn remove_rule(&self, pattern: String, kind: RuleKind) -> Result<()> {
        self.with(|c| {
            c.rules.remove(&pattern, kind);
            match &c.store {
                Some(s) => s.delete_rule(&pattern, kind),
                None => Ok(()),
            }
        })
    }

    pub fn session_active(&self) -> bool {
        self.with(|c| Ok(c.session_active())).unwrap_or(false)
    }

    pub fn start_session(&self, label: Option<String>, goal_min: Option<u32>, t_ms: u64, wall_ms: i64, tz_offset_min: i32) -> Result<i64> {
        self.with(|c| c.start_session(label, goal_min, t_ms, wall_ms, tz_offset_min))
    }

    pub fn ingest(&self, frame: SignalFrame) -> Result<EngineUpdate> {
        self.with(|c| Ok(c.ingest(&frame)))
    }

    pub fn system_pause(&self, t_ms: u64) -> Result<()> {
        self.with(|c| {
            c.system_pause(t_ms);
            Ok(())
        })
    }

    pub fn start_break(&self, minutes: u32, t_ms: u64) -> Result<()> {
        self.with(|c| c.start_break(minutes, t_ms))
    }

    pub fn end_break(&self, t_ms: u64) -> Result<()> {
        self.with(|c| c.end_break(t_ms))
    }

    pub fn end_session(&self, t_ms: u64) -> Result<Option<SessionSummary>> {
        self.with(|c| c.end_session(t_ms))
    }

    pub fn live_segments(&self) -> Vec<SegmentView> {
        self.with(|c| Ok(c.live_segments())).unwrap_or_default()
    }

    pub fn session_summary(&self, session_id: i64) -> Result<Option<SessionSummary>> {
        self.with(|c| {
            let Some(s) = &c.store else { return Ok(None) };
            let Some(sess) = s.session(session_id)? else { return Ok(None) };
            Ok(Some(analytics::session_summary(&sess, &s.session_segments(session_id)?)))
        })
    }

    pub fn resolve_unclear(&self, segment_id: i64, worked: bool, wall_ms: i64) -> Result<()> {
        self.with(|c| c.resolve_unclear(segment_id, worked, wall_ms))
    }

    pub fn relabel_segment(&self, segment_id: i64, activity: Activity, wall_ms: i64) -> Result<()> {
        self.with(|c| match &c.store {
            Some(s) => s.set_segment_activity(segment_id, activity, wall_ms),
            None => Ok(()),
        })
    }

    /// Report for `days` local days starting at `from_date` (YYYY-MM-DD),
    /// with the same-length period before it for comparison.
    pub fn report(&self, from_date: String, days: u32) -> Result<RangeReport> {
        self.with(|c| {
            let from = time::parse_date(&from_date).ok_or(NdError::Invalid { msg: format!("bad date {from_date}") })?;
            let days = days.clamp(1, 366);
            let pad = 15 * 3_600_000;
            let (a, b) = (from * DAY_MS - pad, (from + days as i64) * DAY_MS + pad);
            let (pa, pb) = ((from - days as i64) * DAY_MS - pad, from * DAY_MS + pad);
            let (segs, prev) = match &c.store {
                Some(s) => (s.segments_between(a, b)?, s.segments_between(pa, pb)?),
                None => (Vec::new(), Vec::new()),
            };
            let prev_from = from - days as i64;
            let prev: Vec<_> = prev
                .into_iter()
                .filter(|s| {
                    let d = time::local_ms(s.start_wall, s.tz_offset_min).div_euclid(DAY_MS);
                    d >= prev_from && d < from
                })
                .collect();
            Ok(analytics::range_report(&segs, &prev, from, days))
        })
    }

    pub fn interrupted_session(&self) -> Result<Option<InterruptedSession>> {
        self.with(|c| {
            if c.session_active() {
                return Ok(None);
            }
            let Some(s) = &c.store else { return Ok(None) };
            Ok(s.open_sessions()?.into_iter().last().map(|r| InterruptedSession {
                session_id: r.id,
                label: r.label,
                goal_min: r.goal_min,
                started_wall: r.started_wall,
                ended_wall: r.ended_wall.unwrap_or(r.started_wall),
            }))
        })
    }

    /// Closes sessions left open by a crash, at their last saved moment.
    pub fn close_interrupted(&self) -> Result<Option<SessionSummary>> {
        self.with(|c| {
            if c.session_active() {
                return Ok(None);
            }
            let Some(s) = &c.store else { return Ok(None) };
            let mut last = None;
            for r in s.open_sessions()? {
                let end = r.ended_wall.unwrap_or(r.started_wall);
                s.close_session(r.id, end)?;
                last = Some(analytics::session_summary(&r, &s.session_segments(r.id)?));
            }
            Ok(last)
        })
    }

    pub fn export_csv(&self, path: String) -> Result<u32> {
        self.with(|c| match &c.store {
            Some(s) => s.export_csv(&path),
            None => Ok(0),
        })
    }

    /// Development only: synthetic history for previews and screenshots.
    pub fn seed_demo(&self, days: u32, now_wall_ms: i64, tz_offset_min: i32) -> Result<Option<i64>> {
        self.with(|c| demo::seed(c, days, now_wall_ms, tz_offset_min))
    }

    pub fn delete_all_data(&self) -> Result<()> {
        self.with(|c| {
            c.template = None;
            c.cal = Calibration::default();
            match &c.store {
                Some(s) => s.delete_all(),
                None => Ok(()),
            }
        })
    }
}

#[uniffi::export]
pub fn format_duration_ms(ms: i64) -> String {
    time::format_duration(ms)
}

/// One JSONL line for the label-mode recorder (same format nd-replay reads).
#[uniffi::export]
pub fn frame_to_json(frame: SignalFrame) -> String {
    serde_json::to_string(&frame).unwrap_or_default()
}

#[uniffi::export]
pub fn calibration_to_json(cal: Calibration) -> String {
    serde_json::to_string(&cal).unwrap_or_default()
}

#[uniffi::export]
pub fn core_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn arb_frame() -> impl Strategy<Value = SignalFrame> {
        (
            any::<u16>(),
            prop::option::of((any::<f32>(), any::<f32>(), prop::option::of(any::<f32>()))),
            prop::option::of(any::<bool>()),
            prop::option::of(prop::collection::vec((any::<f32>(), any::<f32>()), 0..3)),
            prop::option::of((any::<f32>(), any::<f32>(), any::<f32>())),
            any::<f32>(),
            prop::option::of(prop::sample::select(vec!["instagram.com", "docs.google.com", "", "a.b"])),
            any::<bool>(),
            any::<f32>(),
        )
            .prop_map(|(dt, face, body, hands, objs, idle, host, cam, bright)| SignalFrame {
                t_ms: dt as u64,
                face: face.map(|(y, p, e)| FaceSig { yaw: y, pitch: p, eye_open: e, ..Default::default() }),
                body_present: body,
                hands: hands.map(|v| v.into_iter().map(|(x, y)| Point { x, y }).collect()),
                objects: objs.map(|(pc, py, bc)| ObjectsSig {
                    phone: Some(Detection { conf: pc, bbox: Rect { x: 0.4, y: py, w: 0.1, h: 0.1 } }),
                    book: Some(Detection { conf: bc, ..Default::default() }),
                }),
                input_idle_s: idle,
                url_host: host.map(str::to_string),
                app_bundle: Some("com.apple.Safari".into()),
                camera_ok: cam,
                brightness: bright,
                ..Default::default()
            })
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        /// Arbitrary (even garbage) input never panics, focus never exceeds
        /// elapsed time, and segments never overlap.
        #[test]
        fn engine_invariants(frames in prop::collection::vec(arb_frame(), 1..200)) {
            let engine = Engine::new(None).unwrap();
            engine.start_session(None, None, 0, 0, 0).unwrap();
            let mut t = 0u64;
            for mut f in frames {
                t += f.t_ms % 3000;
                f.t_ms = t;
                let u = engine.ingest(f).unwrap();
                prop_assert!(u.focus_ms <= u.elapsed_ms + 1);
                prop_assert!(u.present_ms <= u.elapsed_ms + 1);
            }
            let segs = engine.live_segments();
            for w in segs.windows(2) {
                prop_assert!(w[0].end_wall <= w[1].start_wall);
                prop_assert!(w[0].start_wall < w[0].end_wall);
            }
            let s = engine.end_session(t + 1000).unwrap().unwrap();
            prop_assert!(s.totals.focus_ms <= s.ended_wall - s.started_wall);
        }
    }

    #[test]
    fn demo_history_reports() {
        let e = Engine::new(Some(":memory:".into())).unwrap();
        let now = time::parse_date("2026-10-12").unwrap() * DAY_MS + 22 * 3_600_000;
        let last = e.seed_demo(14, now, 0).unwrap();
        assert!(last.is_some());
        let r = e.report("2026-10-06".into(), 7).unwrap();
        assert!(r.totals.focus_ms > 3_600_000, "a week of demo study");
        assert!(r.prev_focus_ms > 0);
        assert!(!r.causes.is_empty());
        assert!(!r.insights.is_empty(), "{:?}", r.insights);
        let s = e.session_summary(last.unwrap()).unwrap().unwrap();
        assert!(s.totals.focus_ms > 0);
    }

    #[test]
    fn engine_survives_reopen_and_reports_interrupted() {
        let dir = std::env::temp_dir().join(format!("nd-test-{}", std::process::id()));
        let path = dir.join("nd.sqlite").to_string_lossy().to_string();
        let _ = std::fs::remove_dir_all(&dir);
        {
            let e = Engine::new(Some(path.clone())).unwrap();
            assert!(e.integrity_ok());
            e.start_session(Some("Chem".into()), None, 0, 1_000_000, 540).unwrap();
            let cal = e.calibration();
            for f in sim::frames(sim::Scenario::Typing, 0, 30, &cal, 1) {
                e.ingest(SignalFrame { wall_ms: 1_000_000 + f.t_ms as i64, ..f }).unwrap();
            }
            // Dropped without end_session: simulates a crash.
        }
        let e = Engine::new(Some(path)).unwrap();
        let i = e.interrupted_session().unwrap().expect("interrupted session found");
        assert_eq!(i.label.as_deref(), Some("Chem"));
        assert!(i.ended_wall - i.started_wall >= 27_000, "lost at most a few seconds");
        let s = e.close_interrupted().unwrap().unwrap();
        assert!(s.totals.focus_ms >= 27_000);
        assert!(e.interrupted_session().unwrap().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
