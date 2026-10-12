//! Debounces raw labels into committed states. A new state has to hold for
//! its grace period, and when it commits, its start is backdated to when it
//! actually began, so grace periods never inflate focus time.

use crate::classify::Activity;
use crate::config::{ms, Config};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Transition {
    pub at_ms: u64,
    pub to: Activity,
}

#[derive(Clone, Copy, Debug)]
struct Candidate {
    label: Activity,
    since: u64,
    misses: u32,
}

#[derive(Debug, Default)]
pub struct Smoother {
    committed: Option<(Activity, u64)>,
    cand: Option<Candidate>,
}

pub fn grace_ms(from: Activity, to: Activity, cfg: &Config) -> u64 {
    let s = match to {
        a if a.is_focus() && from.is_focus() => cfg.grace_switch_s,
        a if a.is_focus() => cfg.grace_focus_s,
        Activity::Social => cfg.grace_social_s,
        Activity::Phone => cfg.grace_phone_s,
        Activity::Zoned => cfg.grace_zoned_s,
        Activity::Dozing => cfg.grace_dozing_s,
        Activity::Away => cfg.grace_away_s,
        Activity::Unclear => cfg.grace_unclear_s,
        Activity::Unmonitored => cfg.grace_unmonitored_s,
        _ => 0.0,
    };
    ms(s)
}

impl Smoother {
    pub fn reset(&mut self) {
        self.committed = None;
        self.cand = None;
    }

    pub fn current(&self) -> Option<Activity> {
        self.committed.map(|(a, _)| a)
    }

    /// Force a state without grace (session start, break end).
    pub fn force(&mut self, t: u64, a: Activity) {
        self.committed = Some((a, t));
        self.cand = None;
    }

    pub fn push(&mut self, t: u64, raw: Activity, cfg: &Config) -> Option<Transition> {
        self.push_with_onset(t, raw, t, cfg)
    }

    /// `onset` is when the evidence for `raw` began (e.g. when the eyes
    /// closed), so the committed state is backdated to the real start.
    pub fn push_with_onset(&mut self, t: u64, raw: Activity, onset: u64, cfg: &Config) -> Option<Transition> {
        let onset = onset.min(t);
        let Some((cur, cur_since)) = self.committed else {
            self.committed = Some((raw, t));
            return Some(Transition { at_ms: t, to: raw });
        };

        if raw == cur {
            if let Some(c) = &mut self.cand {
                c.misses += 1;
                if c.misses > cfg.dropout_ticks {
                    self.cand = None;
                }
            }
            return None;
        }

        match &mut self.cand {
            Some(c) if c.label == raw => c.misses = 0,
            Some(c) => {
                c.misses += 1;
                if c.misses > cfg.dropout_ticks {
                    *c = Candidate { label: raw, since: onset, misses: 0 };
                }
            }
            None => self.cand = Some(Candidate { label: raw, since: onset, misses: 0 }),
        }

        let c = self.cand?;
        if c.label == raw && t.saturating_sub(c.since) >= grace_ms(cur, c.label, cfg) {
            let since = c.since.max(cur_since);
            self.committed = Some((c.label, since));
            self.cand = None;
            return Some(Transition { at_ms: since, to: c.label });
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Activity::*;

    fn run(labels: &[Activity]) -> Vec<Transition> {
        let cfg = Config::default();
        let mut s = Smoother::default();
        labels.iter().enumerate().filter_map(|(i, a)| s.push(i as u64 * 1000, *a, &cfg)).collect()
    }

    #[test]
    fn short_blips_are_absorbed() {
        let mut seq = vec![ScreenFocus; 10];
        seq.extend([Social; 10]);
        seq.extend([ScreenFocus; 10]);
        let tr = run(&seq);
        assert_eq!(tr, vec![Transition { at_ms: 0, to: ScreenFocus }]);
    }

    #[test]
    fn long_drift_commits_backdated() {
        let mut seq = vec![ScreenFocus; 10];
        seq.extend([Social; 30]);
        let tr = run(&seq);
        assert_eq!(tr.last(), Some(&Transition { at_ms: 10_000, to: Social }));
    }

    #[test]
    fn noisy_drift_still_commits() {
        let mut seq = vec![ScreenFocus; 5];
        for i in 0..40 {
            seq.push(if i % 7 == 3 { ScreenFocus } else { Social });
        }
        let tr = run(&seq);
        assert_eq!(tr.last().map(|t| t.to), Some(Social));
        assert_eq!(tr.last().map(|t| t.at_ms), Some(5_000));
    }

    #[test]
    fn onset_backdates_further() {
        let cfg = Config::default();
        let mut s = Smoother::default();
        for i in 0..10 {
            s.push(i * 1000, ScreenFocus, &cfg);
        }
        // Eyes closed at 10 s; dozing is only recognised 8 s later.
        let mut tr = None;
        for i in 18..30 {
            tr = tr.or(s.push_with_onset(i * 1000, Dozing, 10_000, &cfg));
        }
        assert_eq!(tr, Some(Transition { at_ms: 10_000, to: Dozing }));
    }

    #[test]
    fn focus_switch_is_quick() {
        let mut seq = vec![ScreenFocus; 5];
        seq.extend([DeskFocus; 4]);
        let tr = run(&seq);
        assert_eq!(tr.last(), Some(&Transition { at_ms: 5_000, to: DeskFocus }));
    }
}
