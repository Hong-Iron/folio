//! One raw label per tick. Pure: same features in, same label out.

use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::features::{Features, Identity};
use crate::rules::Category;
use crate::zone::Zone;

#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Activity {
    ScreenFocus,
    DeskFocus,
    Social,
    Phone,
    Dozing,
    Zoned,
    Unclear,
    Away,
    Unmonitored,
    Break,
}

impl Activity {
    pub fn is_focus(self) -> bool {
        matches!(self, Activity::ScreenFocus | Activity::DeskFocus)
    }

    pub fn is_drift(self) -> bool {
        matches!(self, Activity::Social | Activity::Phone | Activity::Dozing | Activity::Zoned)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Activity::ScreenFocus => "screen",
            Activity::DeskFocus => "desk",
            Activity::Social => "social",
            Activity::Phone => "phone",
            Activity::Dozing => "dozing",
            Activity::Zoned => "zoned",
            Activity::Unclear => "unclear",
            Activity::Away => "away",
            Activity::Unmonitored => "unmonitored",
            Activity::Break => "break",
        }
    }

    pub fn parse(s: &str) -> Option<Activity> {
        Some(match s {
            "screen" => Activity::ScreenFocus,
            "desk" => Activity::DeskFocus,
            "social" => Activity::Social,
            "phone" => Activity::Phone,
            "dozing" => Activity::Dozing,
            "zoned" => Activity::Zoned,
            "unclear" => Activity::Unclear,
            "away" => Activity::Away,
            "unmonitored" => Activity::Unmonitored,
            "break" => Activity::Break,
            _ => return None,
        })
    }

    pub const ALL: [Activity; 10] = [
        Activity::ScreenFocus,
        Activity::DeskFocus,
        Activity::Social,
        Activity::Phone,
        Activity::Dozing,
        Activity::Zoned,
        Activity::Unclear,
        Activity::Away,
        Activity::Unmonitored,
        Activity::Break,
    ];
}

fn screen_rule(f: &Features, cfg: &Config) -> Activity {
    let limit = if f.category() == Category::Focus { cfg.screen_idle_focus_s } else { cfg.screen_idle_neutral_s };
    if f.input_idle_s <= limit {
        Activity::ScreenFocus
    } else {
        Activity::Unclear
    }
}

/// When the evidence for `a` began, if earlier than now.
pub fn onset(a: Activity, f: &Features) -> u64 {
    match a {
        Activity::Dozing => f.eyes_closed_since.unwrap_or(f.t_ms),
        // You left when your face was last seen, not when we gave up waiting.
        Activity::Away if f.zone == Zone::Absent => f.face_last_seen.map_or(f.t_ms, |s| s + 1000),
        _ => f.t_ms,
    }
}

/// First match wins. The order encodes the safety rule: anything that looks
/// like distraction is checked before anything that would count as focus.
pub fn classify(f: &Features, cfg: &Config) -> Activity {
    if !f.sensing_ok {
        return Activity::Unmonitored;
    }
    if f.zone == Zone::Absent || (f.identity == Identity::Other && f.face_visible) {
        return Activity::Away;
    }
    if f.phone_held {
        return Activity::Phone;
    }
    // A locked screen (often just the display sleeping while you read)
    // rules out computer use, but not studying at the desk.
    let using_computer = !f.locked && f.input_idle_s <= cfg.input_active_s;
    if !f.locked && f.category() == Category::Drift && (f.zone == Zone::Screen || using_computer) {
        return Activity::Social;
    }
    if f.face_visible && f.eyes_closed_s >= cfg.dozing_s && !f.writing {
        return Activity::Dozing;
    }
    match f.zone {
        Zone::Screen if f.locked => Activity::Zoned,
        Zone::Screen => screen_rule(f, cfg),
        // Typing while looking away from the calibrated screen usually means
        // a second monitor, so judge it by the app in front.
        Zone::Elsewhere if using_computer => screen_rule(f, cfg),
        Zone::Elsewhere => Activity::Zoned,
        Zone::Desk if f.desk_no_evidence_s < cfg.desk_no_evidence_s => Activity::DeskFocus,
        Zone::Desk => Activity::Unclear,
        Zone::Absent => Activity::Away,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::Categorized;

    fn base() -> Features {
        Features {
            t_ms: 0,
            zone: Zone::Screen,
            face_visible: true,
            identity: Identity::User,
            head_motion: 0.02,
            eyes_closed_s: 0.0,
            eyes_closed_since: None,
            face_last_seen: None,
            hand_motion: 0.0,
            writing: false,
            phone_held: false,
            book_seen: false,
            input_idle_s: 2.0,
            app: Categorized { category: Category::Neutral, detail: None, host: None, matched: false },
            locked: false,
            sensing_ok: true,
            too_dark: false,
            desk_no_evidence_s: 0.0,
        }
    }

    fn with(f: impl FnOnce(&mut Features)) -> Activity {
        let mut x = base();
        f(&mut x);
        classify(&x, &Config::default())
    }

    #[test]
    fn decision_table() {
        let cfg = Config::default();
        assert_eq!(with(|_| {}), Activity::ScreenFocus);
        assert_eq!(with(|f| f.app.category = Category::Drift), Activity::Social);
        assert_eq!(with(|f| f.input_idle_s = cfg.screen_idle_neutral_s + 1.0), Activity::Unclear);
        assert_eq!(
            with(|f| {
                f.input_idle_s = cfg.screen_idle_neutral_s + 1.0;
                f.app.category = Category::Focus;
            }),
            Activity::ScreenFocus
        );
        assert_eq!(with(|f| f.zone = Zone::Desk), Activity::DeskFocus);
        assert_eq!(
            with(|f| {
                f.zone = Zone::Desk;
                f.desk_no_evidence_s = cfg.desk_no_evidence_s + 1.0;
            }),
            Activity::Unclear
        );
        assert_eq!(
            with(|f| {
                f.zone = Zone::Desk;
                f.phone_held = true;
            }),
            Activity::Phone
        );
        assert_eq!(
            with(|f| {
                f.zone = Zone::Desk;
                f.eyes_closed_s = 9.0;
            }),
            Activity::Dozing
        );
        assert_eq!(
            with(|f| {
                f.zone = Zone::Elsewhere;
                f.input_idle_s = 60.0;
            }),
            Activity::Zoned
        );
        assert_eq!(with(|f| f.zone = Zone::Elsewhere), Activity::ScreenFocus);
        assert_eq!(with(|f| f.zone = Zone::Absent), Activity::Away);
        assert_eq!(with(|f| f.identity = Identity::Other), Activity::Away);
        assert_eq!(with(|f| f.sensing_ok = false), Activity::Unmonitored);
        assert_eq!(with(|f| f.locked = true), Activity::Zoned);
        assert_eq!(
            with(|f| {
                f.locked = true;
                f.zone = Zone::Desk;
            }),
            Activity::DeskFocus
        );
        assert_eq!(
            with(|f| {
                f.locked = true;
                f.zone = Zone::Absent;
            }),
            Activity::Away
        );
        assert_eq!(
            with(|f| {
                f.locked = true;
                f.app.category = Category::Drift;
            }),
            Activity::Zoned
        );
    }

    #[test]
    fn distraction_beats_focus_evidence() {
        // Scrolling a drift site while glancing down still counts as social.
        assert_eq!(
            with(|f| {
                f.zone = Zone::Desk;
                f.app.category = Category::Drift;
                f.input_idle_s = 1.0;
                f.book_seen = true;
            }),
            Activity::Social
        );
    }

    #[test]
    fn activity_names_round_trip() {
        for a in Activity::ALL {
            assert_eq!(Activity::parse(a.as_str()), Some(a));
        }
    }
}
