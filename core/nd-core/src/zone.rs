//! Where the user is looking, relative to two poses captured at onboarding.

use serde::{Deserialize, Serialize};

use crate::config::Config;

#[derive(uniffi::Record, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Calibration {
    pub screen_yaw: f32,
    pub screen_pitch: f32,
    pub desk_pitch: f32,
    pub screen_eye_open: f32,
    pub desk_eye_open: f32,
    /// Whether the face stayed detectable while looking down at the desk.
    pub desk_face_visible: bool,
    pub calibrated: bool,
}

impl Default for Calibration {
    fn default() -> Self {
        Self {
            screen_yaw: 0.0,
            screen_pitch: 0.0,
            desk_pitch: 0.4,
            screen_eye_open: 0.3,
            desk_eye_open: 0.2,
            desk_face_visible: true,
            calibrated: false,
        }
    }
}

#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Zone {
    Screen,
    Desk,
    Elsewhere,
    Absent,
}

/// Maps a head pose to a zone. Vision's pitch sign is taken from the
/// calibration itself, so it doesn't matter which way "down" is.
pub fn zone_for_pose(cal: &Calibration, cfg: &Config, yaw: f32, pitch: f32) -> Zone {
    let raw_span = cal.desk_pitch - cal.screen_pitch;
    let dir = if raw_span >= 0.0 { 1.0 } else { -1.0 };
    let span = raw_span.abs().max(cfg.min_pitch_span);
    let down = (pitch - cal.screen_pitch) * dir;
    let yaw_off = (yaw - cal.screen_yaw).abs();

    if down >= span * cfg.desk_frac {
        if yaw_off <= cfg.desk_yaw_tol {
            Zone::Desk
        } else {
            Zone::Elsewhere
        }
    } else if down >= -span * cfg.up_frac && yaw_off <= cfg.screen_yaw_tol {
        Zone::Screen
    } else {
        Zone::Elsewhere
    }
}

/// How far toward the desk the head is, 0 = screen pose, 1 = desk pose.
pub fn desk_progress(cal: &Calibration, cfg: &Config, pitch: f32) -> f32 {
    let raw_span = cal.desk_pitch - cal.screen_pitch;
    let dir = if raw_span >= 0.0 { 1.0 } else { -1.0 };
    let span = raw_span.abs().max(cfg.min_pitch_span);
    (pitch - cal.screen_pitch) * dir / span
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cal(screen: f32, desk: f32) -> Calibration {
        Calibration { screen_pitch: screen, desk_pitch: desk, calibrated: true, ..Default::default() }
    }

    #[test]
    fn zones_follow_calibrated_direction() {
        let cfg = Config::default();
        for (s, d) in [(0.0, 0.4), (0.0, -0.4)] {
            let c = cal(s, d);
            assert_eq!(zone_for_pose(&c, &cfg, 0.0, s), Zone::Screen);
            assert_eq!(zone_for_pose(&c, &cfg, 0.0, d), Zone::Desk);
            assert_eq!(zone_for_pose(&c, &cfg, 0.0, d * 0.3), Zone::Screen);
            assert_eq!(zone_for_pose(&c, &cfg, 0.9, s), Zone::Elsewhere);
            assert_eq!(zone_for_pose(&c, &cfg, 0.9, d), Zone::Elsewhere);
            assert_eq!(zone_for_pose(&c, &cfg, 0.0, -d), Zone::Elsewhere);
        }
    }
}
