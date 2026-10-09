//! Moments, as lattice keeps them, seconds since the Unix epoch, and as it
//! says them: RFC 3339 in UTC, `2026-10-09T12:00:00Z`, which `wiki.json`
//! and the server's API carry and a browser's `Date` reads.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

/// A moment, to the second.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(pub u64);

impl Timestamp {
    /// Now, by this machine's clock.
    pub fn now() -> Timestamp {
        let since = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        Timestamp(since.as_secs())
    }

    /// The moment `text` says, written the way [`Timestamp`] writes one.
    pub fn parse(text: &str) -> Option<Timestamp> {
        let bytes = text.as_bytes();
        let shaped = bytes.len() == 20
            && [
                (4, b'-'),
                (7, b'-'),
                (10, b'T'),
                (13, b':'),
                (16, b':'),
                (19, b'Z'),
            ]
            .iter()
            .all(|&(at, byte)| bytes[at] == byte);
        if !shaped {
            return None;
        }
        let number = |from: usize, to: usize| -> Option<i64> {
            let digits = text.get(from..to)?;
            digits
                .bytes()
                .all(|byte| byte.is_ascii_digit())
                .then(|| digits.parse().ok())?
        };
        let (year, month, day) = (number(0, 4)?, number(5, 7)?, number(8, 10)?);
        let (hour, minute, second) = (number(11, 13)?, number(14, 16)?, number(17, 19)?);
        let valid = (1970..).contains(&year)
            && (1..=12).contains(&month)
            && (1..=days_in(year, month)).contains(&day)
            && hour < 24
            && minute < 60
            && second < 60;
        if !valid {
            return None;
        }
        let days = days_from_civil(year, month, day);
        Some(Timestamp(
            (days * 86_400 + hour * 3600 + minute * 60 + second) as u64,
        ))
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let days = (self.0 / 86_400) as i64;
        let seconds = self.0 % 86_400;
        let (year, month, day) = civil_from_days(days);
        write!(
            f,
            "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60
        )
    }
}

impl Serialize for Timestamp {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Timestamp, D::Error> {
        let text = String::deserialize(deserializer)?;
        Timestamp::parse(&text)
            .ok_or_else(|| serde::de::Error::custom(format!("{text} isn't a moment in RFC 3339")))
    }
}

/// How many days `month` of `year` has.
fn days_in(year: i64, month: i64) -> i64 {
    match month {
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// The year, month and day `days` after 1970-01-01: Howard Hinnant's
/// `civil_from_days`.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// The days from 1970-01-01 to `year`-`month`-`day`: its
/// `days_from_civil`.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year.rem_euclid(400);
    let mp = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_moment_is_written_in_rfc_3339_and_read_back() {
        for (seconds, text) in [
            (0, "1970-01-01T00:00:00Z"),
            (951_782_400, "2000-02-29T00:00:00Z"),
            (1_791_547_200, "2026-10-09T12:00:00Z"),
            (4_102_444_799, "2099-12-31T23:59:59Z"),
        ] {
            assert_eq!(Timestamp(seconds).to_string(), text);
            assert_eq!(Timestamp::parse(text), Some(Timestamp(seconds)), "{text}");
        }
        let json = serde_json::to_string(&Timestamp(1_791_547_200)).unwrap();
        assert_eq!(json, "\"2026-10-09T12:00:00Z\"");
        let back: Timestamp = serde_json::from_str(&json).unwrap();
        assert_eq!(back, Timestamp(1_791_547_200));
    }

    #[test]
    fn what_isn_t_a_moment_isn_t_read() {
        for text in [
            "",
            "2026-10-09",
            "2026-10-09T12:00:00",
            "2026-10-09 12:00:00Z",
            "2026-13-09T12:00:00Z",
            "2026-02-29T12:00:00Z",
            "2026-10-09T24:00:00Z",
            "+026-10-09T12:00:00Z",
            "1969-12-31T23:59:59Z",
        ] {
            assert_eq!(Timestamp::parse(text), None, "{text}");
        }
        assert!(serde_json::from_str::<Timestamp>("\"yesterday\"").is_err());
    }
}
