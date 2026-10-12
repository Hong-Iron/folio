//! Calendar arithmetic without a date library (Howard Hinnant's algorithms).

pub const DAY_MS: i64 = 86_400_000;

pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = m as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

pub fn parse_date(s: &str) -> Option<i64> {
    let mut it = s.trim().split('-');
    let y = it.next()?.parse::<i64>().ok()?;
    let m = it.next()?.parse::<u32>().ok()?;
    let d = it.next()?.parse::<u32>().ok()?;
    if it.next().is_some() || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let days = days_from_civil(y, m, d);
    (civil_from_days(days) == (y, m, d)).then_some(days)
}

pub fn format_date(days: i64) -> String {
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// 0 = Monday.
pub fn weekday(days: i64) -> u32 {
    (days + 3).rem_euclid(7) as u32
}

pub const WEEKDAYS: [&str; 7] = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];

/// Local milliseconds since the epoch for a UTC instant and a fixed offset.
pub fn local_ms(wall_ms: i64, tz_offset_min: i32) -> i64 {
    wall_ms + tz_offset_min as i64 * 60_000
}

pub fn part_of_day(minute_of_day: i64) -> &'static str {
    match minute_of_day / 60 {
        0..=4 => "night",
        5..=11 => "morning",
        12..=16 => "afternoon",
        17..=20 => "evening",
        _ => "night",
    }
}

/// "1 h 48 m", "46 m", "40 s".
pub fn format_duration(ms: i64) -> String {
    let s = ms.max(0) / 1000;
    let (h, m) = (s / 3600, (s % 3600) / 60);
    match (h, m) {
        (0, 0) => format!("{s} s"),
        (0, m) => format!("{m} m"),
        (h, m) => format!("{h} h {m:02} m"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_round_trip() {
        for days in [-1000, 0, 19_000, 20_373, 60_000] {
            let (y, m, d) = civil_from_days(days);
            assert_eq!(days_from_civil(y, m, d), days);
        }
        assert_eq!(parse_date("2026-10-12").map(format_date).as_deref(), Some("2026-10-12"));
        assert_eq!(parse_date("2026-02-30"), None);
        assert_eq!(weekday(parse_date("2026-10-12").unwrap()), 0);
    }

    #[test]
    fn durations() {
        assert_eq!(format_duration(6_480_000), "1 h 48 m");
        assert_eq!(format_duration(2_760_000), "46 m");
        assert_eq!(format_duration(40_000), "40 s");
    }
}
