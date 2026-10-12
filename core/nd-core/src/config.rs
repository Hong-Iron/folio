//! Every tunable number in one place. Defaults are starting points; the
//! replay suite is what decides whether a change to them is an improvement.

use serde::{Deserialize, Serialize};

#[derive(uniffi::Record, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    // Gaze zones (radians / fractions of the calibrated screen→desk pitch span).
    pub screen_yaw_tol: f32,
    pub desk_yaw_tol: f32,
    pub desk_frac: f32,
    pub up_frac: f32,
    pub min_pitch_span: f32,

    // Evidence.
    pub phone_conf: f32,
    pub book_conf: f32,
    pub phone_hold_s: f32,
    pub book_hold_s: f32,
    pub hand_motion_min: f32,
    pub read_motion_min: f32,
    pub eyes_closed_ratio: f32,
    pub dozing_s: f32,

    // Timing.
    pub screen_idle_neutral_s: f32,
    pub screen_idle_focus_s: f32,
    pub input_active_s: f32,
    pub desk_no_evidence_s: f32,
    pub face_lost_grace_s: f32,

    // How long a new state must hold before it's committed (seconds).
    pub grace_focus_s: f32,
    pub grace_switch_s: f32,
    pub grace_social_s: f32,
    pub grace_phone_s: f32,
    pub grace_zoned_s: f32,
    pub grace_dozing_s: f32,
    pub grace_away_s: f32,
    pub grace_unclear_s: f32,
    pub grace_unmonitored_s: f32,
    /// Ticks of disagreement a pending state tolerates before it's dropped.
    pub dropout_ticks: u32,

    // Session behaviour.
    pub nudges: bool,
    pub nudge_after_s: f32,
    pub nudge_cooldown_s: f32,
    pub auto_pause_away_s: f32,
    pub verify_every_s: f32,
    pub reverify_after_loss_s: f32,
    pub ask_category_after_s: f32,
    pub min_brightness: f32,
    pub face_match_threshold: f32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            screen_yaw_tol: 0.35,
            desk_yaw_tol: 0.6,
            desk_frac: 0.45,
            up_frac: 0.8,
            min_pitch_span: 0.12,

            phone_conf: 0.45,
            book_conf: 0.35,
            phone_hold_s: 4.0,
            book_hold_s: 20.0,
            hand_motion_min: 0.02,
            read_motion_min: 0.012,
            eyes_closed_ratio: 0.5,
            dozing_s: 8.0,

            screen_idle_neutral_s: 300.0,
            screen_idle_focus_s: 600.0,
            input_active_s: 10.0,
            desk_no_evidence_s: 180.0,
            face_lost_grace_s: 3.0,

            grace_focus_s: 5.0,
            grace_switch_s: 3.0,
            grace_social_s: 15.0,
            grace_phone_s: 20.0,
            grace_zoned_s: 45.0,
            grace_dozing_s: 4.0,
            grace_away_s: 10.0,
            grace_unclear_s: 5.0,
            grace_unmonitored_s: 5.0,
            dropout_ticks: 2,

            nudges: true,
            nudge_after_s: 30.0,
            nudge_cooldown_s: 300.0,
            auto_pause_away_s: 300.0,
            verify_every_s: 120.0,
            reverify_after_loss_s: 10.0,
            ask_category_after_s: 300.0,
            min_brightness: 0.06,
            face_match_threshold: 0.36,
        }
    }
}

pub(crate) fn ms(s: f32) -> u64 {
    if s.is_finite() && s > 0.0 {
        (s * 1000.0) as u64
    } else {
        0
    }
}
