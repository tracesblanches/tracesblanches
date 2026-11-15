//! Access titles and their validity zones (H2, #133): titles read on the issuers' pages
//! ([`crate::sources`]), matched to the census domains (and so to their OSM ways).

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use sha2::{Digest, Sha256};
use skimap_common::download::{Downloader, Fetched, Request};
use skimap_osm::census::name_tokens;

use crate::catalogue::{self, Doc};
use crate::domains::Domain;
use crate::sources::{self, EnjFaq, MemberSpec, Price, SitePage, SiteRule, Titre, fold};

/// How a member was matched to census domains.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MemberMatch {
    /// Label in the source.
    pub label: String,
    /// `auto` (name in its departments), `auto_national` (unique name in France),
    /// or the confidence of a hand-made complement (`verifie`, `partiel`, `probable`),
    /// `aucun` when nothing matched.
    pub confidence: String,
    /// Census domains.
    pub domains: Vec<String>,
    /// Reason of the hand-made complement, if used.
    pub complement: Option<String>,
}

/// Words that name a kind of place, not a place: a key made only of them (« La Combe »,
/// « Le Glacier », « Les Marais ») would match any domain that has one.
pub const PLACE_WORDS: &[&str] = &[
    "combe", "marai", "glacier", "foret", "bois", "pre", "prairie", "crete", "fontaine", "source",
    "chapelle", "village", "lac", "plan", "pla", "roche", "rocher", "cret", "pont", "vallon",
    "seigne", "chalet", "refuge", "gare", "station", "centre", "noire", "blanche",
];

/// Words of a name for matching: [`name_tokens`] without a plural « s » (« Aillons » and
/// « Aillon »).
fn words(s: &str) -> BTreeSet<String> {
    name_tokens(s)
        .into_iter()
        .map(|t| {
            if t.len() > 4 && t.ends_with('s') {
                t[..t.len() - 1].to_owned()
            } else {
                t
            }
        })
        .collect()
}

/// Names of a domain: its name, its commune, its OpenSkiMap names, and name + commune
/// together (« Galibier-Thabor » at Valloire for « Valloire Galibier »).
fn domain_names(d: &Domain) -> Vec<BTreeSet<String>> {
    let mut names = vec![words(&d.name), words(&d.commune)];
    names.push(names[0].union(&names[1]).cloned().collect());
    names.extend(d.alt_names.iter().map(|a| words(a)));
    names
}

/// A key matches a domain when every word of the key is in one of its names, or (with
/// `reverse`) when every word of the domain's own name, of 4 letters or more, is in the
/// key (« Sur Lyand Grand Colombier » for the domain « Sur Lyand »).
fn key_matches(keys: &[BTreeSet<String>], d: &Domain, reverse: bool) -> bool {
    let names = domain_names(d);
    let own = words(&d.name);
    let own_ok = reverse && !own.is_empty() && own.iter().all(|t| t.len() >= 4);
    keys.iter()
        .any(|k| names.iter().any(|n| k.is_subset(n)) || (own_ok && own.is_subset(k)))
}

/// Census domains of a member: domains of its departments whose name, OpenSkiMap name
/// or commune contains every word of one of its keys; else the only domain of France that
/// does; plus the hand-checked complements of [`catalogue::OVERRIDES`] still valid on
/// `today`.
pub fn match_member(m: &MemberSpec, domains: &[Domain], today: &str) -> MemberMatch {
    let keys: Vec<BTreeSet<String>> = m
        .keys
        .iter()
        .map(|k| words(k))
        .filter(|k| !k.is_empty() && !k.iter().all(|w| PLACE_WORDS.contains(&w.as_str())))
        .collect();
    let mut ids: Vec<String> = domains
        .iter()
        .filter(|d| m.depts.contains(&d.departement))
        .filter(|d| key_matches(&keys, d, true))
        .map(|d| d.id.clone())
        .collect();
    let mut confidence = if ids.is_empty() { "aucun" } else { "auto" }.to_owned();
    if ids.is_empty() {
        // Outside its departments: only a full name match, and only if unique.
        let all: Vec<&Domain> = domains
            .iter()
            .filter(|d| key_matches(&keys, d, false))
            .collect();
        if all.len() == 1 {
            ids.push(all[0].id.clone());
            confidence = "auto_national".to_owned();
        }
    }
    let label = fold(&m.label);
    let mut complement = None;
    for o in catalogue::OVERRIDES {
        if label.contains(o.label_contains) && catalogue::usable(o.expires, today) {
            for d in o.domains {
                if !ids.iter().any(|x| x == d) {
                    ids.push((*d).to_owned());
                }
            }
            confidence = o.confidence.to_owned();
            complement = Some(format!(
                "{} (vérifié le {}, expire le {})",
                o.reason, o.checked_on, o.expires
            ));
        }
    }
    MemberMatch {
        label: m.label.clone(),
        confidence,
        domains: ids,
        complement,
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

/// Coverage of a title.
pub fn coverage(t: &Titre, domains: &[Domain], today: &str) -> Coverage {
    let members: Vec<MemberMatch> = t
        .members
        .iter()
        .map(|m| match_member(m, domains, today))
        .collect();
    let set = members
        .iter()
        .flat_map(|m| m.domains.iter().cloned())
        .collect();
    Coverage {
        titre: t.id.clone(),
        members,
        domains: set,
    }
}

/// State of a source document after a read.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DocStatus {
    /// Document id (or URL of a domain page).
    pub id: String,
    /// URL.
    pub url: String,
    /// `ok`, `unreachable` (download failed), `broken` (the page no longer has what the
    /// parser expects), `changed` (pinned PDF no longer the same).
    pub status: String,
    /// What went wrong.
    pub error: Option<String>,
    /// SHA-256 of the body read.
    pub sha256: Option<String>,
    /// Download date of the body (RFC 3339).
    pub fetched_at: Option<String>,
}

/// What [`read_titles`] read.
#[derive(Debug, Clone, Default)]
pub struct TitresRead {
    /// Titles built from documents read correctly.
    pub titres: Vec<Titre>,
    /// Site rules.
    pub site_rules: Vec<SiteRule>,
    /// Domain pages (day prices, contacts).
    pub site_pages: Vec<SitePage>,
    /// State of every document.
    pub docs: Vec<DocStatus>,
    /// Documents read, for the metadata.
    pub fetched: Vec<(String, Fetched)>,
    /// Titles that could not be built, with the documents at fault.
    pub missing: Vec<(String, String)>,
}

struct Reader<'a> {
    dl: &'a Downloader,
    out: TitresRead,
}

impl Reader<'_> {
    /// Fetches a document; records its state; returns its body.
    fn body(&mut self, id: &str, source: &str, url: &str) -> Option<Vec<u8>> {
        match self
            .dl
            .fetch(&Request::get(source, url))
            .and_then(|f| f.bytes().map(|b| (f, b)))
        {
            Ok((f, b)) => {
                self.out.docs.push(DocStatus {
                    id: id.to_owned(),
                    url: url.to_owned(),
                    status: "ok".into(),
                    error: None,
                    sha256: Some(hex::encode(Sha256::digest(&b))),
                    fetched_at: skimap_common::meta::unix_to_rfc3339(f.meta.fetched_at_unix as i64),
                });
                self.out.fetched.push((id.to_owned(), f));
                Some(b)
            }
            Err(e) => {
                self.out.docs.push(DocStatus {
                    id: id.to_owned(),
                    url: url.to_owned(),
                    status: "unreachable".into(),
                    error: Some(e.to_string()),
                    sha256: None,
                    fetched_at: None,
                });
                None
            }
        }
    }

    fn lines(&mut self, doc: &Doc) -> Option<Vec<String>> {
        self.body(doc.id, doc.source, doc.url)
            .map(|b| sources::html_lines(&String::from_utf8_lossy(&b)))
    }

    /// Records a parse error as a broken source.
    fn broken(&mut self, id: &str, e: String) {
        if let Some(d) = self.out.docs.iter_mut().rev().find(|d| d.id == id) {
            d.status = "broken".into();
            d.error = Some(e);
        }
    }

    fn parse<T>(&mut self, id: &str, r: sources::ParseResult<T>) -> Option<T> {
        match r {
            Ok(t) => Some(t),
            Err(e) => {
                self.broken(id, e);
                None
            }
        }
    }
}

fn doc(id: &str) -> &'static Doc {
    catalogue::DOCS
        .iter()
        .find(|d| d.id == id)
        .expect("document of the catalogue")
}

/// Prices typed from a pinned PDF, if the PDF read is the same and the complement has
/// not expired; else none, with the reason.
fn pdf_prices(titre: &str, docs: &[DocStatus], today: &str) -> (Vec<Price>, Option<String>) {
    let Some(p) = catalogue::PDF_PRICES.iter().find(|p| p.titre == titre) else {
        return (Vec::new(), None);
    };
    let read = docs.iter().find(|d| d.id == p.doc);
    if !catalogue::usable(p.expires, today) {
        return (
            Vec::new(),
            Some(format!(
                "prix relevés le {} expirés le {}",
                p.checked_on, p.expires
            )),
        );
    }
    match read.and_then(|d| d.sha256.as_deref()) {
        Some(sha) if sha == p.sha256 => (
            p.rows
                .iter()
                .map(|r| Price {
                    product: r.0.into(),
                    category: r.1.into(),
                    ages: Some(r.2.into()),
                    eur: r.3,
                    tier: r.4.into(),
                    from: r.5.map(str::to_owned),
                    until: r.6.map(str::to_owned),
                    season: p.season.into(),
                    doc: p.doc.into(),
                    quote: r.7.into(),
                })
                .collect(),
            None,
        ),
        Some(_) => (
            Vec::new(),
            Some(format!(
                "grille tarifaire modifiée depuis le relevé du {} : prix retirés jusqu'à relecture",
                p.checked_on
            )),
        ),
        None => (Vec::new(), Some("grille tarifaire illisible".into())),
    }
}

fn slug(s: &str) -> String {
    let f: String = fold(s)
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    f.split('-')
        .filter(|x| !x.is_empty())
        .collect::<Vec<_>>()
        .join("-")
        .chars()
        .take(60)
        .collect()
}

/// Reads every issuer page and builds the titles (personal use only: no reuse licence).
/// A page that cannot be downloaded or no longer parses is reported in `docs` and the
/// titles that depend on it in `missing`.
pub fn read_titles(dl: &Downloader, today: &str) -> TitresRead {
    let mut r = Reader {
        dl,
        out: TitresRead::default(),
    };
    // Nordic France: national pass and member list.
    let national = r.lines(doc("nf-pass-national")).and_then(|l| {
        let p = sources::parse_nf_national(&l, "nf-pass-national");
        r.parse("nf-pass-national", p)
    });
    // ENJ: FAQ, shop, grid.
    let faq: Option<EnjFaq> = r.lines(doc("enj-faq")).and_then(|l| {
        let p = sources::parse_enj_faq(&l);
        r.parse("enj-faq", p)
    });
    let shop = r
        .lines(doc("enj-pass-saison"))
        .map(|l| sources::parse_enj_shop(&l));
    if shop.as_ref().is_some_and(Vec::is_empty) {
        r.broken("enj-pass-saison", "aucun site dans la boutique".into());
    }
    let grid = doc("enj-grille");
    let _ = r.body(grid.id, grid.source, grid.url);
    if let Some(p) = catalogue::PDF_PRICES.iter().find(|p| p.doc == grid.id)
        && let Some(d) = r.out.docs.iter_mut().rev().find(|d| d.id == grid.id)
        && d.sha256.as_deref().is_some_and(|s| s != p.sha256)
    {
        d.status = "changed".into();
        d.error = Some(format!("SHA-256 différent du relevé du {}", p.checked_on));
    }
    if let Some(n) = &national {
        r.out.titres.push(n.clone());
    } else {
        r.out
            .missing
            .push(("nordic-pass-national".into(), "nf-pass-national".into()));
    }
    match (&national, &faq) {
        (Some(n), Some(f)) => {
            let enj: Vec<MemberSpec> = n
                .members
                .iter()
                .filter(|m| fold(&m.group).contains("jurassien"))
                .cloned()
                .collect();
            for (id, name, product, validity) in [
                (
                    "enj-montagnes-du-jura-saison",
                    "Pass Nordique saison Montagnes du Jura",
                    "saison",
                    &f.saison,
                ),
                (
                    "enj-montagnes-du-jura-hebdo",
                    "Pass Nordique hebdo Montagnes du Jura",
                    "hebdo",
                    &f.hebdo,
                ),
            ] {
                let (prices, why) = pdf_prices(id, &r.out.docs, today);
                let mut notes = vec!["membres : liste « Espace Nordique Jurassien » de la page du Pass national (Nordic France)".to_owned()];
                notes.extend(why);
                r.out.titres.push(Titre {
                    id: id.into(),
                    name: name.into(),
                    scope: "massif".into(),
                    issuer: "Espace Nordique Jurassien".into(),
                    products: vec![product.into()],
                    validity: validity.clone(),
                    validity_doc: "enj-faq".into(),
                    notes,
                    members: enj.clone(),
                    abroad: Some("Suisse romande (sites adhérents à Romandie Ski de Fond)".into()),
                    prices,
                });
            }
            r.out.site_rules.push(SiteRule {
                depts: vec!["01".into(), "25".into(), "39".into()],
                rule: f.seance.clone(),
                doc: "enj-faq".into(),
            });
        }
        _ => {
            for id in [
                "enj-montagnes-du-jura-saison",
                "enj-montagnes-du-jura-hebdo",
            ] {
                r.out
                    .missing
                    .push((id.into(), "nf-pass-national, enj-faq".into()));
            }
        }
    }
    // ENJ site groups: shop entries that sell several sites together.
    if let (Some(f), Some(groups)) = (&faq, &shop) {
        for g in groups {
            // A group sells several sites: several lines under its heading, or one line
            // naming several places (« Lajoux - Les Moussières - La Pesse »). A single
            // site is a site pass of one domain: nothing to group.
            let site_keys: Vec<String> = g
                .sites
                .iter()
                .flat_map(|s| sources::member_keys(s))
                .collect();
            let several = g.sites.len() >= 2
                || g.sites
                    .iter()
                    .any(|s| s.contains(" - ") && sources::member_keys(s).len() >= 2);
            if !several {
                continue;
            }
            let mut keys = site_keys;
            keys.extend(sources::member_keys(&g.label));
            r.out.titres.push(Titre {
                id: format!("enj-site-{}", slug(&g.label)),
                name: format!("Pass saison de site : {}", g.label),
                scope: "site".into(),
                issuer: "site nordique (boutique de l'Espace Nordique Jurassien)".into(),
                products: vec!["saison_site".into()],
                validity: f.site.clone(),
                validity_doc: "enj-faq".into(),
                notes: vec![
                    format!(
                        "sites vendus ensemble par la boutique : {}",
                        if g.sites.is_empty() {
                            g.label.clone()
                        } else {
                            g.sites.join(" ; ")
                        }
                    ),
                    "prix non publié en texte (calculé par la boutique)".into(),
                ],
                members: vec![MemberSpec {
                    label: g.label.clone(),
                    depts: g.depts.clone(),
                    keys,
                    group: "Espace Nordique Jurassien".into(),
                }],
                abroad: None,
                prices: Vec::new(),
            });
        }
    }
    // Haute-Savoie Nordic.
    if let Some(t) = r.lines(doc("hsn-np74")).and_then(|l| {
        let p = sources::parse_hsn(&l, "hsn-np74");
        r.parse("hsn-np74", p)
    }) {
        r.out.titres.push(t);
    } else {
        r.out
            .missing
            .push(("hsn-nordic-pass-74".into(), "hsn-np74".into()));
    }
    // Nordic Alpes du Sud: shop (members), prices, domain pages.
    let nads_members = r
        .lines(doc("nads-billetterie"))
        .map(|l| sources::parse_nads_shop(&l))
        .filter(|m| !m.is_empty());
    if nads_members.is_none() {
        r.broken(
            "nads-billetterie",
            "aucun domaine dans la billetterie".into(),
        );
    }
    let tarifs_doc = doc("nads-tarifs");
    let tarifs_body = r.body(tarifs_doc.id, tarifs_doc.source, tarifs_doc.url);
    match (&nads_members, &tarifs_body) {
        (Some(m), Some(b)) => {
            let l = sources::html_lines(&String::from_utf8_lossy(b));
            let p = sources::parse_nads_tarifs(&l, "nads-tarifs", m);
            if let Some(ts) = r.parse("nads-tarifs", p) {
                r.out.titres.extend(ts);
            }
        }
        _ => {
            for id in ["nads-alpes-du-sud-saison", "nads-alpes-du-sud-semaine"] {
                r.out
                    .missing
                    .push((id.into(), "nads-billetterie, nads-tarifs".into()));
            }
        }
    }
    if let Some(b) = &tarifs_body {
        let areas: BTreeSet<String> = sources::hrefs(&String::from_utf8_lossy(b))
            .into_iter()
            .filter(|h| h.starts_with("/domaines-nordiques/"))
            .collect();
        // Domain pages and their names (text of the links of the area pages).
        let mut pages: BTreeMap<String, String> = BTreeMap::new();
        for a in &areas {
            let url = format!("{}{a}", catalogue::NADS_ROOT);
            if let Some(b) = r.body(&url, "nordicalpesdusud", &url) {
                for (href, text) in sources::links(&String::from_utf8_lossy(&b)) {
                    if href.starts_with("/domaines/") {
                        let e = pages.entry(href).or_default();
                        if e.is_empty() {
                            *e = text;
                        }
                    }
                }
            }
        }
        for (p, name) in pages {
            let url = format!("{}{p}", catalogue::NADS_ROOT);
            if let Some(b) = r.body(&url, "nordicalpesdusud", &url) {
                let l = sources::html_lines(&String::from_utf8_lossy(&b));
                let depts = ["04", "05", "06"].map(String::from);
                let name = (!name.is_empty()).then_some(name);
                let parsed = sources::parse_nads_domain(&l, &url, name, &depts);
                if let Some(sp) = r.parse(&url, parsed) {
                    r.out.site_pages.push(sp);
                }
            }
        }
    }
    // Nordic Isère.
    if let Some(t) = r.lines(doc("isere-pass")).and_then(|l| {
        let p = sources::parse_isere(&l, "isere-pass");
        r.parse("isere-pass", p)
    }) {
        r.out.titres.push(t);
    } else {
        r.out
            .missing
            .push(("nordic-isere-isere-drome".into(), "isere-pass".into()));
    }
    r.out
}

/// Census domains of a domain page (its name and its address in its departments).
pub fn match_site_page(p: &SitePage, domains: &[Domain], today: &str) -> MemberMatch {
    let slug = p
        .url
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .replace('-', " ");
    let mut keys = sources::member_keys(&p.name);
    keys.push(slug);
    match_member(
        &MemberSpec {
            label: p.name.clone(),
            depts: p.depts.clone(),
            keys,
            group: "Nordic Alpes du Sud".into(),
        },
        domains,
        today,
    )
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::parse;
    use serde_json::json;

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
            f("site-9227542", "Sur Lyand", "Corbonod", "01", 6.5),
            f("net-196901574", "Saint-Sigismond", "Saint-Sigismond", "74", 6.6),
        ]}))
        .unwrap()
    }

    fn m(label: &str, depts: &[&str]) -> MemberSpec {
        MemberSpec {
            label: label.into(),
            depts: depts.iter().map(|s| (*s).to_owned()).collect(),
            keys: sources::member_keys(label),
            group: String::new(),
        }
    }

    #[test]
    fn automatic_matching() {
        let ds = domains();
        let ids = |x: &MemberSpec| match_member(x, &ds, "2026-11-15").domains;
        assert_eq!(ids(&m("Plateau des Glières", &["74"])), vec!["a"]);
        // Every word of a key: « Chapelle d'Abondance » is not « Chapelle-Rambaud ».
        assert_eq!(ids(&m("La Chapelle d'Abondance", &["74"])), vec!["d"]);
        assert_eq!(
            ids(&m("Domaine Nordique de La Chapelle-Rambaud", &["74"])),
            vec!["c"]
        );
        // Outside its departments, only a full name unique in France.
        let s = match_member(&m("Le Grand-Bornand", &["01"]), &ds, "2026-11-15");
        assert_eq!(
            (s.domains, s.confidence.as_str()),
            (vec!["b".to_owned()], "auto_national")
        );
        // « Glières » is in two departments: not unique, no national match.
        let g = match_member(&m("Plateau des Glières", &["01"]), &ds, "2026-11-15");
        assert!(g.domains.is_empty());
        // « La Chapelle » of another department never matches « La Chapelle-en-X ».
        assert!(
            match_member(
                &m("La Chapelle-Rambaud Fictive", &["05"]),
                &ds,
                "2026-11-15"
            )
            .domains
            .is_empty()
        );
        // A domain of another department through a hand-made complement.
        let s = match_member(&m("Sur Lyand Grand Colombier", &["74"]), &ds, "2026-11-15");
        assert_eq!(
            (s.domains, s.confidence.as_str()),
            (vec!["site-9227542".to_owned()], "verifie")
        );
        // A key of place words only matches nothing.
        assert!(ids(&m("La Combe", &["74"])).is_empty());
        // Plural: « Aillons » is « Aillon ».
        assert_eq!(words("Aillons-Margériaz"), words("Aillon Margériaz"));
        // A hand-made complement, while valid.
        let agy = match_member(&m("Agy", &["74"]), &ds, "2026-11-15");
        assert_eq!(
            (agy.domains, agy.confidence.as_str()),
            (vec!["net-196901574".to_owned()], "verifie")
        );
        assert!(agy.complement.unwrap().contains("expire"));
        // Expired: not used any more.
        assert!(
            match_member(&m("Agy", &["74"]), &ds, "2099-01-01")
                .domains
                .is_empty()
        );
    }

    #[test]
    fn pdf_prices_pinned() {
        let ok = DocStatus {
            id: "enj-grille".into(),
            url: String::new(),
            status: "ok".into(),
            error: None,
            sha256: Some(catalogue::PDF_PRICES[0].sha256.into()),
            fetched_at: None,
        };
        let (p, why) = pdf_prices(
            "enj-montagnes-du-jura-saison",
            std::slice::from_ref(&ok),
            "2026-11-15",
        );
        assert_eq!((p.len(), why), (6, None));
        let changed = DocStatus {
            sha256: Some("0".repeat(64)),
            ..ok.clone()
        };
        let (p, why) = pdf_prices("enj-montagnes-du-jura-saison", &[changed], "2026-11-15");
        assert!(p.is_empty() && why.unwrap().contains("modifiée"));
        let (p, why) = pdf_prices("enj-montagnes-du-jura-saison", &[ok], "2027-05-01");
        assert!(p.is_empty() && why.unwrap().contains("expirés"));
    }

    #[test]
    fn slugs() {
        assert_eq!(
            slug("Sites nordiques de la communauté de communes des Lacs et Montagnes du Ht Doubs"),
            "sites-nordiques-de-la-communaute-de-communes-des-lacs-et-mon"
        );
    }
}
