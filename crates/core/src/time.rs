//! Time helpers. Timestamps are stored as Unix epoch milliseconds.

use std::time::{SystemTime, UNIX_EPOCH};

/// Current time in Unix epoch milliseconds (0 if the clock is before 1970).
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// `YYYYMMDD-HHMMSS` in UTC, e.g. for file names that sort by time.
pub fn compact_utc(epoch_ms: i64) -> String {
    let secs = epoch_ms.div_euclid(1000);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Civil date from days since 1970-01-01 (H. Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}{month:02}{day:02}-{:02}{:02}{:02}",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_utc_formats_dates() {
        assert_eq!(compact_utc(0), "19700101-000000");
        assert_eq!(compact_utc(951_782_400_000), "20000229-000000");
        assert_eq!(compact_utc(1_793_219_998_000), "20261028-203958");
        assert!(compact_utc(1_793_219_998_000) < compact_utc(1_793_219_999_000));
    }
}
