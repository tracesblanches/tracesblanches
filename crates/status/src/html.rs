//! Small helpers to read the few fixed HTML pages of the trail status sources (no HTML
//! parser dependency: the pages are server-rendered templates with stable class names).

/// Decodes the character references found in the pages (`&#039;`, `&#8217;`, `&amp;`,
/// `&nbsp;`, `&eacute;`...). Unknown named references are kept as is.
pub fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        let Some(end) = tail[1..].find(';').map(|e| e + 1) else {
            out.push_str(tail);
            return out;
        };
        let name = &tail[1..end];
        let decoded = if let Some(num) = name.strip_prefix('#') {
            let code = match num.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok(),
                None => num.parse().ok(),
            };
            code.and_then(char::from_u32).map(String::from)
        } else {
            match name {
                "amp" => Some("&".into()),
                "lt" => Some("<".into()),
                "gt" => Some(">".into()),
                "quot" => Some("\"".into()),
                "apos" => Some("'".into()),
                "nbsp" => Some("\u{a0}".into()),
                "eacute" => Some("é".into()),
                "egrave" => Some("è".into()),
                "agrave" => Some("à".into()),
                "ccedil" => Some("ç".into()),
                "rsquo" => Some("’".into()),
                _ => None,
            }
        };
        match decoded {
            Some(d) if end <= 12 => {
                out.push_str(&d);
                rest = &tail[end + 1..];
            }
            _ => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Removes tags (`<br>` becomes a space), decodes references and collapses white space
/// (non-breaking spaces included).
pub fn text(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => {
                in_tag = true;
                out.push(' ');
            }
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    decode_entities(&out)
        .split(|c: char| c.is_whitespace() || c == '\u{a0}')
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// The part of `hay` after the first `marker`.
pub fn after<'a>(hay: &'a str, marker: &str) -> Option<&'a str> {
    hay.find(marker).map(|i| &hay[i + marker.len()..])
}

/// The part of `hay` before the first `end` (all of it if absent).
pub fn before<'a>(hay: &'a str, end: &str) -> &'a str {
    hay.find(end).map_or(hay, |i| &hay[..i])
}

/// Text content of the element whose opening tag contains `marker` (e.g.
/// `class="StationWidget-title"`), up to the next closing tag `</tag>`.
pub fn element_text(hay: &str, marker: &str, tag: &str) -> Option<String> {
    let s = after(hay, marker)?;
    let s = after(s, ">")?;
    let t = text(before(s, &format!("</{tag}>")));
    (!t.is_empty()).then_some(t)
}

/// Value of attribute `attr` in the first tag after `marker` (`marker` may be part of
/// the same tag).
pub fn attr_after(hay: &str, marker: &str, attr: &str) -> Option<String> {
    let s = after(hay, marker)?;
    let s = after(before(s, ">"), &format!("{attr}=\""))?;
    Some(decode_entities(before(s, "\"")).trim().to_owned())
}

/// Splits `hay` into the chunks that start at each occurrence of `marker` (the text
/// before the first occurrence is dropped).
pub fn chunks<'a>(hay: &'a str, marker: &str) -> Vec<&'a str> {
    let idx: Vec<usize> = hay.match_indices(marker).map(|(i, _)| i).collect();
    idx.iter()
        .enumerate()
        .map(|(k, &i)| &hay[i..idx.get(k + 1).copied().unwrap_or(hay.len())])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entities_and_text() {
        assert_eq!(
            decode_entities("L&#039;Izoard &amp; L&#8217;Aigalan"),
            "L'Izoard & L’Aigalan"
        );
        assert_eq!(
            decode_entities("a & b &unknown; &#x41;"),
            "a & b &unknown; A"
        );
        assert_eq!(
            text("<p><strong>Izoard<br />\n16km</strong>&nbsp;</p>"),
            "Izoard 16km"
        );
        let h = r#"<h1 class="T-title">Cervi&egrave;res</h1><div class="I" data-comment="x &quot;y&quot;">"#;
        assert_eq!(
            element_text(h, "class=\"T-title\"", "h1").as_deref(),
            Some("Cervières")
        );
        assert_eq!(
            attr_after(h, "class=\"I\"", "data-comment").as_deref(),
            Some("x \"y\"")
        );
        assert_eq!(chunks("a<x>1<x>2", "<x>"), ["<x>1", "<x>2"]);
    }
}
