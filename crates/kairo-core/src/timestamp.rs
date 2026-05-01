use std::fmt;
use std::str::FromStr;

use crate::canonical::{encode_i64, CanonicalEncode};

/// A Kairo statement timestamp.
///
/// Stored as `i64` Unix epoch seconds. JSON interchange uses strict RFC 3339
/// in UTC: `YYYY-MM-DDTHH:MM:SSZ` with no fractional seconds and no offset
/// other than `Z`. Canonical bytes are `i64` big-endian.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(i64);

impl Timestamp {
    pub const fn from_seconds(seconds: i64) -> Self {
        Self(seconds)
    }

    pub const fn seconds(&self) -> i64 {
        self.0
    }
}

impl CanonicalEncode for Timestamp {
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        encode_i64(out, self.0);
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (year, month, day, hour, minute, second) = civil_from_seconds(self.0);
        write!(
            f,
            "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z"
        )
    }
}

impl FromStr for Timestamp {
    type Err = TimestampError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let bytes = value.as_bytes();

        if bytes.len() != 20 {
            return Err(TimestampError::InvalidFormat);
        }

        for (index, expected) in [(4, b'-'), (7, b'-'), (10, b'T'), (13, b':'), (16, b':')] {
            if bytes[index] != expected {
                return Err(TimestampError::InvalidFormat);
            }
        }

        if bytes[19] != b'Z' {
            return Err(TimestampError::NonUtcOffset);
        }

        let year = parse_digits(&bytes[0..4])?;
        let month = parse_digits(&bytes[5..7])?;
        let day = parse_digits(&bytes[8..10])?;
        let hour = parse_digits(&bytes[11..13])?;
        let minute = parse_digits(&bytes[14..16])?;
        let second = parse_digits(&bytes[17..19])?;

        if !(1..=12).contains(&month) {
            return Err(TimestampError::OutOfRange);
        }

        let days_in_month = days_in_month(year, month);
        if day < 1 || day > days_in_month {
            return Err(TimestampError::OutOfRange);
        }

        if hour > 23 || minute > 59 || second > 59 {
            return Err(TimestampError::OutOfRange);
        }

        let seconds = seconds_from_civil(year, month, day, hour, minute, second);
        Ok(Self(seconds))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimestampError {
    InvalidFormat,
    NonUtcOffset,
    OutOfRange,
}

impl fmt::Display for TimestampError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidFormat => f.write_str(
                "invalid timestamp; expected strict RFC 3339 UTC seconds (YYYY-MM-DDTHH:MM:SSZ)",
            ),
            Self::NonUtcOffset => {
                f.write_str("timestamp must use UTC offset Z; other offsets are not accepted")
            }
            Self::OutOfRange => f.write_str("timestamp field out of range"),
        }
    }
}

impl std::error::Error for TimestampError {}

fn parse_digits(bytes: &[u8]) -> Result<u32, TimestampError> {
    let mut value: u32 = 0;
    for byte in bytes {
        match byte {
            b'0'..=b'9' => value = value * 10 + u32::from(*byte - b'0'),
            _ => return Err(TimestampError::InvalidFormat),
        }
    }
    Ok(value)
}

fn is_leap_year(year: u32) -> bool {
    (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400)
}

fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap_year(year) {
                29
            } else {
                28
            }
        }
        _ => 0,
    }
}

/// Howard Hinnant's `days_from_civil` algorithm: returns days since
/// 1970-01-01 (negative for earlier dates) for any proleptic Gregorian date.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let m = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * m + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn seconds_from_civil(year: u32, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> i64 {
    let days = days_from_civil(i64::from(year), i64::from(month), i64::from(day));
    days * 86_400 + i64::from(hour) * 3_600 + i64::from(minute) * 60 + i64::from(second)
}

/// Inverse of `days_from_civil`. `days` is days since 1970-01-01.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };
    let month_u32 = u32::try_from(month).unwrap_or(0);
    let day_u32 = u32::try_from(day).unwrap_or(0);
    (year, month_u32, day_u32)
}

fn civil_from_seconds(seconds: i64) -> (i64, u32, u32, u32, u32, u32) {
    let days = seconds.div_euclid(86_400);
    let time_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = u32::try_from(time_of_day / 3_600).unwrap_or(0);
    let minute = u32::try_from((time_of_day % 3_600) / 60).unwrap_or(0);
    let second = u32::try_from(time_of_day % 60).unwrap_or(0);
    (year, month, day, hour, minute, second)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_round_trip() {
        let timestamp = Timestamp::from_seconds(0);
        assert_eq!(timestamp.to_string(), "1970-01-01T00:00:00Z");
        assert_eq!(
            "1970-01-01T00:00:00Z".parse::<Timestamp>(),
            Ok(Timestamp::from_seconds(0))
        );
    }

    #[test]
    fn parses_arbitrary_date() {
        let parsed = "2026-05-01T14:32:07Z".parse::<Timestamp>();
        assert!(matches!(parsed, Ok(timestamp) if timestamp.to_string() == "2026-05-01T14:32:07Z"));
    }

    #[test]
    fn parses_leap_day() {
        let parsed = "2024-02-29T00:00:00Z".parse::<Timestamp>();
        assert!(matches!(parsed, Ok(timestamp) if timestamp.to_string() == "2024-02-29T00:00:00Z"));
    }

    #[test]
    fn rejects_non_z_offset() {
        assert_eq!(
            "2026-05-01T14:32:07+00:00".parse::<Timestamp>(),
            Err(TimestampError::InvalidFormat)
        );
        assert_eq!(
            "2026-05-01T14:32:07+0000".parse::<Timestamp>(),
            Err(TimestampError::InvalidFormat)
        );
    }

    #[test]
    fn rejects_fractional_seconds() {
        assert_eq!(
            "2026-05-01T14:32:07.500Z".parse::<Timestamp>(),
            Err(TimestampError::InvalidFormat)
        );
    }

    #[test]
    fn rejects_lowercase_t_or_z() {
        assert_eq!(
            "2026-05-01t14:32:07Z".parse::<Timestamp>(),
            Err(TimestampError::InvalidFormat)
        );
        assert_eq!(
            "2026-05-01T14:32:07z".parse::<Timestamp>(),
            Err(TimestampError::NonUtcOffset)
        );
    }

    #[test]
    fn rejects_invalid_day_for_month() {
        assert_eq!(
            "2025-02-29T00:00:00Z".parse::<Timestamp>(),
            Err(TimestampError::OutOfRange)
        );
        assert_eq!(
            "2025-04-31T00:00:00Z".parse::<Timestamp>(),
            Err(TimestampError::OutOfRange)
        );
    }

    #[test]
    fn rejects_leap_second() {
        assert_eq!(
            "2026-05-01T14:32:60Z".parse::<Timestamp>(),
            Err(TimestampError::OutOfRange)
        );
    }

    #[test]
    fn canonical_bytes_are_i64_be() {
        let timestamp = Timestamp::from_seconds(0x0102_0304_0506_0708);
        let mut bytes = Vec::new();
        timestamp.encode_canonical(&mut bytes);
        assert_eq!(bytes, vec![0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08]);
    }

    #[test]
    fn negative_seconds_round_trip() {
        // 1969-12-31T23:59:59Z is one second before the epoch.
        let timestamp = Timestamp::from_seconds(-1);
        assert_eq!(timestamp.to_string(), "1969-12-31T23:59:59Z");
        assert_eq!(
            "1969-12-31T23:59:59Z".parse::<Timestamp>(),
            Ok(Timestamp::from_seconds(-1))
        );
    }

    #[test]
    fn parses_zero_padded_year() {
        let parsed = "0001-01-01T00:00:00Z".parse::<Timestamp>();
        assert!(matches!(parsed, Ok(timestamp) if timestamp.to_string() == "0001-01-01T00:00:00Z"));
    }
}
