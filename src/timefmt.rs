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
    fn a_future_timestamp_does_not_underflow() {
        assert_eq!(ago(100, 500), "just now");
    }
}
