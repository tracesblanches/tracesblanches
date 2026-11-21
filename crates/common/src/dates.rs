//! Checked calendar constructors and UTC conversions over jiff (#106).
//!
//! The workspace keeps its calendar fields as `i32` years and `u32` months, days, hours and
//! minutes (file formats, tables, arithmetic); jiff stores them as `i16` / `i8`. These
//! functions convert with range checks, so an out-of-range field is `None`, never a value
//! wrapped into another valid date.

use jiff::Timestamp;
use jiff::civil::{Date, DateTime, Time};
use jiff::tz::Offset;

/// The date `year-month-day`, `None` when it does not exist (or is outside the years
/// -9999..=9999 jiff represents).
pub fn ymd(year: i32, month: u32, day: u32) -> Option<Date> {
    Date::new(
        i16::try_from(year).ok()?,
        i8::try_from(month).ok()?,
        i8::try_from(day).ok()?,
    )
    .ok()
}

/// The time `hour:minute:second`, `None` when a field is out of range.
pub fn hms(hour: u32, minute: u32, second: u32) -> Option<Time> {
    Time::new(
        i8::try_from(hour).ok()?,
        i8::try_from(minute).ok()?,
        i8::try_from(second).ok()?,
        0,
    )
    .ok()
}

/// The instant of a UTC civil date-time; `None` only at the two ends of the jiff range
/// (the first and last day of years -9999 and 9999), where no instant exists.
pub fn utc(at: DateTime) -> Option<Timestamp> {
    Offset::UTC.to_timestamp(at).ok()
}

/// The UTC civil date-time of an instant.
pub fn utc_datetime(t: Timestamp) -> DateTime {
    Offset::UTC.to_datetime(t)
}

/// `YYYY-MM-DD HH:MM:SS` with a space, the fraction of a second only when there is one,
/// in groups of 3, 6 or 9 digits: the text logs and the proofs have always shown.
pub fn spaced(at: DateTime) -> String {
    let fraction = fraction(at.subsec_nanosecond());
    format!("{}{fraction}", at.strftime("%Y-%m-%d %H:%M:%S"))
}

/// RFC 3339 text of an instant in UTC with a `+00:00` offset, the fraction of a second as
/// [`spaced`]: `2027-01-10T08:30:00+00:00`, `2027-01-10T08:30:00.120+00:00`.
pub fn rfc3339(t: Timestamp) -> String {
    let at = utc_datetime(t);
    let fraction = fraction(at.subsec_nanosecond());
    format!("{}{fraction}+00:00", at.strftime("%Y-%m-%dT%H:%M:%S"))
}

/// `.NNN`, `.NNNNNN` or `.NNNNNNNNN` (the shortest of the three that is exact), empty for
/// a whole second.
fn fraction(nanos: i32) -> String {
    if nanos == 0 {
        String::new()
    } else if nanos % 1_000_000 == 0 {
        format!(".{:03}", nanos / 1_000_000)
    } else if nanos % 1_000 == 0 {
        format!(".{:06}", nanos / 1_000)
    } else {
        format!(".{nanos:09}")
    }
}

/// The UTC date of an instant.
pub fn utc_date(t: Timestamp) -> Date {
    utc_datetime(t).date()
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;

    #[test]
    fn checked_fields() {
        assert_eq!(ymd(2027, 2, 28), Some(date(2027, 2, 28)));
        assert_eq!(ymd(2027, 2, 29), None);
        assert_eq!(ymd(2028, 2, 29), Some(date(2028, 2, 29)));
        assert_eq!(ymd(2027, 13, 1), None);
        // Would wrap to a valid year with an `as i16` cast.
        assert_eq!(ymd(65_536 + 2027, 1, 1), None);
        assert_eq!(ymd(2027, 256 + 1, 1), None);
        assert_eq!(
            hms(23, 59, 59).map(|t| t.to_string()).as_deref(),
            Some("23:59:59")
        );
        assert_eq!(hms(24, 0, 0), None);
    }

    #[test]
    fn utc_round_trip() {
        let at = date(2027, 3, 28).at(1, 30, 0, 0);
        let t = utc(at).unwrap();
        assert_eq!(t.to_string(), "2027-03-28T01:30:00Z");
        assert_eq!(utc_datetime(t), at);
        assert_eq!(utc_date(t), date(2027, 3, 28));
        assert_eq!(utc(DateTime::MAX), None);
    }

    #[test]
    fn rfc3339_text() {
        let t = utc(date(2027, 1, 10).at(8, 30, 0, 0)).unwrap();
        assert_eq!(rfc3339(t), "2027-01-10T08:30:00+00:00");
        assert_eq!(
            rfc3339(t + jiff::SignedDuration::from_millis(120)),
            "2027-01-10T08:30:00.120+00:00"
        );
        assert_eq!(
            rfc3339(t - jiff::SignedDuration::from_nanos(1)),
            "2027-01-10T08:29:59.999999999+00:00"
        );
        let before_1970 = utc(date(1969, 12, 31).at(23, 59, 59, 500_000_000)).unwrap();
        assert_eq!(rfc3339(before_1970), "1969-12-31T23:59:59.500+00:00");
    }

    #[test]
    fn spaced_text() {
        let at = date(2027, 1, 1).at(13, 0, 0, 0);
        assert_eq!(spaced(at), "2027-01-01 13:00:00");
        assert_eq!(
            spaced(at + jiff::SignedDuration::from_millis(120)),
            "2027-01-01 13:00:00.120"
        );
        assert_eq!(
            spaced(at + jiff::SignedDuration::from_micros(5)),
            "2027-01-01 13:00:00.000005"
        );
        assert_eq!(
            spaced(at + jiff::SignedDuration::from_nanos(7)),
            "2027-01-01 13:00:00.000000007"
        );
    }
}
