//! RFC 3339 formatting, because a `no_std` crate cannot use `chrono`
//! (decision 35).
//!
//! A probe formats the timestamp it computed: the value has to be the
//! *target's* own clock (decision 19), so it cannot be produced by the server
//! after the fact.

use alloc::format;
use alloc::string::String;

/// Seconds since the Unix epoch as RFC 3339 in UTC, to one-second resolution:
/// `2026-09-20T14:03:11Z`.
///
/// No fractional part, because the sources are whole seconds and implying more
/// precision than the reading has would be its own small lie. `None` before
/// 1970 or beyond year 9999, which for a boot time means something is wrong
/// rather than merely unusual.
pub fn rfc3339(epoch_seconds: i64) -> Option<String> {
    if epoch_seconds < 0 {
        return None;
    }
    let days = epoch_seconds / 86_400;
    let secs = epoch_seconds % 86_400;
    let (year, month, day) = civil_from_days(days)?;
    Some(format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        secs / 3600,
        (secs % 3600) / 60,
        secs % 60,
    ))
}

/// Days since 1970-01-01 to a civil date.
///
/// Howard Hinnant's `civil_from_days`, which shifts the epoch to 0000-03-01 so
/// that leap days land at the end of the era and the month arithmetic needs no
/// table. Exact for the whole range this returns.
fn civil_from_days(days: i64) -> Option<(i64, u32, u32)> {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097); // [0, 146096], days into the 400-year era
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365], from 1 March
    let mp = (5 * doy + 2) / 153; // [0, 11], March = 0
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = if month <= 2 { y + 1 } else { y };
    (0..=9999).contains(&year).then_some((year, month, day))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_instants() {
        assert_eq!(rfc3339(0).unwrap(), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339(1).unwrap(), "1970-01-01T00:00:01Z");
        assert_eq!(rfc3339(86_399).unwrap(), "1970-01-01T23:59:59Z");
        assert_eq!(rfc3339(86_400).unwrap(), "1970-01-02T00:00:00Z");
        // A leap day, and the day after it.
        assert_eq!(rfc3339(1_709_164_800).unwrap(), "2024-02-29T00:00:00Z");
        assert_eq!(rfc3339(1_709_251_200).unwrap(), "2024-03-01T00:00:00Z");
        // 1900 was not a leap year and 2000 was; both eras are exercised here.
        assert_eq!(rfc3339(951_782_400).unwrap(), "2000-02-29T00:00:00Z");
        assert_eq!(rfc3339(1_774_008_191).unwrap(), "2026-03-20T12:03:11Z");
        assert_eq!(rfc3339(253_402_300_799).unwrap(), "9999-12-31T23:59:59Z");
    }

    #[test]
    fn out_of_range_is_none() {
        assert!(rfc3339(-1).is_none());
        assert!(rfc3339(253_402_300_800).is_none()); // year 10000
    }

    #[test]
    fn every_day_for_a_century_round_trips() {
        // Walks 1970-01-01 to 2069-12-31 a day at a time, checking the date
        // advances exactly as a calendar does. Catches an off-by-one in the
        // era arithmetic that spot checks would miss.
        const DAYS_IN: [u32; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
        let (mut y, mut m, mut d) = (1970i64, 1u32, 1u32);
        for day in 0..36_525i64 {
            let expected = format!("{y:04}-{m:02}-{d:02}T00:00:00Z");
            assert_eq!(rfc3339(day * 86_400).unwrap(), expected, "day {day}");
            let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
            let last = if m == 2 && leap {
                29
            } else {
                DAYS_IN[m as usize - 1]
            };
            d += 1;
            if d > last {
                d = 1;
                m += 1;
            }
            if m > 12 {
                m = 1;
                y += 1;
            }
        }
        assert_eq!((y, m, d), (2070, 1, 1));
    }
}
