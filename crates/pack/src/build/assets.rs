//! Style resources of the basemaps: sprites and glyphs, fetched once through the shared
//! downloader and stored next to the style so the map renders offline.

use std::collections::BTreeSet;
use std::path::Path;

use serde_json::Value;
use skimap_common::download::{Downloader, Request};

use crate::build::write_bytes;
use crate::error::Result;

/// Glyph ranges stored per font: Basic Latin + Latin-1 (é, à, ç), Latin Extended-A (œ),
/// General Punctuation (’ « » – …). Other ranges are not stored: MapLibre skips the
/// characters it cannot find (none expected in French, Italian and Swiss place names).
pub const GLYPH_RANGES: [&str; 3] = ["0-255", "256-511", "8192-8447"];

/// Font stacks used by a style (`text-font` values, literal arrays, zoom stops,
/// `case` / `match` expressions).
pub fn fontstacks(style: &Value) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    if let Some(layers) = style.get("layers").and_then(Value::as_array) {
        for l in layers {
            if let Some(tf) = l.pointer("/layout/text-font") {
                collect_font_arrays(tf, &mut out);
            }
        }
    }
    out
}

fn collect_font_arrays(v: &Value, out: &mut BTreeSet<String>) {
    match v {
        Value::Array(a) => {
            let strings: Option<Vec<&str>> = a.iter().map(Value::as_str).collect();
            match strings {
                // A font list, unless it is an expression (first item is an operator).
                Some(s) if !s.is_empty() && !is_operator(s[0]) => {
                    out.insert(s.join(","));
                }
                _ => a.iter().for_each(|x| collect_font_arrays(x, out)),
            }
        }
        Value::Object(o) => o.values().for_each(|x| collect_font_arrays(x, out)),
        _ => {}
    }
}

fn is_operator(s: &str) -> bool {
    matches!(
        s,
        "literal"
            | "case"
            | "match"
            | "step"
            | "interpolate"
            | "coalesce"
            | "get"
            | "=="
            | "!="
            | "<="
            | ">="
            | "<"
            | ">"
            | "zoom"
            | "concat"
            | "length"
            | "all"
            | "any"
            | "has"
    )
}

/// Percent-encodes a font stack for a URL path segment.
pub fn encode_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Fetches the glyph ranges of every font stack into `dir/<fontstack>/<range>.pbf`.
/// `url_template` contains `{fontstack}` and `{range}`. Returns the bytes written.
pub fn fetch_glyphs(
    dl: &Downloader,
    source: &str,
    url_template: &str,
    stacks: &BTreeSet<String>,
    dir: &Path,
) -> Result<u64> {
    let mut n = 0;
    for stack in stacks {
        for range in GLYPH_RANGES {
            let url = url_template
                .replace("{fontstack}", &encode_segment(stack))
                .replace("{range}", range);
            let body = dl.fetch(&Request::get(source, &url))?.bytes()?;
            n += write_bytes(&dir.join(stack).join(format!("{range}.pbf")), &body)?;
        }
    }
    Ok(n)
}

/// Fetches a MapLibre sprite (`.json`, `.png`, `@2x.json`, `@2x.png`) from `base_url`
/// into `dir/<name>...`. Returns the bytes written.
pub fn fetch_sprite(
    dl: &Downloader,
    source: &str,
    base_url: &str,
    dir: &Path,
    name: &str,
) -> Result<u64> {
    let mut n = 0;
    for suffix in [".json", ".png", "@2x.json", "@2x.png"] {
        let body = dl
            .fetch(&Request::get(source, &format!("{base_url}{suffix}")))?
            .bytes()?;
        n += write_bytes(&dir.join(format!("{name}{suffix}")), &body)?;
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn finds_font_stacks_in_expressions() {
        let style = json!({"layers": [
            {"layout": {"text-font": ["Source Sans Pro Bold"]}},
            {"layout": {"text-font": {"stops": [[1, ["A Regular"]], [7, ["A Bold", "B Bold"]]]}}},
            {"layout": {"text-font": ["case", ["<=", ["get", "min_zoom"], 5],
                ["literal", ["Noto Sans Medium"]], ["literal", ["Noto Sans Regular"]]]}},
            {"layout": {}}
        ]});
        let s: Vec<String> = fontstacks(&style).into_iter().collect();
        assert_eq!(
            s,
            vec![
                "A Bold,B Bold",
                "A Regular",
                "Noto Sans Medium",
                "Noto Sans Regular",
                "Source Sans Pro Bold"
            ]
        );
    }

    #[test]
    fn encodes_font_names() {
        assert_eq!(encode_segment("Noto Sans Regular"), "Noto%20Sans%20Regular");
        assert_eq!(encode_segment("A,B"), "A%2CB");
    }
}
