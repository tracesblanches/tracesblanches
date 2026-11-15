//! Small helpers: random tokens, Base64, URL encoding, French local time, e-mail checks.

use std::io::Read;

use chrono::{DateTime, Datelike, Duration, NaiveDate, TimeZone, Utc, Weekday};

/// `n` random bytes from the kernel (`/dev/urandom`), lowercase hex.
pub fn random_hex(n: usize) -> std::io::Result<String> {
    let mut buf = vec![0u8; n];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut buf)?;
    Ok(hex::encode(buf))
}

/// SHA-256 of a string, lowercase hex.
pub fn sha256_hex(s: &str) -> String {
    skimap_pack::update::sha256_hex(s.as_bytes())
}

/// Constant-time comparison of two strings of the same length.
pub fn constant_time_eq(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard Base64 with padding (RFC 4648).
pub fn base64(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(B64[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// `application/x-www-form-urlencoded` encoding of one value.
pub fn form_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// A form body from pairs.
pub fn form(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{}={}", form_encode(k), form_encode(v)))
        .collect::<Vec<_>>()
        .join("&")
}

/// Decodes `%XX` and `+` of a query-string component; `None` if malformed.
pub fn url_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' => {
                let h = s.get(i + 1..i + 3)?;
                out.push(u8::from_str_radix(h, 16).ok()?);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

/// Query-string pairs, decoded (malformed pairs are dropped).
pub fn parse_query(q: &str) -> Vec<(String, String)> {
    q.split('&')
        .filter(|p| !p.is_empty())
        .filter_map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            Some((url_decode(k)?, url_decode(v)?))
        })
        .collect()
}

fn last_sunday(year: i32, month: u32) -> NaiveDate {
    let next = if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1)
    }
    .expect("valid date");
    let mut d = next - Duration::days(1);
    while d.weekday() != Weekday::Sun {
        d -= Duration::days(1);
    }
    d
}

/// UTC offset of France (Europe/Paris) at `t`: +2 h from the last Sunday of March 01:00
/// UTC to the last Sunday of October 01:00 UTC (EU rule since 1996), else +1 h.
pub fn paris_offset_hours(t: DateTime<Utc>) -> i64 {
    let y = t.year();
    let start = Utc.from_utc_datetime(&last_sunday(y, 3).and_hms_opt(1, 0, 0).expect("time"));
    let end = Utc.from_utc_datetime(&last_sunday(y, 10).and_hms_opt(1, 0, 0).expect("time"));
    if t >= start && t < end { 2 } else { 1 }
}

/// `t` in French local time, e.g. `dimanche 15/11/2026 à 17:30 (heure de Paris)`.
pub fn paris_time(t: DateTime<Utc>) -> String {
    let local = t + Duration::hours(paris_offset_hours(t));
    let day = match local.weekday() {
        Weekday::Mon => "lundi",
        Weekday::Tue => "mardi",
        Weekday::Wed => "mercredi",
        Weekday::Thu => "jeudi",
        Weekday::Fri => "vendredi",
        Weekday::Sat => "samedi",
        Weekday::Sun => "dimanche",
    };
    format!(
        "{day} {} à {} (heure de Paris)",
        local.format("%d/%m/%Y"),
        local.format("%H:%M")
    )
}

/// A plausible single e-mail address: one `@`, a dot in the domain, no space, no control
/// character, nothing that could break a mail header, 254 bytes at most.
pub fn valid_email(s: &str) -> bool {
    let Some((local, domain)) = s.split_once('@') else {
        return false;
    };
    s.len() <= 254
        && !local.is_empty()
        && local.len() <= 64
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !domain.contains('@')
        && s.chars()
            .all(|c| !c.is_control() && !c.is_whitespace() && !"<>()[],;:\"\\".contains(c))
}

/// Text without control characters except new lines and tabs, trimmed.
pub fn clean_text(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect::<String>()
        .trim()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_rfc4648_vectors() {
        for (i, o) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(i.as_bytes()), o);
        }
        // Strava's example: client_id=12345, client_secret=abc123.
        assert_eq!(base64(b"12345:abc123"), "MTIzNDU6YWJjMTIz");
    }

    #[test]
    fn form_and_query() {
        assert_eq!(form(&[("a b", "x&y=z/é")]), "a+b=x%26y%3Dz%2F%C3%A9");
        assert_eq!(
            parse_query("code=ab%2Fc&scope=read,activity%3Awrite&x"),
            [
                ("code".into(), "ab/c".into()),
                ("scope".into(), "read,activity:write".into()),
                ("x".into(), "".into())
            ]
        );
        assert_eq!(url_decode("%zz"), None);
    }

    #[test]
    fn paris_time_follows_dst() {
        let t = |s: &str| DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc);
        // 2026: summer time from 29/03 01:00 UTC to 25/10 01:00 UTC.
        assert_eq!(paris_offset_hours(t("2026-03-29T00:59:59Z")), 1);
        assert_eq!(paris_offset_hours(t("2026-03-29T01:00:00Z")), 2);
        assert_eq!(paris_offset_hours(t("2026-10-25T00:59:59Z")), 2);
        assert_eq!(paris_offset_hours(t("2026-10-25T01:00:00Z")), 1);
        assert_eq!(
            paris_time(t("2026-11-15T16:30:00Z")),
            "dimanche 15/11/2026 à 17:30 (heure de Paris)"
        );
        assert_eq!(
            paris_time(t("2027-07-14T10:05:00Z")),
            "mercredi 14/07/2027 à 12:05 (heure de Paris)"
        );
    }

    #[test]
    fn emails() {
        assert!(valid_email("marie.dupont+ski@example.fr"));
        for bad in [
            "",
            "a",
            "a@b",
            "a@@b.fr",
            "a b@c.fr",
            "a@c.fr\r\nBcc: x@y.z",
            "<a@b.fr>",
            "a@.fr",
            "a@b.",
        ] {
            assert!(!valid_email(bad), "{bad:?}");
        }
    }

    #[test]
    fn tokens() {
        let a = random_hex(16).unwrap();
        assert_eq!(a.len(), 32);
        assert_ne!(a, random_hex(16).unwrap());
        assert!(constant_time_eq(&a, &a.clone()));
        assert!(!constant_time_eq(&a, &random_hex(16).unwrap()));
        assert_eq!(clean_text("  a\u{7}b\nc\t "), "ab\nc");
    }
}
