//! Small XML writing helpers (GPX, KML).

/// Escapes text for XML content and attribute values.
pub fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            // Characters not allowed in XML 1.0.
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

/// Escapes text for an HTML description (then XML-escaped by the caller): line
/// breaks become `<br/>`.
pub fn html_lines(lines: &[String]) -> String {
    lines
        .iter()
        .map(|l| {
            let mut s = String::new();
            for c in l.chars() {
                match c {
                    '&' => s.push_str("&amp;"),
                    '<' => s.push_str("&lt;"),
                    '>' => s.push_str("&gt;"),
                    '\r' => {}
                    '\n' => s.push_str("<br/>"),
                    c => s.push(c),
                }
            }
            s
        })
        .collect::<Vec<_>>()
        .join("<br/>")
}

/// Formats a coordinate with 6 decimals (≈ 0.1 m), without trailing zeros.
pub fn coord(v: f64) -> String {
    let s = format!("{v:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" {
        "0".to_owned()
    } else {
        s.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escaping() {
        assert_eq!(
            esc("a<b & \"c\" 'd'>"),
            "a&lt;b &amp; &quot;c&quot; &apos;d&apos;&gt;"
        );
        assert_eq!(esc("x\u{1}y"), "x y");
        assert_eq!(
            html_lines(&["a<b".into(), "c\nd".into()]),
            "a&lt;b<br/>c<br/>d"
        );
        assert_eq!(coord(6.1234567), "6.123457");
        assert_eq!(coord(44.5), "44.5");
        assert_eq!(coord(-0.0000001), "0");
    }
}
