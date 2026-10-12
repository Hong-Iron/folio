//! Synthetic history for previews and screenshots. Development only: the
//! app calls it only when pointed at a scratch database.

use crate::engine::EngineCore;
use crate::error::Result;
use crate::sim::{self, Rng, Scenario};
use crate::time::DAY_MS;

/// Fills `days` days ending today with one to three sessions a day, built by
/// running simulated sensing through the real engine. Returns the id of the
/// last session.
pub fn seed(core: &mut EngineCore, days: u32, now_wall_ms: i64, tz_offset_min: i32) -> Result<Option<i64>> {
    let cal = core.cal.clone();
    let tz_ms = tz_offset_min as i64 * 60_000;
    let today = (now_wall_ms + tz_ms).div_euclid(DAY_MS);
    let mut rng = Rng::new(20_261_012);
    let mut last = None;
    for back in (0..days as i64).rev() {
        let day = today - back;
        let sessions = 1 + (rng.next_f32() * 2.6) as usize;
        let mut starts = [9.0f32, 14.0, 19.5];
        for s in starts.iter_mut() {
            *s += rng.next_f32() * 1.5 - 0.25;
        }
        for (i, start_h) in starts.iter().take(sessions).enumerate() {
            let start_wall = day * DAY_MS - tz_ms + (*start_h * 3_600_000.0) as i64;
            if start_wall + 3 * 3_600_000 > now_wall_ms {
                continue;
            }
            let script = script_for(&mut rng, i);
            let (frames, _) = sim::session(&script, &cal, (day as u64) * 31 + i as u64);
            let t0 = start_wall as u64;
            core.start_session(Some(label_for(&mut rng)), Some(90), t0, start_wall, tz_offset_min)?;
            for mut f in frames {
                f.t_ms += t0;
                f.wall_ms = f.t_ms as i64;
                core.ingest(&f);
            }
            let end = t0 + script.iter().map(|(_, s)| *s as u64 * 1000).sum::<u64>();
            if let Some(s) = core.end_session(end)? {
                last = Some(s.session_id);
            }
        }
    }
    Ok(last)
}

fn script_for(rng: &mut Rng, slot: usize) -> Vec<(Scenario, u32)> {
    let mut v = vec![(Scenario::Typing, 300 + (rng.next_f32() * 900.0) as u32)];
    let blocks = 3 + (rng.next_f32() * 4.0) as usize;
    for _ in 0..blocks {
        let r = rng.next_f32();
        let focus = if r < 0.5 { Scenario::Reading } else if r < 0.75 { Scenario::Writing } else { Scenario::Typing };
        v.push((focus, 600 + (rng.next_f32() * 1500.0) as u32));
        let d = rng.next_f32();
        let drift = match d {
            x if x < 0.30 => Some((Scenario::Phone, 60 + (rng.next_f32() * 300.0) as u32)),
            x if x < 0.55 => Some((Scenario::Instagram, 40 + (rng.next_f32() * 240.0) as u32)),
            x if x < 0.65 => Some((Scenario::StaringAway, 60 + (rng.next_f32() * 120.0) as u32)),
            x if x < 0.75 => Some((Scenario::LeftDesk, 120 + (rng.next_f32() * 300.0) as u32)),
            x if x < 0.80 && slot == 2 => Some((Scenario::StillAtDesk, 400)),
            _ => None,
        };
        v.extend(drift);
    }
    v.push((Scenario::Typing, 200 + (rng.next_f32() * 400.0) as u32));
    v
}

fn label_for(rng: &mut Rng) -> String {
    const LABELS: [&str; 5] = ["Organic chemistry, ch. 4", "Linear algebra problem set", "History essay", "Physics reading", "Vocabulary review"];
    LABELS[(rng.next_f32() * LABELS.len() as f32) as usize % LABELS.len()].to_string()
}
