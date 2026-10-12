//! Replays a trace through the engine and scores the recorded timeline
//! against ground-truth labels. Shared by tests and `nd-replay`.

use std::collections::BTreeMap;

use crate::classify::Activity;
use crate::config::Config;
use crate::engine::EngineCore;
use crate::signal::SignalFrame;
use crate::zone::Calibration;

#[derive(Clone, Debug, Default)]
pub struct EvalResult {
    pub seconds: u64,
    pub agree: u64,
    pub confident_seconds: u64,
    pub confident_agree: u64,
    pub focus_true_s: u64,
    pub focus_pred_s: u64,
    pub distraction_true_s: u64,
    pub distraction_as_focus_s: u64,
    pub unclear_s: u64,
    pub nudges: u32,
    pub false_nudges: u32,
    pub confusion: BTreeMap<(&'static str, &'static str), u64>,
    pub timeline: Vec<(u64, u64, Activity, Option<String>)>,
}

impl EvalResult {
    pub fn agreement(&self) -> f64 {
        ratio(self.agree, self.seconds)
    }
    pub fn confident_agreement(&self) -> f64 {
        ratio(self.confident_agree, self.confident_seconds)
    }
    pub fn focus_error(&self) -> f64 {
        if self.focus_true_s == 0 {
            return if self.focus_pred_s == 0 { 0.0 } else { 1.0 };
        }
        (self.focus_pred_s as f64 - self.focus_true_s as f64).abs() / self.focus_true_s as f64
    }
    pub fn distraction_as_focus(&self) -> f64 {
        ratio(self.distraction_as_focus_s, self.distraction_true_s)
    }
    pub fn unclear_share(&self) -> f64 {
        ratio(self.unclear_s, self.seconds)
    }
}

fn ratio(a: u64, b: u64) -> f64 {
    if b == 0 {
        0.0
    } else {
        a as f64 / b as f64
    }
}

/// Targets from the plan (§7). Unclear share is checked separately because
/// some scenarios are meant to end up Unclear.
pub struct Targets {
    pub max_focus_error: f64,
    pub max_distraction_as_focus: f64,
    pub min_confident_agreement: f64,
}

impl Default for Targets {
    fn default() -> Self {
        Self { max_focus_error: 0.05, max_distraction_as_focus: 0.01, min_confident_agreement: 0.95 }
    }
}

impl Targets {
    pub fn failures(&self, r: &EvalResult) -> Vec<String> {
        let mut out = Vec::new();
        if r.focus_error() > self.max_focus_error {
            out.push(format!("focus-time error {:.1}% > {:.1}%", r.focus_error() * 100.0, self.max_focus_error * 100.0));
        }
        if r.distraction_as_focus() > self.max_distraction_as_focus {
            out.push(format!(
                "distraction counted as focus {:.1}% > {:.1}%",
                r.distraction_as_focus() * 100.0,
                self.max_distraction_as_focus * 100.0
            ));
        }
        if r.confident_agreement() < self.min_confident_agreement {
            out.push(format!(
                "confident agreement {:.1}% < {:.1}%",
                r.confident_agreement() * 100.0,
                self.min_confident_agreement * 100.0
            ));
        }
        if r.false_nudges > 0 {
            out.push(format!("{} nudge(s) during real focus", r.false_nudges));
        }
        out
    }
}

fn label_at(labels: &[(u64, u64, Activity)], t: u64) -> Option<Activity> {
    labels.iter().find(|(a, b, _)| *a <= t && t < *b).map(|l| l.2)
}

pub fn evaluate(frames: &[SignalFrame], labels: &[(u64, u64, Activity)], cfg: &Config, cal: &Calibration) -> EvalResult {
    let mut core = EngineCore::new(None).expect("in-memory engine");
    core.cfg = cfg.clone();
    core.cal = cal.clone();
    let mut r = EvalResult::default();
    let Some(first) = frames.first() else { return r };
    let t0 = first.t_ms;
    core.start_session(None, None, t0, t0 as i64, 0).expect("session");
    for f in frames {
        let u = core.ingest(f);
        if u.nudge.is_some() {
            r.nudges += 1;
            if label_at(labels, f.t_ms).is_some_and(|a| a.is_focus()) {
                r.false_nudges += 1;
            }
        }
    }
    let end = frames.last().map_or(t0, |f| f.t_ms + 1000);
    let summary = core.end_session(end).expect("end").expect("summary");
    // Wall time equals monotonic time here (session started at wall = t0).
    r.timeline = summary
        .segments
        .iter()
        .map(|s| (s.start_wall as u64, s.end_wall as u64, s.activity, s.detail.clone()))
        .collect();

    let mut t = t0;
    while t < end {
        let pred = r.timeline.iter().find(|(a, b, _, _)| *a <= t && t < *b).map(|x| x.2);
        if let (Some(truth), Some(pred)) = (label_at(labels, t), pred) {
            r.seconds += 1;
            if truth == pred {
                r.agree += 1;
            }
            if pred != Activity::Unclear && truth != Activity::Unclear {
                r.confident_seconds += 1;
                if truth == pred {
                    r.confident_agree += 1;
                }
            }
            if truth.is_focus() {
                r.focus_true_s += 1;
            }
            if pred.is_focus() {
                r.focus_pred_s += 1;
            }
            if matches!(truth, Activity::Phone | Activity::Social) {
                r.distraction_true_s += 1;
                if pred.is_focus() {
                    r.distraction_as_focus_s += 1;
                }
            }
            if pred == Activity::Unclear {
                r.unclear_s += 1;
            }
            *r.confusion.entry((truth.as_str(), pred.as_str())).or_default() += 1;
        }
        t += 1000;
    }
    r
}

/// Labels CSV: `start_ms,end_ms,label` with labels like "screen", "desk",
/// "phone", "social", "idle" (zoned out), "away".
pub fn parse_labels(csv: &str) -> Result<Vec<(u64, u64, Activity)>, String> {
    let mut out = Vec::new();
    for (i, line) in csv.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with("start") {
            continue;
        }
        let parts: Vec<&str> = line.split(',').map(str::trim).collect();
        if parts.len() < 3 {
            return Err(format!("line {}: expected start_ms,end_ms,label", i + 1));
        }
        let a = parts[0].parse::<u64>().map_err(|e| format!("line {}: {e}", i + 1))?;
        let b = parts[1].parse::<u64>().map_err(|e| format!("line {}: {e}", i + 1))?;
        let label = match parts[2] {
            "idle" => Activity::Zoned,
            other => Activity::parse(other).ok_or_else(|| format!("line {}: unknown label {other}", i + 1))?,
        };
        if b > a {
            out.push((a, b, label));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::{self, Scenario};

    #[test]
    fn every_scenario_meets_targets() {
        let cal = Calibration { calibrated: true, ..Default::default() };
        let cfg = Config::default();
        let targets = Targets::default();
        for sc in Scenario::ALL {
            // Lead in with typing so the scenario starts from a focus state.
            let (frames, labels) = sim::session(&[(Scenario::Typing, 60), (sc, 300)], &cal, 42);
            let r = evaluate(&frames, &labels, &cfg, &cal);
            let fails = targets.failures(&r);
            assert!(fails.is_empty(), "{}: {fails:?}\n{:?}", sc.name(), r.confusion);
        }
    }

    #[test]
    fn mixed_session_meets_targets_across_seeds() {
        let cal = Calibration { calibrated: true, ..Default::default() };
        let script = [
            (Scenario::Typing, 300),
            (Scenario::Reading, 420),
            (Scenario::Phone, 120),
            (Scenario::Writing, 300),
            (Scenario::Instagram, 90),
            (Scenario::Typing, 240),
            (Scenario::LeftDesk, 200),
            (Scenario::Reading, 300),
            (Scenario::StaringAway, 120),
            (Scenario::Typing, 180),
        ];
        for seed in 1..=10 {
            let (frames, labels) = sim::session(&script, &cal, seed);
            let r = evaluate(&frames, &labels, &Config::default(), &cal);
            let fails = Targets::default().failures(&r);
            assert!(fails.is_empty(), "seed {seed}: {fails:?}\n{:?}", r.confusion);
        }
    }

    #[test]
    fn labels_parse() {
        let l = parse_labels("start_ms,end_ms,label\n0,1000,screen\n1000,5000,idle\n").unwrap();
        assert_eq!(l, vec![(0, 1000, Activity::ScreenFocus), (1000, 5000, Activity::Zoned)]);
        assert!(parse_labels("0,1,nope").is_err());
    }
}
