//! Turns raw frames into evidence. This is the only stateful step before
//! classification: rolling windows, last-seen objects, identity.

use std::collections::VecDeque;

use crate::config::{ms, Config};
use crate::rules::{Categorized, Category, Rules};
use crate::signal::{Detection, Point, SignalFrame};
use crate::zone::{desk_progress, zone_for_pose, Calibration, Zone};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Identity {
    Unknown,
    User,
    Other,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Features {
    pub t_ms: u64,
    pub zone: Zone,
    pub face_visible: bool,
    pub identity: Identity,
    pub head_motion: f32,
    pub eyes_closed_s: f32,
    pub eyes_closed_since: Option<u64>,
    /// When the face was last seen, while it's out of view.
    pub face_last_seen: Option<u64>,
    pub hand_motion: f32,
    pub writing: bool,
    pub phone_held: bool,
    pub book_seen: bool,
    pub input_idle_s: f32,
    pub app: Categorized,
    pub locked: bool,
    pub sensing_ok: bool,
    pub too_dark: bool,
    pub desk_no_evidence_s: f32,
}

const POSE_WINDOW_MS: u64 = 8_000;
const HAND_STALE_MS: u64 = 6_000;

#[derive(Debug, Default)]
pub struct Extractor {
    last_zone: Option<Zone>,
    last_progress: f32,
    last_face_t: Option<u64>,
    pose_hist: VecDeque<(u64, f32, f32)>,
    eyes_closed_since: Option<u64>,
    prev_hands: Option<(u64, Vec<Point>)>,
    hand_motion: f32,
    hand_motion_t: u64,
    hands_low: bool,
    phone_last: Option<(u64, Detection)>,
    phone_near_hands: bool,
    book_last_t: Option<u64>,
    identity: Option<Identity>,
    identity_fails: u32,
    desk_since: Option<u64>,
    desk_evidence_t: Option<u64>,
    pub last_hands_run: Option<u64>,
    pub last_objects_run: Option<u64>,
    pub last_verify: Option<u64>,
}

fn finite(v: f32) -> bool {
    v.is_finite()
}

fn std_dev(values: impl Iterator<Item = f32> + Clone) -> f32 {
    let n = values.clone().count() as f32;
    if n < 2.0 {
        return 0.0;
    }
    let mean = values.clone().sum::<f32>() / n;
    (values.map(|v| (v - mean) * (v - mean)).sum::<f32>() / n).sqrt()
}

impl Extractor {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn identity(&self) -> Identity {
        self.identity.unwrap_or(Identity::Unknown)
    }

    /// A check failed recently and hasn't been confirmed either way.
    pub fn identity_in_doubt(&self) -> bool {
        self.identity_fails > 0
    }

    pub fn update(&mut self, f: &SignalFrame, cal: &Calibration, cfg: &Config, rules: &Rules) -> Features {
        let t = f.t_ms;
        let face = f.face.as_ref().filter(|fc| finite(fc.yaw) && finite(fc.pitch));
        let face_visible = face.is_some();

        // Zone, with continuity when the face drops out (common when looking
        // steeply down at a book).
        let zone = match face {
            Some(fc) => {
                self.last_face_t = Some(t);
                self.last_progress = desk_progress(cal, cfg, fc.pitch);
                zone_for_pose(cal, cfg, fc.yaw, fc.pitch)
            }
            None => {
                let recently_seen = self.last_face_t.is_some_and(|lt| t.saturating_sub(lt) <= ms(cfg.face_lost_grace_s));
                let body = f.body_present;
                let heading_down = self.last_zone == Some(Zone::Desk) || self.last_progress > 0.3;
                match body {
                    Some(true) if heading_down => Zone::Desk,
                    Some(true) => Zone::Elsewhere,
                    Some(false) if recently_seen => self.last_zone.unwrap_or(Zone::Absent),
                    Some(false) => Zone::Absent,
                    None if recently_seen => self.last_zone.unwrap_or(Zone::Absent),
                    None if heading_down => Zone::Desk,
                    None => Zone::Absent,
                }
            }
        };
        if zone == Zone::Desk {
            self.desk_since.get_or_insert(t);
        } else {
            self.desk_since = None;
        }
        self.last_zone = Some(zone);

        // Identity: a check result latches until the face is lost long enough
        // that it could be someone else. One bad angle isn't enough to decide
        // it's someone else; two failed checks in a row are.
        if let Some(is_user) = f.is_user {
            self.last_verify = Some(t);
            if is_user {
                self.identity = Some(Identity::User);
                self.identity_fails = 0;
            } else {
                self.identity_fails += 1;
                if self.identity_fails >= 2 {
                    self.identity = Some(Identity::Other);
                }
            }
        }
        if !face_visible && self.last_face_t.is_some_and(|lt| t.saturating_sub(lt) > ms(cfg.reverify_after_loss_s)) {
            self.identity = None;
            self.identity_fails = 0;
        }

        // Head micro-motion over a short window.
        if let Some(fc) = face {
            self.pose_hist.push_back((t, fc.yaw, fc.pitch));
        }
        while self.pose_hist.front().is_some_and(|(pt, _, _)| t.saturating_sub(*pt) > POSE_WINDOW_MS) {
            self.pose_hist.pop_front();
        }
        let head_motion = if face_visible && self.pose_hist.len() >= 3 {
            let yaw = std_dev(self.pose_hist.iter().map(|p| p.1));
            let pitch = std_dev(self.pose_hist.iter().map(|p| p.2));
            (yaw * yaw + pitch * pitch).sqrt()
        } else {
            0.0
        };

        // Eyes, compared against the baseline for the zone we're in (eyelids
        // naturally lower when reading at a desk).
        let eye = face.and_then(|fc| fc.eye_open).filter(|e| finite(*e));
        match eye {
            Some(e) => {
                let baseline = if zone == Zone::Desk { cal.desk_eye_open } else { cal.screen_eye_open }.max(0.01);
                if e < baseline * cfg.eyes_closed_ratio {
                    self.eyes_closed_since.get_or_insert(t);
                } else {
                    self.eyes_closed_since = None;
                }
            }
            None if !face_visible => self.eyes_closed_since = None,
            None => {}
        }
        let eyes_closed_s = self.eyes_closed_since.map_or(0.0, |s| t.saturating_sub(s) as f32 / 1000.0);

        // Hands: centroid displacement between runs.
        if let Some(hands) = &f.hands {
            self.last_hands_run = Some(t);
            let hands: Vec<Point> = hands.iter().copied().filter(|p| finite(p.x) && finite(p.y)).collect();
            if let Some((pt, prev)) = &self.prev_hands {
                let dt = t.saturating_sub(*pt) as f32 / 1000.0;
                if dt > 0.0 && !hands.is_empty() && !prev.is_empty() {
                    let moved: f32 = hands
                        .iter()
                        .map(|h| prev.iter().map(|p| ((h.x - p.x).powi(2) + (h.y - p.y).powi(2)).sqrt()).fold(f32::MAX, f32::min))
                        .sum::<f32>()
                        / hands.len() as f32;
                    self.hand_motion = moved / dt;
                } else {
                    self.hand_motion = 0.0;
                }
            }
            self.hand_motion_t = t;
            self.hands_low = hands.iter().any(|h| h.y < 0.5);
            self.prev_hands = Some((t, hands));
        }
        if t.saturating_sub(self.hand_motion_t) > HAND_STALE_MS {
            self.hand_motion = 0.0;
            self.hands_low = false;
        }
        let writing = self.hands_low && self.hand_motion >= cfg.hand_motion_min;

        // Objects. A phone counts when it's being held: near a hand, or raised
        // above the bottom edge of the frame rather than lying on the desk.
        if let Some(objects) = &f.objects {
            self.last_objects_run = Some(t);
            match objects.phone.as_ref().filter(|p| p.conf >= cfg.phone_conf) {
                Some(p) => {
                    let (cx, cy) = p.bbox.center();
                    let near_hand = self
                        .prev_hands
                        .as_ref()
                        .filter(|(ht, _)| t.saturating_sub(*ht) <= HAND_STALE_MS)
                        .is_some_and(|(_, hs)| hs.iter().any(|h| ((h.x - cx).powi(2) + (h.y - cy).powi(2)).sqrt() < 0.2));
                    self.phone_near_hands = near_hand || cy >= 0.2;
                    self.phone_last = Some((t, p.clone()));
                }
                None => {
                    self.phone_last = None;
                    self.phone_near_hands = false;
                }
            }
            if objects.book.as_ref().is_some_and(|b| b.conf >= cfg.book_conf) {
                self.book_last_t = Some(t);
            }
        }
        let phone_held = self.phone_near_hands
            && self.phone_last.as_ref().is_some_and(|(pt, _)| t.saturating_sub(*pt) <= ms(cfg.phone_hold_s));
        let book_seen = self.book_last_t.is_some_and(|bt| t.saturating_sub(bt) <= ms(cfg.book_hold_s));

        // How long we've been at the desk without any sign of study.
        if zone == Zone::Desk && (book_seen || writing || head_motion >= cfg.read_motion_min) {
            self.desk_evidence_t = Some(t);
        }
        let desk_no_evidence_s = match self.desk_since {
            Some(since) => {
                let from = self.desk_evidence_t.map_or(since, |e| e.max(since));
                t.saturating_sub(from) as f32 / 1000.0
            }
            None => 0.0,
        };

        let too_dark = f.camera_ok && f.brightness.is_finite() && f.brightness < cfg.min_brightness;
        let sensing_ok = f.camera_ok && !too_dark;

        let app = rules.categorize(
            f.app_bundle.as_deref(),
            f.app_name.as_deref(),
            f.url_host.as_deref(),
            f.window_title.as_deref(),
        );

        Features {
            t_ms: t,
            zone,
            face_visible,
            identity: self.identity(),
            head_motion,
            eyes_closed_s,
            eyes_closed_since: self.eyes_closed_since,
            face_last_seen: if face_visible { None } else { self.last_face_t },
            hand_motion: self.hand_motion,
            writing,
            phone_held,
            book_seen,
            input_idle_s: if f.input_idle_s.is_finite() { f.input_idle_s.max(0.0) } else { f32::MAX },
            app,
            locked: f.screen_locked,
            sensing_ok,
            too_dark,
            desk_no_evidence_s,
        }
    }
}

impl Features {
    pub fn category(&self) -> Category {
        self.app.category
    }
}
