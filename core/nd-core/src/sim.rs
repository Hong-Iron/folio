//! Synthetic sensing traces for the eight study scenarios. They exercise the
//! engine's logic end to end; real recorded traces (label mode) are what tune
//! the thresholds, since only those carry real camera noise.

use crate::classify::Activity;
use crate::signal::{Detection, FaceSig, ObjectsSig, Point, Rect, SignalFrame};
use crate::zone::Calibration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scenario {
    Typing,
    Reading,
    Writing,
    Phone,
    StaringAway,
    LeftDesk,
    Dozing,
    Instagram,
    StillAtDesk,
}

impl Scenario {
    pub const ALL: [Scenario; 9] = [
        Scenario::Typing,
        Scenario::Reading,
        Scenario::Writing,
        Scenario::Phone,
        Scenario::StaringAway,
        Scenario::LeftDesk,
        Scenario::Dozing,
        Scenario::Instagram,
        Scenario::StillAtDesk,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Scenario::Typing => "typing",
            Scenario::Reading => "reading",
            Scenario::Writing => "writing",
            Scenario::Phone => "phone",
            Scenario::StaringAway => "staring-away",
            Scenario::LeftDesk => "left-desk",
            Scenario::Dozing => "dozing",
            Scenario::Instagram => "instagram",
            Scenario::StillAtDesk => "still-at-desk",
        }
    }

    /// What really happened at second `s` of the scenario.
    pub fn truth(self, s: u32) -> Activity {
        match self {
            Scenario::Typing => Activity::ScreenFocus,
            Scenario::Reading | Scenario::Writing => Activity::DeskFocus,
            Scenario::Phone => Activity::Phone,
            Scenario::StaringAway => Activity::Zoned,
            Scenario::LeftDesk => Activity::Away,
            Scenario::Dozing => Activity::Dozing,
            Scenario::Instagram => Activity::Social,
            Scenario::StillAtDesk if s < 180 => Activity::DeskFocus,
            Scenario::StillAtDesk => Activity::Unclear,
        }
    }
}

/// xorshift64* with Box-Muller; deterministic per seed.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    pub fn next_f32(&mut self) -> f32 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        ((self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 40) as f32) / (1u64 << 24) as f32
    }
    pub fn normal(&mut self, sd: f32) -> f32 {
        let u1 = self.next_f32().max(1e-7);
        let u2 = self.next_f32();
        (-2.0 * u1.ln()).sqrt() * (2.0 * std::f32::consts::PI * u2).cos() * sd
    }
}

fn face(yaw: f32, pitch: f32, eye: f32) -> Option<FaceSig> {
    Some(FaceSig { yaw, pitch, roll: 0.0, bbox: Rect { x: 0.35, y: 0.4, w: 0.3, h: 0.4 }, eye_open: Some(eye) })
}

fn det(conf: f32, x: f32, y: f32, w: f32, h: f32) -> Detection {
    Detection { conf, bbox: Rect { x, y, w, h } }
}

pub fn frames(sc: Scenario, t0_ms: u64, seconds: u32, cal: &Calibration, seed: u64) -> Vec<SignalFrame> {
    let mut rng = Rng::new(seed);
    let (sp, dp) = (cal.screen_pitch, cal.desk_pitch);
    let mut idle = 0.0f32;
    (0..seconds)
        .map(|s| {
            let t = t0_ms + s as u64 * 1000;
            let mut f = SignalFrame {
                t_ms: t,
                wall_ms: t as i64,
                face_count: 1,
                camera_ok: true,
                brightness: 0.4 + rng.normal(0.01),
                app_bundle: Some("com.microsoft.VSCode".into()),
                app_name: Some("Code".into()),
                ..Default::default()
            };
            let objects_due = s % 10 == 0;
            let desk_due = s % 2 == 0;
            idle += 1.0;
            match sc {
                Scenario::Typing => {
                    if rng.next_f32() < 0.6 {
                        idle = 0.0;
                    }
                    f.face = face(rng.normal(0.02), sp + rng.normal(0.02), 0.3 + rng.normal(0.02));
                    if objects_due {
                        f.objects = Some(ObjectsSig::default());
                    }
                }
                Scenario::Instagram => {
                    if rng.next_f32() < 0.5 {
                        idle = 0.0;
                    }
                    f.face = face(rng.normal(0.02), sp + rng.normal(0.02), 0.3 + rng.normal(0.02));
                    f.app_bundle = Some("com.apple.Safari".into());
                    f.app_name = Some("Safari".into());
                    f.url_host = Some("www.instagram.com".into());
                    f.window_title = Some("Instagram".into());
                }
                Scenario::Reading => {
                    let line = ((s % 15) as f32 / 15.0) * 0.06 - 0.03;
                    f.face = face(rng.normal(0.03), dp + line + rng.normal(0.02), cal.desk_eye_open + rng.normal(0.02));
                    f.app_bundle = Some("com.apple.Preview".into());
                    f.app_name = Some("Preview".into());
                    idle += 30.0;
                    if desk_due {
                        let book = if rng.next_f32() < 0.5 { Some(det(0.6, 0.3, 0.0, 0.4, 0.15)) } else { None };
                        f.objects = Some(ObjectsSig { phone: None, book });
                        f.hands = Some(vec![Point { x: 0.6 + rng.normal(0.005), y: 0.15 }]);
                    }
                }
                Scenario::Writing => {
                    idle += 30.0;
                    if rng.next_f32() < 0.3 {
                        f.face = None;
                        f.face_count = 0;
                        f.body_present = Some(true);
                    } else {
                        f.face = face(rng.normal(0.03), dp + 0.05 + rng.normal(0.02), cal.desk_eye_open * 0.9);
                    }
                    if desk_due {
                        let x = 0.5 + 0.08 * ((s / 2) % 2) as f32;
                        f.hands = Some(vec![Point { x, y: 0.1 }]);
                        f.objects = Some(ObjectsSig::default());
                    }
                }
                Scenario::Phone => {
                    idle += 30.0;
                    f.face = face(rng.normal(0.03), sp + (dp - sp) * 0.8 + rng.normal(0.02), cal.desk_eye_open + rng.normal(0.02));
                    if desk_due {
                        f.objects = Some(ObjectsSig { phone: Some(det(0.75, 0.4, 0.25, 0.15, 0.2)), book: None });
                        f.hands = Some(vec![Point { x: 0.47, y: 0.3 + rng.normal(0.01) }]);
                    }
                }
                Scenario::StaringAway => {
                    idle += 60.0;
                    f.face = face(0.8 + rng.normal(0.05), sp + rng.normal(0.03), 0.3 + rng.normal(0.02));
                    if objects_due {
                        f.objects = Some(ObjectsSig::default());
                    }
                }
                Scenario::LeftDesk => {
                    idle += 60.0;
                    f.face = None;
                    f.face_count = 0;
                    f.body_present = Some(false);
                    if desk_due {
                        f.objects = Some(ObjectsSig::default());
                        f.hands = Some(vec![]);
                    }
                }
                Scenario::Dozing => {
                    idle += 60.0;
                    f.face = face(rng.normal(0.004), dp + 0.1 + rng.normal(0.004), 0.03);
                    if desk_due {
                        f.objects = Some(ObjectsSig::default());
                        f.hands = Some(vec![]);
                    }
                }
                Scenario::StillAtDesk => {
                    idle += 60.0;
                    f.face = face(rng.normal(0.003), dp + rng.normal(0.003), cal.desk_eye_open);
                    if desk_due {
                        f.objects = Some(ObjectsSig::default());
                        f.hands = Some(vec![]);
                    }
                }
            }
            f.input_idle_s = idle;
            f
        })
        .collect()
}

/// A scripted session: frames plus per-second truth labels.
pub fn session(script: &[(Scenario, u32)], cal: &Calibration, seed: u64) -> (Vec<SignalFrame>, Vec<(u64, u64, Activity)>) {
    let mut frames_out = Vec::new();
    let mut labels = Vec::new();
    let mut t = 0u64;
    for (i, (sc, secs)) in script.iter().enumerate() {
        frames_out.extend(frames(*sc, t, *secs, cal, seed.wrapping_add(i as u64 * 7919)));
        for s in 0..*secs {
            let a = sc.truth(s);
            let ts = t + s as u64 * 1000;
            match labels.last_mut() {
                Some((_, end, last)) if *last == a && *end == ts => *end = ts + 1000,
                _ => labels.push((ts, ts + 1000, a)),
            }
        }
        t += *secs as u64 * 1000;
    }
    (frames_out, labels)
}
