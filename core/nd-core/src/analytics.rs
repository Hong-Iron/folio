//! Reports computed straight from segments, so an edit or an Unclear review
//! is reflected everywhere immediately. Volumes are tiny (hundreds of
//! segments a day), so there's no rollup table to keep in sync.

use std::collections::HashMap;

use crate::classify::Activity;
use crate::store::{SegmentRow, SessionRow};
use crate::time::{format_duration, local_ms, part_of_day, weekday, DAY_MS, WEEKDAYS};

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct RibbonPiece {
    /// Minutes since local midnight.
    pub start_min: u32,
    pub end_min: u32,
    pub activity: Activity,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq, Default)]
pub struct Totals {
    pub focus_ms: i64,
    pub screen_ms: i64,
    pub desk_ms: i64,
    pub drift_ms: i64,
    pub away_ms: i64,
    pub unclear_ms: i64,
    pub unmonitored_ms: i64,
    pub break_ms: i64,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct DayReport {
    pub date: String,
    /// 0 = Monday.
    pub weekday: u32,
    pub totals: Totals,
    pub longest_ms: i64,
    pub drifts: u32,
    pub pieces: Vec<RibbonPiece>,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct Cause {
    pub label: String,
    pub ms: i64,
    pub count: u32,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct RangeReport {
    pub from_date: String,
    pub days: Vec<DayReport>,
    /// Focus milliseconds per local hour of day (24 entries).
    pub hours: Vec<i64>,
    pub causes: Vec<Cause>,
    pub totals: Totals,
    pub prev_focus_ms: i64,
    pub longest_ms: i64,
    pub insights: Vec<String>,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct SegmentView {
    pub id: i64,
    pub activity: Activity,
    pub start_wall: i64,
    pub end_wall: i64,
    pub detail: Option<String>,
    pub claimed: bool,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct UnclearSpan {
    pub segment_id: i64,
    pub start_wall: i64,
    pub end_wall: i64,
    pub at_desk: bool,
    pub claimed: bool,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct SessionSummary {
    pub session_id: i64,
    pub label: Option<String>,
    pub started_wall: i64,
    pub ended_wall: i64,
    pub tz_offset_min: i32,
    pub totals: Totals,
    pub present_ms: i64,
    pub longest_ms: i64,
    pub causes: Vec<Cause>,
    pub unclear: Vec<UnclearSpan>,
    pub segments: Vec<SegmentView>,
    pub headline: String,
    pub sentence: String,
}

/// Unclear time the user claimed counts as focus at the screen or desk.
pub fn effective(s: &SegmentRow) -> Activity {
    match (s.activity, s.claimed) {
        (Activity::Unclear, true) if s.detail.as_deref() == Some("desk") => Activity::DeskFocus,
        (Activity::Unclear, true) => Activity::ScreenFocus,
        (a, _) => a,
    }
}

fn add(t: &mut Totals, a: Activity, ms: i64) {
    match a {
        Activity::ScreenFocus => {
            t.focus_ms += ms;
            t.screen_ms += ms;
        }
        Activity::DeskFocus => {
            t.focus_ms += ms;
            t.desk_ms += ms;
        }
        Activity::Social | Activity::Phone | Activity::Dozing | Activity::Zoned => t.drift_ms += ms,
        Activity::Away => t.away_ms += ms,
        Activity::Unclear => t.unclear_ms += ms,
        Activity::Unmonitored => t.unmonitored_ms += ms,
        Activity::Break => t.break_ms += ms,
    }
}

pub fn cause_label(s: &SegmentRow) -> Option<String> {
    Some(match effective(s) {
        Activity::Social => s.detail.clone().unwrap_or_else(|| "Social media".into()),
        Activity::Phone => "Phone".into(),
        Activity::Zoned => "Looking away".into(),
        Activity::Dozing => "Dozing".into(),
        _ => return None,
    })
}

/// Unbroken focus stretches as (start_wall, end_wall), per session.
fn stretches(segs: &[SegmentRow]) -> Vec<(i64, i64, i64)> {
    let mut out = Vec::new();
    let mut cur: Option<(i64, i64, i64)> = None;
    for s in segs {
        let focus = effective(s).is_focus();
        match (&mut cur, focus) {
            (Some((sid, _, end)), true) if *sid == s.session_id && s.start_wall - *end <= 1_000 => *end = s.end_wall,
            (_, true) => {
                if let Some(c) = cur.take() {
                    out.push(c);
                }
                cur = Some((s.session_id, s.start_wall, s.end_wall));
            }
            (_, false) => {
                if let Some(c) = cur.take() {
                    out.push(c);
                }
            }
        }
    }
    out.extend(cur);
    out.into_iter().map(|(_, a, b)| (a, b, b - a)).collect()
}

fn drift_episodes(segs: &[SegmentRow]) -> Vec<&SegmentRow> {
    let mut out = Vec::new();
    let mut prev: Option<&SegmentRow> = None;
    for s in segs {
        let drift = effective(s).is_drift();
        let continues = prev.is_some_and(|p| p.session_id == s.session_id && effective(p).is_drift());
        if drift && !continues {
            out.push(s);
        }
        prev = Some(s);
    }
    out
}

fn causes(segs: &[SegmentRow]) -> Vec<Cause> {
    let mut map: HashMap<String, (i64, u32)> = HashMap::new();
    for s in segs {
        if let Some(label) = cause_label(s) {
            let e = map.entry(label).or_default();
            e.0 += s.end_wall - s.start_wall;
            e.1 += 1;
        }
    }
    let mut v: Vec<Cause> = map.into_iter().map(|(label, (ms, count))| Cause { label, ms, count }).collect();
    v.sort_by(|a, b| b.ms.cmp(&a.ms).then(a.label.cmp(&b.label)));
    v
}

/// Clips a segment to local-day pieces: (day, local_start, local_end).
fn local_pieces(s: &SegmentRow) -> Vec<(i64, i64, i64)> {
    let (ls, le) = (local_ms(s.start_wall, s.tz_offset_min), local_ms(s.end_wall, s.tz_offset_min));
    let mut out = Vec::new();
    if le <= ls {
        return out;
    }
    let mut day = ls.div_euclid(DAY_MS);
    while day * DAY_MS < le {
        let a = ls.max(day * DAY_MS);
        let b = le.min((day + 1) * DAY_MS);
        if b > a {
            out.push((day, a, b));
        }
        day += 1;
    }
    out
}

fn mins(ms: i64) -> i64 {
    (ms + 30_000) / 60_000
}

fn clock(hour: usize) -> String {
    format!("{}:00", hour % 24)
}

pub fn range_report(segs: &[SegmentRow], prev_segs: &[SegmentRow], from_day: i64, n_days: u32) -> RangeReport {
    let to_day = from_day + n_days as i64;
    let in_range: Vec<SegmentRow> = segs
        .iter()
        .filter(|s| local_pieces(s).iter().any(|(d, _, _)| *d >= from_day && *d < to_day))
        .cloned()
        .collect();

    let mut days: Vec<DayReport> = (0..n_days as i64)
        .map(|i| DayReport {
            date: crate::time::format_date(from_day + i),
            weekday: weekday(from_day + i),
            totals: Totals::default(),
            longest_ms: 0,
            drifts: 0,
            pieces: Vec::new(),
        })
        .collect();
    let mut hours = vec![0i64; 24];
    let mut totals = Totals::default();

    for s in &in_range {
        let a = effective(s);
        for (day, ls, le) in local_pieces(s) {
            if day < from_day || day >= to_day {
                continue;
            }
            let d = &mut days[(day - from_day) as usize];
            add(&mut d.totals, a, le - ls);
            add(&mut totals, a, le - ls);
            d.pieces.push(RibbonPiece {
                start_min: ((ls - day * DAY_MS) / 60_000) as u32,
                end_min: ((le - day * DAY_MS + 59_999) / 60_000).min(1440) as u32,
                activity: a,
            });
            if a.is_focus() {
                let mut h = ls;
                while h < le {
                    let next = ((h / 3_600_000) + 1) * 3_600_000;
                    let hour = ((h - day * DAY_MS) / 3_600_000).clamp(0, 23) as usize;
                    hours[hour] += le.min(next) - h;
                    h = next;
                }
            }
        }
    }

    let st = stretches(&in_range);
    for (a, b, len) in &st {
        if let Some((day, _, _)) = in_range
            .iter()
            .find(|s| s.start_wall <= *a && s.end_wall > *a)
            .and_then(|s| local_pieces(&SegmentRow { start_wall: *a, end_wall: *b, ..s.clone() }).first().copied())
        {
            if day >= from_day && day < to_day {
                let d = &mut days[(day - from_day) as usize];
                d.longest_ms = d.longest_ms.max(*len);
            }
        }
    }
    let episodes = drift_episodes(&in_range);
    for e in &episodes {
        if let Some((day, _, _)) = local_pieces(e).first() {
            if *day >= from_day && *day < to_day {
                days[(*day - from_day) as usize].drifts += 1;
            }
        }
    }

    let prev_focus_ms = prev_segs.iter().filter(|s| effective(s).is_focus()).map(|s| s.end_wall - s.start_wall).sum();
    let longest_ms = st.iter().map(|s| s.2).max().unwrap_or(0);
    let insights = insights(&in_range, &st, &episodes, &hours, totals.focus_ms, n_days);

    RangeReport {
        from_date: crate::time::format_date(from_day),
        days,
        hours,
        causes: causes(&in_range),
        totals,
        prev_focus_ms,
        longest_ms,
        insights,
    }
}

fn insights(
    segs: &[SegmentRow],
    st: &[(i64, i64, i64)],
    episodes: &[&SegmentRow],
    hours: &[i64],
    focus_ms: i64,
    n_days: u32,
) -> Vec<String> {
    let mut out = Vec::new();
    if focus_ms < 30 * 60_000 {
        return out;
    }
    let period = match n_days {
        1 => "day",
        2..=7 => "week",
        _ => "month",
    };

    let (best, best_ms) = (0..23).map(|h| (h, hours[h] + hours[h + 1])).max_by_key(|x| x.1).unwrap_or((0, 0));
    if best_ms * 3 >= focus_ms {
        let lead = if best + 2 <= 12 { format!("Your mornings carry the {period}. ") } else { String::new() };
        out.push(format!("{lead}Most focus lands between {} and {}.", clock(best), clock(best + 2)));
    }

    if let Some((a, _, len)) = st.iter().max_by_key(|s| s.2).copied() {
        if len >= 20 * 60_000 {
            let tz = segs.iter().find(|s| s.start_wall <= a && s.end_wall > a).map_or(0, |s| s.tz_offset_min);
            let local = local_ms(a, tz);
            let part = part_of_day(local.rem_euclid(DAY_MS) / 60_000);
            if n_days == 1 {
                out.push(format!("Your longest stretch was {} minutes, in the {part}.", mins(len)));
            } else {
                let day = WEEKDAYS[weekday(local.div_euclid(DAY_MS)) as usize];
                out.push(format!("{day} {part} held your longest stretch: {} minutes, unbroken.", mins(len)));
            }
        }
    }

    let phone: Vec<i64> = episodes
        .iter()
        .filter(|e| e.activity == Activity::Phone)
        .map(|e| {
            // An episode can span several drift segments; measure the phone part.
            e.end_wall - e.start_wall
        })
        .collect();
    if phone.len() >= 4 {
        let half = phone.len() / 2;
        let early = phone[..half].iter().sum::<i64>() as f64 / half as f64;
        let late = phone[half..].iter().sum::<i64>() as f64 / (phone.len() - half) as f64;
        let (e, l) = (((early / 60_000.0).round() as i64).max(1), ((late / 60_000.0).round() as i64).max(1));
        if late < early * 0.8 && l < e {
            out.push(format!("Phone breaks got shorter: {e} min on average early on, {l} min later."));
        } else if late > early * 1.25 && l > e {
            out.push(format!("Phone breaks are getting longer: {e} min on average early on, {l} min later."));
        }
    }

    if out.len() < 3 && episodes.len() >= 4 {
        let session_start: HashMap<i64, i64> = segs.iter().fold(HashMap::new(), |mut m, s| {
            let e = m.entry(s.session_id).or_insert(s.start_wall);
            *e = (*e).min(s.start_wall);
            m
        });
        let early = episodes
            .iter()
            .filter(|e| session_start.get(&e.session_id).is_some_and(|st| e.start_wall - st <= 10 * 60_000))
            .count();
        if early * 2 >= episodes.len() {
            out.push("Most drifts happen in the first 10 minutes of a session.".into());
        }
    }

    out.truncate(3);
    out
}

pub fn session_summary(sess: &SessionRow, segs: &[SegmentRow]) -> SessionSummary {
    let mut totals = Totals::default();
    for s in segs {
        add(&mut totals, effective(s), s.end_wall - s.start_wall);
    }
    let started = segs.first().map_or(sess.started_wall, |s| s.start_wall.min(sess.started_wall));
    let ended = segs.last().map_or(sess.ended_wall.unwrap_or(started), |s| s.end_wall).max(started);
    let present_ms = (ended - started - totals.away_ms - totals.break_ms).max(0);

    let st = stretches(segs);
    let longest = st.iter().max_by_key(|s| s.2).copied();
    let longest_ms = longest.map_or(0, |s| s.2);

    let headline = if totals.focus_ms < 60_000 {
        "Not much focus this time.".to_string()
    } else {
        format!("{} of real work.", format_duration(totals.focus_ms))
    };
    let mut sentence = format!("Out of {} at your desk.", format_duration(present_ms));
    if let Some((a, _, len)) = longest.filter(|l| l.2 >= 5 * 60_000) {
        let after_phone = segs
            .iter()
            .rev()
            .find(|s| s.end_wall <= a + 1_000 && s.end_wall >= a - 60_000 && !effective(s).is_focus())
            .is_some_and(|s| s.activity == Activity::Phone);
        sentence.push_str(&format!(" Your longest stretch was {} minutes", mins(len)));
        sentence.push_str(if after_phone { ", right after you put your phone away." } else { "." });
    }
    if totals.unmonitored_ms >= 60_000 {
        sentence.push_str(&format!(" {} couldn't be monitored.", format_duration(totals.unmonitored_ms)));
    }

    SessionSummary {
        session_id: sess.id,
        label: sess.label.clone(),
        started_wall: started,
        ended_wall: ended,
        tz_offset_min: sess.tz_offset_min,
        totals,
        present_ms,
        longest_ms,
        causes: causes(segs),
        unclear: segs
            .iter()
            .filter(|s| s.activity == Activity::Unclear && s.end_wall - s.start_wall >= 60_000)
            .map(|s| UnclearSpan {
                segment_id: s.id,
                start_wall: s.start_wall,
                end_wall: s.end_wall,
                at_desk: s.detail.as_deref() == Some("desk"),
                claimed: s.claimed,
            })
            .collect(),
        segments: segs.iter().map(view).collect(),
        headline,
        sentence,
    }
}

pub fn view(s: &SegmentRow) -> SegmentView {
    SegmentView {
        id: s.id,
        activity: s.activity,
        start_wall: s.start_wall,
        end_wall: s.end_wall,
        detail: s.detail.clone(),
        claimed: s.claimed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::parse_date;

    fn seg(id: i64, session: i64, a: Activity, start_min: i64, end_min: i64, detail: Option<&str>) -> SegmentRow {
        let base = parse_date("2026-10-12").unwrap() * DAY_MS - 9 * 3_600_000; // local midnight at +09:00
        SegmentRow {
            id,
            session_id: session,
            start_wall: base + start_min * 60_000,
            end_wall: base + end_min * 60_000,
            activity: a,
            detail: detail.map(str::to_string),
            claimed: false,
            tz_offset_min: 540,
        }
    }

    fn evening() -> Vec<SegmentRow> {
        use Activity::*;
        let s = |id, a, from: i64, to: i64, d| seg(id, 1, a, 19 * 60 + from, 19 * 60 + to, d);
        vec![
            s(1, ScreenFocus, 0, 20, None),
            s(2, Social, 20, 25, Some("instagram.com")),
            s(3, DeskFocus, 25, 51, None),
            s(4, Phone, 51, 59, None),
            s(5, ScreenFocus, 59, 91, None),
            s(6, DeskFocus, 91, 111, None),
            s(7, Away, 111, 116, None),
            s(8, Unclear, 116, 130, Some("desk")),
            s(9, ScreenFocus, 130, 140, None),
        ]
    }

    #[test]
    fn summary_matches_mockup() {
        let segs = evening();
        let sess = SessionRow {
            id: 1,
            started_wall: segs[0].start_wall,
            ended_wall: Some(segs[8].end_wall),
            tz_offset_min: 540,
            label: Some("Organic chemistry, ch. 4".into()),
            goal_min: Some(90),
            closed: true,
        };
        let sum = session_summary(&sess, &segs);
        assert_eq!(sum.totals.focus_ms, 108 * 60_000);
        assert_eq!(sum.totals.screen_ms, 62 * 60_000);
        assert_eq!(sum.totals.desk_ms, 46 * 60_000);
        assert_eq!(sum.totals.drift_ms, 13 * 60_000);
        assert_eq!(sum.longest_ms, 52 * 60_000);
        assert_eq!(sum.headline, "1 h 48 m of real work.");
        assert_eq!(
            sum.sentence,
            "Out of 2 h 15 m at your desk. Your longest stretch was 52 minutes, right after you put your phone away."
        );
        assert_eq!(sum.unclear.len(), 1);
        assert_eq!(sum.causes[0].label, "Phone");

        let mut claimed = segs.clone();
        claimed[7].claimed = true;
        let sum = session_summary(&sess, &claimed);
        assert_eq!(sum.totals.focus_ms, 122 * 60_000);
        assert_eq!(sum.totals.desk_ms, 60 * 60_000);
    }

    #[test]
    fn day_report_buckets_by_local_time() {
        let segs = evening();
        let day = parse_date("2026-10-12").unwrap();
        let r = range_report(&segs, &[], day, 7);
        assert_eq!(r.days[0].totals.focus_ms, 108 * 60_000);
        assert_eq!(r.days[0].drifts, 2);
        assert_eq!(r.days[0].pieces.first().map(|p| p.start_min), Some(19 * 60));
        assert_eq!(r.hours[19] + r.hours[20] + r.hours[21], 108 * 60_000);
        assert_eq!(r.days[1].totals.focus_ms, 0);
        assert_eq!(r.longest_ms, 52 * 60_000);
        assert!(r.insights.iter().any(|s| s.contains("52 minutes")), "{:?}", r.insights);
    }

    #[test]
    fn segments_crossing_midnight_split() {
        let s = seg(1, 1, Activity::DeskFocus, 23 * 60 + 30, 24 * 60 + 30, None);
        let day = parse_date("2026-10-12").unwrap();
        let r = range_report(&[s], &[], day, 2);
        assert_eq!(r.days[0].totals.focus_ms, 30 * 60_000);
        assert_eq!(r.days[1].totals.focus_ms, 30 * 60_000);
        assert_eq!(r.days[0].pieces[0].end_min, 1440);
    }
}
