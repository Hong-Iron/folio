//! What the Swift shell measures each tick. Raw-ish on purpose: every derived
//! feature is computed in Rust so recorded traces replay exactly.

use serde::{Deserialize, Serialize};

/// Normalized rectangle in Vision coordinates (origin bottom-left, 0..1).
#[derive(uniffi::Record, Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn center(&self) -> (f32, f32) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }
}

#[derive(uniffi::Record, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FaceSig {
    /// Radians, as reported by Vision.
    pub yaw: f32,
    pub pitch: f32,
    pub roll: f32,
    pub bbox: Rect,
    /// Eye openness (eye height / eye width, both eyes averaged).
    /// None when landmarks weren't run this tick.
    pub eye_open: Option<f32>,
}

#[derive(uniffi::Record, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Detection {
    pub conf: f32,
    pub bbox: Rect,
}

#[derive(uniffi::Record, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ObjectsSig {
    pub phone: Option<Detection>,
    pub book: Option<Detection>,
}

#[derive(uniffi::Record, Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

/// One tick of sensing. `None` on an optional sensor means "not run this
/// tick", which is different from "ran and found nothing".
#[derive(uniffi::Record, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SignalFrame {
    /// Monotonic milliseconds (continues across sleep).
    pub t_ms: u64,
    /// Unix epoch milliseconds, for display and day bucketing.
    pub wall_ms: i64,
    pub face: Option<FaceSig>,
    pub face_count: u32,
    /// Some only on ticks where identity verification ran.
    pub is_user: Option<bool>,
    /// Upper body seen; None when body detection wasn't run.
    pub body_present: Option<bool>,
    /// Hand centroids; None when hand pose wasn't run.
    pub hands: Option<Vec<Point>>,
    /// None when the object detector wasn't run.
    pub objects: Option<ObjectsSig>,
    pub input_idle_s: f32,
    pub app_bundle: Option<String>,
    pub app_name: Option<String>,
    pub window_title: Option<String>,
    pub url_host: Option<String>,
    pub screen_locked: bool,
    /// The camera delivered a usable frame this tick.
    pub camera_ok: bool,
    /// Mean luma of the frame, 0..1.
    pub brightness: f32,
}
