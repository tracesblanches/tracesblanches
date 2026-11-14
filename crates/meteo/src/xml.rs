//! Minimal XML tree on top of `quick-xml`, enough for the BERA bulletin.
//!
//! Entity and character references are resolved; text of nested elements (the `<a>` links
//! inside `TexteMeta`) stays in the child and is reachable with [`Element::full_text`].

use quick_xml::XmlVersion;
use quick_xml::events::{BytesStart, Event};
use quick_xml::reader::Reader;

/// An element with its attributes, children and direct text.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Element {
    /// Local name.
    pub name: String,
    /// Attributes in document order, values unescaped.
    pub attrs: Vec<(String, String)>,
    /// Child elements in document order.
    pub children: Vec<Element>,
    /// Direct text content (concatenated, references resolved).
    pub text: String,
}

impl Element {
    /// Attribute value.
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    /// First child with this name.
    pub fn child(&self, name: &str) -> Option<&Element> {
        self.children.iter().find(|c| c.name == name)
    }

    /// Children with this name.
    pub fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Element> {
        self.children.iter().filter(move |c| c.name == name)
    }

    /// Text of this element and all descendants, in document order of the children
    /// appended after the direct text (sufficient for the short mixed-content blocks of
    /// the BERA).
    pub fn full_text(&self) -> String {
        let mut s = self.text.clone();
        for c in &self.children {
            s.push_str(&c.full_text());
        }
        s
    }
}

fn start_element(e: &BytesStart<'_>) -> Result<Element, String> {
    let name = e.local_name().as_ref().to_owned();
    let mut attrs = Vec::new();
    for a in e.attributes() {
        let a = a.map_err(|err| err.to_string())?;
        let key = a.key.local_name().as_ref().to_owned();
        let value = a
            .normalized_value(XmlVersion::Implicit1_0)
            .map_err(|err| err.to_string())?
            .into_owned();
        attrs.push((key, value));
    }
    Ok(Element {
        name,
        attrs,
        ..Element::default()
    })
}

/// Parses a document and returns its root element.
pub fn parse(xml: &str) -> Result<Element, String> {
    let mut reader = Reader::from_str(xml);
    let mut stack: Vec<Element> = Vec::new();
    let mut root: Option<Element> = None;
    loop {
        let ev = reader
            .read_event()
            .map_err(|e| format!("at byte {}: {e}", reader.buffer_position()))?;
        match ev {
            Event::Start(e) => stack.push(start_element(&e)?),
            Event::Empty(e) => {
                let el = start_element(&e)?;
                match stack.last_mut() {
                    Some(parent) => parent.children.push(el),
                    None => root = Some(el),
                }
            }
            Event::End(_) => {
                let el = stack.pop().ok_or("unbalanced end tag")?;
                match stack.last_mut() {
                    Some(parent) => parent.children.push(el),
                    None => root = Some(el),
                }
            }
            Event::Text(t) => {
                if let Some(cur) = stack.last_mut() {
                    cur.text.push_str(&t.xml10_content());
                }
            }
            Event::CData(t) => {
                if let Some(cur) = stack.last_mut() {
                    cur.text.push_str(&t.into_inner());
                }
            }
            Event::GeneralRef(r) => {
                let resolved: String = if r.is_char_ref() {
                    match r.resolve_char_ref().map_err(|e| e.to_string())? {
                        Some(c) => c.to_string(),
                        None => String::new(),
                    }
                } else {
                    let name: &str = r.as_ref();
                    match quick_xml::escape::resolve_predefined_entity(name) {
                        Some(s) => s.to_owned(),
                        None => return Err(format!("unknown entity &{name};")),
                    }
                };
                if let Some(cur) = stack.last_mut() {
                    cur.text.push_str(&resolved);
                }
            }
            Event::Eof => break,
            Event::Comment(_) | Event::Decl(_) | Event::PI(_) | Event::DocType(_) => {}
        }
    }
    if !stack.is_empty() {
        return Err(format!("unclosed element <{}>", stack[0].name));
    }
    root.ok_or_else(|| "empty document".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree_entities_and_mixed_content() {
        let x = r#"<?xml version="1.0"?><R a="&lt;2100" b="x &amp; y"><T>coul&#233;es &gt; 5</T>
            <M>see <a href="u">u</a></M><E/></R>"#;
        let r = parse(x).unwrap();
        assert_eq!(r.name, "R");
        assert_eq!(r.attr("a"), Some("<2100"));
        assert_eq!(r.attr("b"), Some("x & y"));
        assert_eq!(r.child("T").unwrap().text, "coulées > 5");
        assert_eq!(r.child("M").unwrap().full_text(), "see u");
        assert!(r.child("E").is_some());
        assert!(parse("<a><b></a>").is_err());
        assert!(parse("").is_err());
    }
}
