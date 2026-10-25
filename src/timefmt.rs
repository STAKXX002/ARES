//! Small, dependency-free time formatting helpers.

/// Format seconds since the Unix epoch as `YYYY-MM-DD HH:MM UTC`.
pub fn format_utc(secs: u64) -> String {
    let days = secs / 86_400;
    let rem = secs % 86_400;
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02} UTC", rem / 3600, (rem % 3600) / 60)
}

/// Convert days since 1970-01-01 to a (year, month, day) in the proleptic
/// Gregorian calendar. Algorithm by Howard Hinnant.
fn civil_from_days(days: u64) -> (u64, u64, u64) {
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + u64::from(m <= 2);
    (y, m, d)
}

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn days_in_month(y: i64, m: i64) -> i64 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            if is_leap(y) {
                29
            } else {
                28
            }
        }
    }
}

/// Days since 1970-01-01 for a civil date. Algorithm by Howard Hinnant.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Parse a UTC timestamp of the exact form `YYYY-MM-DDTHH:MM:SSZ`, as GitHub
/// returns. Returns seconds since the Unix epoch, or `None` for anything
/// else, including impossible dates and years before 1970.
pub fn parse_rfc3339_utc(s: &str) -> Option<u64> {
    let b = s.as_bytes();
    if b.len() != 20
        || b[4] != b'-'
        || b[7] != b'-'
        || b[10] != b'T'
        || b[13] != b':'
        || b[16] != b':'
        || b[19] != b'Z'
    {
        return None;
    }
    let num = |from: usize, to: usize| -> Option<i64> {
        let part = &b[from..to];
        if part.iter().all(u8::is_ascii_digit) {
            std::str::from_utf8(part).ok()?.parse().ok()
        } else {
            None
        }
    };
    let (y, mo, d) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (h, mi, sec) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
    if y < 1970 || !(1..=12).contains(&mo) || d < 1 || d > days_in_month(y, mo) {
        return None;
    }
    if h > 23 || mi > 59 || sec > 59 {
        return None;
    }
    let days = days_from_civil(y, mo, d);
    u64::try_from(days * 86_400 + h * 3_600 + mi * 60 + sec).ok()
}

fn plural(n: u64, unit: &str) -> String {
    format!("{} {}{} ago", n, unit, if n == 1 { "" } else { "s" })
}

/// Describe how long before `now` the time `then` was, coarsely.
pub fn ago(now: u64, then: u64) -> String {
    let diff = now.saturating_sub(then);
    if diff < 60 {
        "just now".to_string()
    } else if diff < 3_600 {
        plural(diff / 60, "minute")
    } else if diff < 86_400 {
        plural(diff / 3_600, "hour")
    } else {
        plural(diff / 86_400, "day")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_and_known_dates() {
        assert_eq!(format_utc(0), "1970-01-01 00:00 UTC");
        assert_eq!(format_utc(31_536_000), "1971-01-01 00:00 UTC");
        assert_eq!(format_utc(1_700_000_000), "2023-11-14 22:13 UTC");
    }

    #[test]
    fn leap_days_are_handled() {
        assert_eq!(format_utc(951_782_400), "2000-02-29 00:00 UTC");
        // 2100 is not a leap year, so the day after Feb 28 is Mar 1.
        assert_eq!(format_utc(4_102_444_800), "2100-01-01 00:00 UTC");
        assert_eq!(format_utc(4_107_542_400), "2100-03-01 00:00 UTC");
    }

    #[test]
    fn end_of_day_and_minutes() {
        assert_eq!(format_utc(86_399), "1970-01-01 23:59 UTC");
        assert_eq!(format_utc(86_400 + 3_661), "1970-01-02 01:01 UTC");
    }

    #[test]
    fn ago_uses_the_coarsest_sensible_unit() {
        assert_eq!(ago(1_000, 1_000), "just now");
        assert_eq!(ago(1_059, 1_000), "just now");
        assert_eq!(ago(1_060, 1_000), "1 minute ago");
        assert_eq!(ago(1_000 + 7_200, 1_000), "2 hours ago");
        assert_eq!(ago(1_000 + 3_599, 1_000), "59 minutes ago");
        assert_eq!(ago(1_000 + 86_400, 1_000), "1 day ago");
        assert_eq!(ago(1_000 + 86_400 * 12, 1_000), "12 days ago");
    }

    #[test]
    fn parses_known_timestamps() {
        assert_eq!(parse_rfc3339_utc("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_rfc3339_utc("2023-11-14T22:13:20Z"), Some(1_700_000_000));
        assert_eq!(parse_rfc3339_utc("2000-02-29T00:00:00Z"), Some(951_782_400));
        assert_eq!(parse_rfc3339_utc("2100-03-01T00:00:00Z"), Some(4_107_542_400));
        assert_eq!(parse_rfc3339_utc("1970-01-02T01:01:01Z"), Some(86_400 + 3_661));
    }

    #[test]
    fn parse_and_format_agree_over_many_days() {
        for day in (0..60_000u64).step_by(7) {
            let t = day * 86_400 + 12 * 3_600 + 34 * 60;
            let text = format_utc(t); // "YYYY-MM-DD HH:MM UTC"
            let iso = format!("{}T{}:00Z", &text[..10], &text[11..16]);
            assert_eq!(parse_rfc3339_utc(&iso), Some(t), "{iso}");
        }
    }

    #[test]
    fn rejects_impossible_dates_and_times() {
        for bad in [
            "2023-02-29T00:00:00Z", // not a leap year
            "2100-02-29T00:00:00Z", // century, not a leap year
            "2024-04-31T00:00:00Z",
            "2024-13-01T00:00:00Z",
            "2024-00-10T00:00:00Z",
            "2024-01-00T00:00:00Z",
            "2024-01-01T24:00:00Z",
            "2024-01-01T00:60:00Z",
            "2024-01-01T00:00:60Z",
            "1969-12-31T23:59:59Z",
        ] {
            assert_eq!(parse_rfc3339_utc(bad), None, "{bad}");
        }
        assert!(parse_rfc3339_utc("2024-02-29T00:00:00Z").is_some());
        assert!(parse_rfc3339_utc("2000-02-29T00:00:00Z").is_some());
    }

    #[test]
    fn rejects_malformed_text() {
        for bad in [
            "",
            "2024-01-01",
            "2024-01-01 00:00:00Z",
            "2024-01-01T00:00:00",
            "2024-01-01T00:00:00z",
            "2024-01-01T00:00:00+00:00",
            "2024-01-01T00:00:00.5Z",
            "2024-01-01T00:00:00Zjunk",
            "+024-01-01T00:00:00Z",
            "2024-1-01T00:00:00Z",
            "\u{662}024-01-01T00:00:00Z",
        ] {
            assert_eq!(parse_rfc3339_utc(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn a_future_timestamp_does_not_underflow() {
        assert_eq!(ago(100, 500), "just now");
    }
}
