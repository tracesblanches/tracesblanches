//! Access titles and their validity zones (H2, #133): which census domains (and so which
//! OSM ways) each title covers, its footprint, and a check of every cited source.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use skimap_common::download::{Downloader, Fetched, Request};
use skimap_osm::census::name_tokens;

use crate::catalogue::{self, Doc, Member, Price, Titre};
use crate::domains::Domain;

/// How a member was matched to census domains.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MemberMatch {
    /// Label in the source.
    pub label: String,
    /// `verifie`, `probable`, `auto`.
    pub confidence: String,
    /// Census domains.
    pub domains: Vec<String>,
}

/// Census domains of a member: the hand-checked list, or the domains of its departments
/// whose name, OpenSkiMap name or commune contains every word of one of its keys.
pub fn match_member(m: &Member, domains: &[Domain]) -> MemberMatch {
    let ids: Vec<String> = if !m.domains.is_empty() || m.confidence != "auto" {
        m.domains.iter().map(|s| (*s).to_owned()).collect()
    } else {
        let keys: Vec<BTreeSet<String>> = m
            .keys
            .iter()
            .map(|k| name_tokens(k))
            .filter(|k| !k.is_empty())
            .collect();
        domains
            .iter()
            .filter(|d| m.depts.contains(&d.departement.as_str()))
            .filter(|d| {
                let mut names: Vec<BTreeSet<String>> =
                    vec![name_tokens(&d.name), name_tokens(&d.commune)];
                names.extend(d.alt_names.iter().map(|a| name_tokens(a)));
                keys.iter().any(|k| names.iter().any(|n| k.is_subset(n)))
            })
            .map(|d| d.id.clone())
            .collect()
    };
    MemberMatch {
        label: m.label.to_owned(),
        confidence: m.confidence.to_owned(),
        domains: ids,
    }
}

/// Coverage of a title.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Coverage {
    /// Title id.
    pub titre: String,
    /// Members and their domains.
    pub members: Vec<MemberMatch>,
    /// Union of the members' domains.
    pub domains: BTreeSet<String>,
}

/// Coverage of every title of the catalogue (the national pass: every association list).
pub fn coverages(domains: &[Domain]) -> Vec<Coverage> {
    let national = catalogue::national_members();
    catalogue::TITRES
        .iter()
        .map(|t| {
            let members: &[Member] = if t.id == "nordic-pass-national" {
                &national
            } else {
                t.members
            };
            let ms: Vec<MemberMatch> = members.iter().map(|m| match_member(m, domains)).collect();
            let set = ms.iter().flat_map(|m| m.domains.iter().cloned()).collect();
            Coverage {
                titre: t.id.to_owned(),
                members: ms,
                domains: set,
            }
        })
        .collect()
}

/// Result of the check of a source document.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DocCheck {
    /// Document id.
    pub id: String,
    /// `ok`, `quotes_missing`, `changed` (PDF hash differs), `unreachable`, `not_checked`
    /// (without `personal_sources`).
    pub status: String,
    /// Quotes not found in the current text.
    pub missing: Vec<String>,
    /// SHA-256 of the current document.
    pub sha256: Option<String>,
    /// Fetch date (RFC 3339).
    pub fetched_at: Option<String>,
    /// Error, if unreachable.
    pub error: Option<String>,
}

/// Plain text of an HTML page: tags, scripts and styles removed, common entities
/// decoded, apostrophes and spaces normalised.
pub fn html_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len() / 2);
    let lower = html.to_ascii_lowercase();
    let mut i = 0;
    let b = html.as_bytes();
    while i < b.len() {
        if b[i] == b'<' {
            let rest = &lower[i..];
            let skip_to = if rest.starts_with("<script") {
                rest.find("</script>").map(|e| e + 9)
            } else if rest.starts_with("<style") {
                rest.find("</style>").map(|e| e + 8)
            } else {
                rest.find('>').map(|e| e + 1)
            };
            i += skip_to.unwrap_or(rest.len());
            out.push(' ');
            continue;
        }
        let next = html[i..].find('<').map_or(html.len(), |e| i + e);
        out.push_str(&html[i..next]);
        i = next;
    }
    normalise(&decode_entities(&out))
}

/// Decodes numeric entities (`&#160;`, `&#x2019;`) and the common named ones.
pub fn decode_entities(s: &str) -> String {
    const NAMED: &[(&str, &str)] = &[
        ("nbsp", " "),
        ("rsquo", "'"),
        ("lsquo", "'"),
        ("quot", "\""),
        ("eacute", "é"),
        ("egrave", "è"),
        ("ecirc", "ê"),
        ("agrave", "à"),
        ("ccedil", "ç"),
        ("euro", "€"),
        ("laquo", "«"),
        ("raquo", "»"),
        ("amp", "&"),
        ("lt", "<"),
        ("gt", ">"),
    ];
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let tail = &rest[at + 1..];
        let end = tail.find(';').filter(|e| *e <= 10);
        let decoded = end.and_then(|e| {
            let name = &tail[..e];
            let c = if let Some(hex) = name.strip_prefix("#x").or_else(|| name.strip_prefix("#X")) {
                u32::from_str_radix(hex, 16)
                    .ok()
                    .and_then(char::from_u32)
                    .map(String::from)
            } else if let Some(dec) = name.strip_prefix('#') {
                dec.parse::<u32>()
                    .ok()
                    .and_then(char::from_u32)
                    .map(String::from)
            } else {
                NAMED
                    .iter()
                    .find(|(n, _)| *n == name)
                    .map(|(_, v)| (*v).to_owned())
            };
            c.map(|c| (c, e))
        });
        match decoded {
            Some((c, e)) => {
                out.push_str(&c);
                rest = &tail[e + 1..];
            }
            None => {
                out.push('&');
                rest = tail;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Normalises apostrophes, quotes and whitespace for quote matching.
pub fn normalise(s: &str) -> String {
    s.replace(['’', 'ʼ'], "'")
        .replace(['\u{a0}', '\u{202f}'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// [`normalise`] without any whitespace: inline tags (`<strong>160€</strong>`) split the
/// text at places the quote does not.
pub fn squash(s: &str) -> String {
    normalise(s)
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect()
}

/// Quotes that a document must still contain: validity texts and price quotes of HTML
/// sources.
pub fn quotes_of(doc: &str) -> Vec<String> {
    let mut q: BTreeSet<String> = BTreeSet::new();
    for t in catalogue::TITRES {
        if t.validity_doc == doc {
            q.insert(t.validity.to_owned());
        }
        for p in t.prices.iter().filter(|p| p.doc == doc) {
            q.insert(p.quote.to_owned());
        }
    }
    for s in catalogue::SITE_PRICES {
        for p in s.prices.iter().filter(|p| p.doc == doc) {
            q.insert(p.quote.to_owned());
        }
    }
    for r in catalogue::SITE_RULES.iter().filter(|r| r.doc == doc) {
        // Rules are quoted sentence by sentence.
        for s in r
            .rule
            .split(". ")
            .filter(|s| s.len() > 20 && !s.contains('«'))
        {
            q.insert(s.trim_end_matches('.').to_owned());
        }
    }
    q.into_iter().collect()
}

/// Checks one document against its fetched body.
pub fn check_body(doc: &Doc, body: &[u8], fetched: &Fetched) -> DocCheck {
    let sha = hex::encode(Sha256::digest(body));
    let (status, missing) = if doc.format == "pdf" {
        let same = doc.sha256 == Some(sha.as_str());
        (if same { "ok" } else { "changed" }, Vec::new())
    } else {
        let text = squash(&html_text(&String::from_utf8_lossy(body)));
        let missing: Vec<String> = quotes_of(doc.id)
            .into_iter()
            .filter(|q| !text.contains(&squash(q)))
            .collect();
        (
            if missing.is_empty() {
                "ok"
            } else {
                "quotes_missing"
            },
            missing,
        )
    };
    DocCheck {
        id: doc.id.to_owned(),
        status: status.to_owned(),
        missing,
        sha256: Some(sha),
        fetched_at: skimap_common::meta::unix_to_rfc3339(fetched.meta.fetched_at_unix as i64),
        error: None,
    }
}

/// Re-reads every source document (personal use only: none displays a reuse licence).
pub fn check_sources(dl: &Downloader) -> (Vec<DocCheck>, Vec<(&'static Doc, Fetched)>) {
    let mut out = Vec::new();
    let mut fetched = Vec::new();
    for d in catalogue::DOCS {
        match dl
            .fetch(&Request::get(d.source, d.url))
            .and_then(|f| f.bytes().map(|b| (f, b)))
        {
            Ok((f, b)) => {
                out.push(check_body(d, &b, &f));
                fetched.push((d, f));
            }
            Err(e) => out.push(DocCheck {
                id: d.id.to_owned(),
                status: "unreachable".to_owned(),
                missing: Vec::new(),
                sha256: None,
                fetched_at: None,
                error: Some(e.to_string()),
            }),
        }
    }
    (out, fetched)
}

fn doc(id: &str) -> Option<&'static Doc> {
    catalogue::DOCS.iter().find(|d| d.id == id)
}

/// A price as written in `titres.json`: dated, sourced, checked.
pub fn price_json(p: &Price, checks: &BTreeMap<String, DocCheck>) -> Value {
    let d = doc(p.doc);
    let check = checks.get(p.doc);
    let confirmed = check.map(|c| {
        c.status == "ok"
            || (c.status == "quotes_missing" && !c.missing.iter().any(|m| m == p.quote))
    });
    json!({
        "product": p.product, "category": p.category, "ages": p.ages, "eur": p.eur,
        "tier": p.tier, "from": p.from, "until": p.until, "season": p.season,
        "source": d.map(|d| d.url), "publisher": d.map(|d| d.publisher),
        "read_on": d.map(|d| d.read_on), "quote": p.quote,
        "confirmed_now": confirmed,
    })
}

/// Validity rule of a title as JSON (for `titres.json`).
pub fn titre_json(
    t: &Titre,
    cov: &Coverage,
    ways: usize,
    checks: &BTreeMap<String, DocCheck>,
) -> Value {
    let unmatched: Vec<&str> = cov
        .members
        .iter()
        .filter(|m| m.domains.is_empty())
        .map(|m| m.label.as_str())
        .collect();
    json!({
        "id": t.id, "name": t.name, "scope": t.scope, "issuer": t.issuer, "products": t.products,
        "validity": t.validity, "validity_source": doc(t.validity_doc).map(|d| d.url),
        "notes": t.notes, "abroad": t.abroad,
        "prices": t.prices.iter().map(|p| price_json(p, checks)).collect::<Vec<_>>(),
        "members": cov.members, "members_without_domain": unmatched,
        "domains": cov.domains, "ways": ways,
        "personal_use_only": true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::parse;

    fn domains() -> Vec<Domain> {
        let sq = |x: f64| {
            json!([[[
                [x, 46.0],
                [x + 0.01, 46.0],
                [x + 0.01, 46.01],
                [x, 46.01],
                [x, 46.0]
            ]]])
        };
        let f = |id: &str, name: &str, commune: &str, dep: &str, x: f64| {
            json!({"geometry": {"type": "MultiPolygon", "coordinates": sq(x)},
            "properties": {"id": id, "name": name, "alt_names": [], "lon": x, "lat": 46.0,
            "commune": {"name": commune, "insee": "0", "departement": dep}}})
        };
        parse(&json!({"features": [
            f("a", "Plateau des Glières", "Glières-Val-de-Borne", "74", 6.0),
            f("b", "Le Grand-Bornand", "Le Grand-Bornand", "74", 6.1),
            f("c", "La Chapelle-Rambaud", "La Chapelle-Rambaud", "74", 6.2),
            f("d", "Les Portes du Soleil", "La Chapelle-d'Abondance", "74", 6.3),
            f("e", "Glières", "Somewhere", "25", 6.4),
        ]}))
        .unwrap()
    }

    #[test]
    fn automatic_matching() {
        let ds = domains();
        let ids = |m: &Member| match_member(m, &ds).domains;
        assert_eq!(
            ids(&catalogue::HSN_MEMBERS[16]),
            vec!["a"],
            "{}",
            catalogue::HSN_MEMBERS[16].label
        );
        // Words of the key must all be there: « chapelle abondance » is not « Chapelle-Rambaud ».
        let abondance = catalogue::HSN_MEMBERS
            .iter()
            .find(|m| m.label.contains("Abondance"))
            .unwrap();
        assert_eq!(ids(abondance), vec!["d"]);
        let rambaud = catalogue::HSN_MEMBERS
            .iter()
            .find(|m| m.label.contains("Rambaud"))
            .unwrap();
        assert_eq!(ids(rambaud), vec!["c"]);
        // Department filter: the Doubs « Glières » is not Haute-Savoie's.
        assert!(!ids(&catalogue::HSN_MEMBERS[16]).contains(&"e".to_owned()));
    }

    #[test]
    fn enj_site_titles_point_at_their_group() {
        let expect = [
            (
                "enj-site-lacs-montagnes-haut-doubs",
                "Lacs et Montagnes du Haut-Doubs",
            ),
            ("enj-site-grand-pontarlier", "Grand Pontarlier"),
            ("enj-site-station-des-rousses", "Station des Rousses"),
            ("enj-site-hautes-combes", "Hautes Combes du Jura"),
            ("enj-site-bellefontaine", "Bellefontaine"),
            ("enj-site-monts-jura", "Station Monts Jura"),
        ];
        for (id, label) in expect {
            let t = catalogue::TITRES.iter().find(|t| t.id == id).unwrap();
            assert_eq!(t.members.len(), 1);
            assert!(
                t.members[0].label.contains(label),
                "{id}: {}",
                t.members[0].label
            );
        }
    }

    #[test]
    fn every_price_is_dated_and_sourced() {
        for t in catalogue::TITRES {
            assert!(doc(t.validity_doc).is_some(), "{}", t.id);
            for p in t.prices {
                assert!(
                    !p.season.is_empty() && doc(p.doc).is_some(),
                    "{} {:?}",
                    t.id,
                    p
                );
                assert!(p.eur > 0.0);
            }
        }
        for s in catalogue::SITE_PRICES {
            for p in s.prices {
                assert!(p.season.len() == 9 && doc(p.doc).is_some(), "{}", s.label);
            }
        }
        // Every hand-checked domain id has the census form.
        for m in catalogue::national_members() {
            for d in m.domains {
                assert!(d.starts_with("site-") || d.starts_with("net-"), "{d}");
            }
        }
    }

    #[test]
    fn html_and_quotes() {
        let t = html_text(
            "<p>Promo du 1er octobre au 15&nbsp;novembre = 214 €</p><script>x=1</script><b>l’hiver</b> 143 €&#160;/&#x2019;",
        );
        assert!(t.ends_with("143 € /'"), "{t}");
        assert!(
            t.contains(&normalise("Promo du 1er octobre au 15 novembre = 214 €")),
            "{t}"
        );
        assert!(t.contains("l'hiver") && !t.contains("x=1"));
        let q = quotes_of("nf-pass-national");
        assert!(
            q.iter()
                .any(|x| x == "Plein tarif à partir du 16 novembre = 255 €")
        );
        assert!(q.iter().any(|x| x.starts_with("Nordic France propose")));
    }
}
