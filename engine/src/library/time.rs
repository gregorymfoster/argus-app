//! Timestamps as ISO 8601 text, without pulling in a date crate.

use std::time::{SystemTime, UNIX_EPOCH};

/// Now, in UTC, e.g. `2026-10-18T01:02:03Z`.
pub fn now() -> String {
    from_system(SystemTime::now()) + "Z"
}

/// A file time as ISO 8601 (UTC, without the zone so it sorts with EXIF local times).
pub fn from_system(t: SystemTime) -> String {
    let secs = t
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// EXIF `2017:06:20 14:33:01` -> `2017-06-20T14:33:01`.
pub fn from_exif(s: &str) -> Option<String> {
    let b = s.trim().as_bytes();
    if b.len() < 19
        || !b[..19].iter().enumerate().all(|(i, c)| match i {
            4 | 7 => *c == b':' || *c == b'-',
            10 => *c == b' ' || *c == b'T',
            13 | 16 => *c == b':',
            _ => c.is_ascii_digit(),
        })
    {
        return None;
    }
    let s = &s.trim()[..19];
    Some(format!(
        "{}-{}-{}T{}",
        &s[0..4],
        &s[5..7],
        &s[8..10],
        &s[11..19]
    ))
}

/// Seconds from `a` to `b`, both `YYYY-MM-DDTHH:MM:SS` (a trailing `Z` is fine).
pub fn seconds_between(a: &str, b: &str) -> Option<f64> {
    Some((to_secs(b)? - to_secs(a)?) as f64)
}

/// A capture time `YYYY-MM-DDTHH:MM:SS...` as Unix seconds, for gaps between frames: a
/// missing time of day reads as midnight, an unparseable date as 0.
pub fn capture_seconds(t: &str) -> i64 {
    let n = |r: std::ops::Range<usize>| t.get(r)?.parse::<i64>().ok();
    let Some(days) = days_of(t) else {
        return 0;
    };
    let (hh, mm, ss) = (
        n(11..13).unwrap_or(0),
        n(14..16).unwrap_or(0),
        n(17..19).unwrap_or(0),
    );
    days * 86_400 + hh * 3600 + mm * 60 + ss
}

fn to_secs(s: &str) -> Option<i64> {
    let n = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    Some(days_of(s)? * 86_400 + n(11..13)? * 3600 + n(14..16)? * 60 + n(17..19)?)
}

/// Days since 1970-01-01 of a `YYYY-MM-DD...` date (Howard Hinnant's days from civil, the
/// inverse of [`civil_from_days`]).
fn days_of(s: &str) -> Option<i64> {
    let n = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, m, d) = (n(0..4)?, n(5..7)?, n(8..10)?);
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    Some(era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468)
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn formats_unix_times() {
        assert_eq!(from_system(UNIX_EPOCH), "1970-01-01T00:00:00");
        let t = UNIX_EPOCH + Duration::from_secs(1_497_969_181);
        assert_eq!(from_system(t), "2017-06-20T14:33:01");
    }

    #[test]
    fn measures_seconds_between_times() {
        assert_eq!(
            seconds_between("2026-10-18T23:59:30Z", "2026-10-19T00:01:00Z"),
            Some(90.0)
        );
        let t = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_760_000_000);
        assert_eq!(to_secs(&from_system(t)), Some(1_760_000_000));
        assert_eq!(capture_seconds(&from_system(t)), 1_760_000_000);
        assert_eq!(
            capture_seconds("2026-10-18"),
            capture_seconds("2026-10-18T00:00:00")
        );
        assert_eq!(capture_seconds("unknown"), 0);
    }

    #[test]
    fn parses_exif_times() {
        assert_eq!(
            from_exif("2017:06:20 14:33:01").as_deref(),
            Some("2017-06-20T14:33:01")
        );
        assert_eq!(from_exif("0000:00:00"), None);
        assert_eq!(from_exif("    :  :     :  :  "), None);
    }
}
