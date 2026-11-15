//! Collector of official piste plans: finds the plans published for the nordic domains
//! of the census (P3, 219 domains), downloads them through the shared downloader (polite
//! sources of their own: one request every 5 s, cached) and describes them (format, pages,
//! text layer, date). Usage personnel (`personal_sources`): the plans belong to the
//! resorts; they stay in the cache and serve as local input (registration, training of
//! the matching model), never redistributed.
//!
//! Sources:
//! - Nordic France: media library of its WordPress (`wp-json/wp/v2/media`, searches
//!   « plan » and « piste »), each file attached to a station page whose title names the
//!   domain;
//! - Espace Nordique Jurassien: the massif pages listed by « les domaines nordiques »,
//!   which link the plans (`ENJ_Plans_…_ski.pdf`) and the sites of the massif.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use skimap_common::config::{Config, RateLimit};
use skimap_common::download::{Downloader, Request, SourcePolicy};
use skimap_common::geom::BBox;
use skimap_common::layout::DataLayout;

use crate::error::{Error, Result};
use crate::input;
use crate::labels::{fold, similarity};

/// Nordic France REST API.
pub const NF_API: &str = "https://www.nordicfrance.fr/wp-json/wp/v2";
/// Espace Nordique Jurassien, list of the massif pages.
pub const ENJ_LIST: &str = "https://www.espacenordiquejurassien.com/les-domaines-nordiques.html";
/// ENJ site root.
pub const ENJ_ROOT: &str = "https://www.espacenordiquejurassien.com";

/// Downloader source of the Nordic France API pages.
pub const SRC_NF_API: &str = "plans-nordicfrance-api";
/// Downloader source of the Nordic France files (plans).
pub const SRC_NF_FILES: &str = "plans-nordicfrance";
/// Downloader source of the ENJ pages and plans.
pub const SRC_ENJ: &str = "plans-enj";

/// Policies of the collector's sources: one request every 5 s (no published quota:
/// prudence), pages cached 7 days, plans 30 days (no `ETag` on these servers).
pub fn policies() -> Vec<SourcePolicy> {
    let every5 = || vec![RateLimit::new(1, Duration::from_secs(5))];
    vec![
        SourcePolicy::new(SRC_NF_API)
            .limits(every5())
            .max_age(Duration::from_secs(7 * 86_400)),
        SourcePolicy::new(SRC_NF_FILES)
            .limits(every5())
            .max_age(input::PDF_MAX_AGE),
        SourcePolicy::new(SRC_ENJ)
            .limits(every5())
            .max_age(Duration::from_secs(7 * 86_400)),
    ]
}

/// The shared downloader with the collector's policies.
pub fn downloader(cfg: &Config) -> Result<Downloader> {
    let mut b = Downloader::builder(cfg.layout().cache_root())
        .rate_limit_overrides(cfg.rate_limits.clone());
    for p in policies() {
        b = b.policy(p);
    }
    Ok(b.build()?)
}

/// A domain of the census.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Domain {
    /// Census identifier.
    pub id: String,
    /// Name.
    pub name: String,
    /// Commune.
    pub commune: String,
    /// Département.
    pub departement: String,
    /// Extent of its pistes.
    pub bbox: BBox,
    /// Names of its stations in the Nordic France widget.
    pub nf_names: Vec<String>,
}

/// Domains of the census (`data/out/france/domaines/`), with the Nordic France station
/// names matched to them by the census.
pub fn load_census(layout: &DataLayout) -> Result<Vec<Domain>> {
    let dir = layout.france_dir().join("domaines");
    let read = |name: &str| -> Result<Value> {
        let p = dir.join(name);
        let s = std::fs::read_to_string(&p).map_err(|e| Error::io(&p, e))?;
        serde_json::from_str(&s).map_err(|e| Error::Json {
            context: name.to_owned(),
            source: e,
        })
    };
    let geo = read("domaines.geojson")?;
    let nf = read("nordic_france.json")?;
    let mut nf_names: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for m in nf["matches"].as_array().into_iter().flatten() {
        if let (Some(d), Some(n)) = (m["domain"].as_str(), m["name"].as_str()) {
            nf_names.entry(d.to_owned()).or_default().push(n.to_owned());
        }
    }
    let mut out = Vec::new();
    for f in geo["features"].as_array().into_iter().flatten() {
        let p = &f["properties"];
        let Some(id) = p["id"].as_str() else { continue };
        let (mut s, mut w, mut n, mut e) = (90.0f64, 180.0f64, -90.0f64, -180.0f64);
        let mut stack = vec![&f["geometry"]["coordinates"]];
        while let Some(v) = stack.pop() {
            match v.as_array() {
                Some(a) if a.len() == 2 && a[0].is_f64() && a[1].is_f64() => {
                    let (lon, lat) = (a[0].as_f64().unwrap_or(0.0), a[1].as_f64().unwrap_or(0.0));
                    s = s.min(lat);
                    n = n.max(lat);
                    w = w.min(lon);
                    e = e.max(lon);
                }
                Some(a) => stack.extend(a.iter()),
                None => {}
            }
        }
        if s > n {
            continue;
        }
        out.push(Domain {
            id: id.to_owned(),
            name: p["name"].as_str().unwrap_or_default().to_owned(),
            commune: p["commune"]["name"].as_str().unwrap_or_default().to_owned(),
            departement: p["commune"]["departement"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
            bbox: BBox::new(s, w, n, e),
            nf_names: nf_names.remove(id).unwrap_or_default(),
        });
    }
    Ok(out)
}

/// Words that say a file is not a cross-country ski plan (or not a plan at all).
const NOT_A_SKI_PLAN: &[&str] = &[
    "raquette",
    "pieton",
    "navette",
    "bilan",
    "strategie",
    "presentation",
    "apprentissage",
    "accompagnement",
    "logo",
    "avenir",
    "feuille de route",
    "offre",
    "alpin",
    "rando",
    "ligne",
    "horaire",
    "tarif",
    "programme",
    "affiche",
];

/// Whether a file title or name looks like a cross-country ski plan: it says « plan » or
/// « carte » and nothing of [`NOT_A_SKI_PLAN`] (« plan des pistes nordiques et alpines »
/// stays when it also says « nordique », « fond » or « ski de fond »). `Err` gives the
/// reason of a rejection.
pub fn ski_plan_title(text: &str) -> std::result::Result<(), String> {
    let f = fold(text);
    if !(f.contains("plan") || f.contains("carte")) {
        return Err("ni « plan » ni « carte »".into());
    }
    let nordic = f.contains("nordi") || f.contains("fond") || f.contains("skating");
    for w in NOT_A_SKI_PLAN {
        if f.contains(w) && !(nordic && matches!(*w, "alpin" | "rando" | "raquette" | "pieton")) {
            return Err(format!("« {w} »"));
        }
    }
    Ok(())
}

/// Domains a name designates: census domains whose name, commune or Nordic France station
/// names match it (folded similarity ≥ 0.85, or every word of one in the other).
pub fn domains_named(name: &str, census: &[Domain]) -> Vec<String> {
    let f = fold(name);
    if f.chars().filter(|c| c.is_alphabetic()).count() < 3 {
        return Vec::new();
    }
    let words = |s: &str| -> BTreeSet<String> {
        fold(s)
            .split(' ')
            .filter(|w| {
                w.len() > 2
                    && !matches!(
                        *w,
                        "les" | "des" | "site" | "domaine" | "nordique" | "station" | "espace"
                    )
            })
            .map(str::to_owned)
            .collect()
    };
    let fw = words(name);
    let hit = |cand: &str| {
        let cw = words(cand);
        similarity(cand, name) >= 0.85
            || (!cw.is_empty() && cw.is_subset(&fw))
            || (!fw.is_empty() && fw.is_subset(&cw))
    };
    census
        .iter()
        .filter(|d| {
            d.nf_names.iter().any(|n| hit(n))
                || hit(&d.name)
                || (!d.commune.is_empty() && hit(&d.commune))
        })
        .map(|d| d.id.clone())
        .collect()
}

/// A plan found, before download.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Candidate {
    /// Identifier (`nf-<media id>`, `enj-<file name>`).
    pub id: String,
    /// Where it was found (`nordicfrance-media`, `enj`).
    pub origin: String,
    /// URL of the file.
    pub url: String,
    /// Downloader source.
    pub source: String,
    /// Page linking it.
    pub page_url: String,
    /// Title (media title or file name).
    pub title: String,
    /// Publisher.
    pub publisher: String,
    /// Upload or publication date, if known.
    pub uploaded: Option<String>,
    /// MIME type announced.
    pub mime: String,
    /// Station or massif page.
    pub station: Option<String>,
    /// Census domains designated by the station, page or title.
    pub domains: Vec<String>,
}

/// Every page of a WordPress collection (`per_page=100`).
fn wp_all(dl: &Downloader, path_query: &str) -> Result<Vec<Value>> {
    let mut out = Vec::new();
    for page in 1..=20 {
        let url = format!("{NF_API}/{path_query}&per_page=100&page={page}");
        let f = match dl.fetch(&Request::get(SRC_NF_API, &url)) {
            Ok(f) => f,
            // WordPress answers 400 past the last page.
            Err(skimap_common::Error::Status { status: 400, .. }) if page > 1 => break,
            Err(e) => return Err(e.into()),
        };
        let v: Vec<Value> = f.json()?;
        let n = v.len();
        out.extend(v);
        if n < 100 {
            break;
        }
    }
    Ok(out)
}

/// Files set aside by their title: URL and reason.
pub type Rejected = Vec<(String, String)>;

/// Plans of the Nordic France media library. Returns the candidates and the rejected
/// files with their reason.
pub fn discover_nordicfrance(
    dl: &Downloader,
    census: &[Domain],
) -> Result<(Vec<Candidate>, Rejected)> {
    let stations: BTreeMap<i64, (String, String)> =
        wp_all(dl, "nordicfrance_station?_fields=id,title,link")?
            .into_iter()
            .filter_map(|s| {
                Some((
                    s["id"].as_i64()?,
                    (
                        html_text(s["title"]["rendered"].as_str()?),
                        s["link"].as_str().unwrap_or_default().to_owned(),
                    ),
                ))
            })
            .collect();
    let mut media: BTreeMap<i64, Value> = BTreeMap::new();
    for q in ["plan", "piste"] {
        for m in wp_all(
            dl,
            &format!("media?search={q}&_fields=id,date,source_url,title,mime_type,post,link"),
        )? {
            if let Some(id) = m["id"].as_i64() {
                media.insert(id, m);
            }
        }
    }
    let mut out = Vec::new();
    let mut rejected = Vec::new();
    for (id, m) in media {
        let url = m["source_url"].as_str().unwrap_or_default().to_owned();
        let mime = m["mime_type"].as_str().unwrap_or_default().to_owned();
        let title = html_text(m["title"]["rendered"].as_str().unwrap_or_default());
        let file = url.rsplit('/').next().unwrap_or_default().to_owned();
        if !matches!(
            mime.as_str(),
            "application/pdf" | "image/jpeg" | "image/png"
        ) {
            rejected.push((url, format!("type {mime}")));
            continue;
        }
        if let Err(r) = ski_plan_title(&format!("{title} {file}")) {
            rejected.push((url, r));
            continue;
        }
        let station = m["post"].as_i64().and_then(|p| stations.get(&p));
        let mut domains = station.map_or_else(Vec::new, |s| domains_named(&s.0, census));
        if domains.is_empty() {
            domains = domains_named(&title, census);
        }
        out.push(Candidate {
            id: format!("nf-{id}"),
            origin: "nordicfrance-media".into(),
            url,
            source: SRC_NF_FILES.into(),
            page_url: station
                .map(|s| s.1.clone())
                .unwrap_or_else(|| m["link"].as_str().unwrap_or_default().to_owned()),
            title,
            publisher: station.map_or_else(
                || "Nordic France".to_owned(),
                |s| format!("{} (via Nordic France)", s.0),
            ),
            uploaded: m["date"].as_str().map(str::to_owned),
            mime,
            station: station.map(|s| s.0.clone()),
            domains,
        });
    }
    Ok((out, rejected))
}

/// Decodes the few HTML entities of WordPress titles.
pub fn html_text(s: &str) -> String {
    s.replace("&#8217;", "'")
        .replace("&rsquo;", "'")
        .replace("&#8211;", "-")
        .replace("&#038;", "&")
        .replace("&amp;", "&")
        .replace("&nbsp;", " ")
        .replace("&#039;", "'")
        .replace("&quot;", "\"")
}

/// `href` targets of a page, entities decoded.
pub fn links(html: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(i) = rest.find("href=\"") {
        rest = &rest[i + 6..];
        let Some(j) = rest.find('"') else { break };
        out.push(rest[..j].replace("&amp;", "&"));
        rest = &rest[j..];
    }
    out
}

/// `originalname` of an ENJ download link.
fn enj_original_name(link: &str) -> Option<String> {
    let i = link.find("originalname=")? + "originalname=".len();
    let end = link[i..].find('&').map_or(link.len(), |j| i + j);
    Some(link[i..end].to_owned())
}

/// Plans of the ENJ massif pages: links to `ENJ_Plans_…` PDFs whose name says « ski »
/// (or « fond »), with the domains of the sites listed on the page.
pub fn discover_enj(dl: &Downloader, census: &[Domain]) -> Result<Vec<Candidate>> {
    let list = dl.fetch(&Request::get(SRC_ENJ, ENJ_LIST))?.text()?;
    let mut pages: BTreeSet<String> = BTreeSet::new();
    for l in links(&list) {
        let abs = if l.starts_with('/') {
            format!("{ENJ_ROOT}{l}")
        } else {
            l
        };
        let name = abs.trim_start_matches(ENJ_ROOT).trim_start_matches('/');
        if abs.starts_with(ENJ_ROOT)
            && abs.ends_with(".html")
            && !name.contains('/')
            && !matches!(
                name,
                "activites.html"
                    | "bulletin-neige.html"
                    | "carte-interactive-snowmap.html"
                    | "contact.html"
                    | "espace-pro.html"
                    | "home.html"
                    | "infos-pratiques.html"
                    | "la-halte.html"
                    | "les-domaines-nordiques.html"
                    | "mentions.html"
                    | "mentions_legales.html"
                    | "nos-liens.html"
                    | "plan-du-site.html"
                    | "vente-en-ligne-picto.html"
                    | "webcams.html"
            )
        {
            pages.insert(abs);
        }
    }
    let mut out: Vec<Candidate> = Vec::new();
    for page in pages {
        let html = dl.fetch(&Request::get(SRC_ENJ, &page))?.text()?;
        let ls = links(&html);
        // Sites of the massif: « /annuaire/site_nordique_de_chapelle_des_bois~….html ».
        let mut domains: BTreeSet<String> = BTreeSet::new();
        for l in &ls {
            if let Some(slug) = l
                .strip_prefix("/annuaire/")
                .and_then(|s| s.split('~').next())
                && (slug.contains("site") || slug.contains("domaine") || slug.contains("foyer"))
            {
                let name = slug.replace('_', " ");
                domains.extend(domains_named(&name, census));
            }
        }
        // Else the page's own name, « pontarlier-larmont-bolandoz » (each word).
        if domains.is_empty() {
            let slug = page
                .trim_start_matches(ENJ_ROOT)
                .trim_start_matches('/')
                .trim_end_matches(".html");
            for w in slug.split('-').filter(|w| w.len() > 3) {
                domains.extend(domains_named(w, census));
            }
        }
        for l in &ls {
            let Some(name) = enj_original_name(l) else {
                continue;
            };
            let f = fold(&name);
            if !(f.starts_with("enj plans") || f.contains("plan"))
                || !name.to_lowercase().ends_with(".pdf")
                || !(f.contains("ski") || f.contains("fond"))
                || f.contains("raquette")
            {
                continue;
            }
            let url = if l.starts_with('/') {
                format!("{ENJ_ROOT}{l}")
            } else {
                l.clone()
            };
            if out.iter().any(|c| c.url == url) {
                continue;
            }
            out.push(Candidate {
                id: format!("enj-{}", name.trim_end_matches(".pdf")),
                origin: "enj".into(),
                url,
                source: SRC_ENJ.into(),
                page_url: page.clone(),
                title: name.clone(),
                publisher: "Espace Nordique Jurassien".into(),
                uploaded: None,
                mime: "application/pdf".into(),
                station: Some(page.trim_start_matches(ENJ_ROOT).to_owned()),
                domains: domains.iter().cloned().collect(),
            });
        }
    }
    Ok(out)
}

/// A plan downloaded and described.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Collected {
    /// The candidate.
    pub candidate: Candidate,
    /// SHA-256 of the file.
    pub sha256: Option<String>,
    /// Size, bytes.
    pub bytes: Option<u64>,
    /// `pdf_text` (text layer), `pdf_picture` (one image, no text), `pdf_vector`
    /// (drawing without text layer), `image`, `other`.
    pub format: Option<String>,
    /// Pages of the PDF.
    pub pages: Option<usize>,
    /// Size of page 1, points.
    pub page_pt: Option<[f64; 2]>,
    /// Words of the text layer of page 1.
    pub words: Option<usize>,
    /// Creation date of the PDF.
    pub created: Option<String>,
    /// Software that produced it.
    pub creator: Option<String>,
    /// Identifier of an earlier candidate with the same file.
    pub duplicate_of: Option<String>,
    /// Download or probe error.
    pub error: Option<String>,
}

/// Downloads and probes one candidate.
pub fn fetch_one(c: &Candidate, dl: &Downloader, layout: &DataLayout) -> Collected {
    let mut out = Collected {
        candidate: c.clone(),
        sha256: None,
        bytes: None,
        format: None,
        pages: None,
        page_pt: None,
        words: None,
        created: None,
        creator: None,
        duplicate_of: None,
        error: None,
    };
    let f = match dl.fetch(&Request::get(&c.source, &c.url).max_age(input::PDF_MAX_AGE)) {
        Ok(f) => f,
        Err(e) => {
            out.error = Some(e.to_string());
            return out;
        }
    };
    out.sha256 = Some(f.meta.sha256.clone());
    out.bytes = std::fs::metadata(&f.path).ok().map(|m| m.len());
    let head = std::fs::read(&f.path)
        .ok()
        .map(|b| b[..b.len().min(8)].to_vec())
        .unwrap_or_default();
    if head.starts_with(b"%PDF") {
        let probed = input::text_layer(&f, layout).and_then(|t| input::probe(&f.path, &t));
        match probed {
            Ok(p) => {
                out.format = Some(
                    if p.words >= 20 {
                        "pdf_text"
                    } else if p.picture_only() {
                        "pdf_picture"
                    } else {
                        "pdf_vector"
                    }
                    .to_owned(),
                );
                out.pages = Some(p.pages);
                out.page_pt = Some(p.page_pt);
                out.words = Some(p.words);
                out.created = p.created;
                out.creator = p.creator;
            }
            Err(e) => out.error = Some(e.to_string()),
        }
    } else if head.starts_with(&[0xff, 0xd8]) || head.starts_with(b"\x89PNG") {
        out.format = Some("image".into());
    } else {
        out.format = Some("other".into());
    }
    out
}

/// Downloads every candidate (in order; duplicates by SHA-256 marked).
pub fn fetch_all(cands: &[Candidate], dl: &Downloader, layout: &DataLayout) -> Vec<Collected> {
    let mut out: Vec<Collected> = Vec::new();
    for c in cands {
        let mut got = fetch_one(c, dl, layout);
        if let Some(sha) = &got.sha256
            && let Some(first) = out
                .iter()
                .find(|o| o.sha256.as_ref() == Some(sha) && o.duplicate_of.is_none())
        {
            got.duplicate_of = Some(first.candidate.id.clone());
        }
        out.push(got);
    }
    out
}

/// Layer of the catalogue (`data/out/france/plans/`).
pub const LAYER: &str = "plans";

/// Key figures of a catalogue.
pub fn summary(col: &[Collected], census: &[Domain]) -> Value {
    let unique: Vec<&Collected> = col
        .iter()
        .filter(|c| c.duplicate_of.is_none() && c.error.is_none())
        .collect();
    let mut formats: BTreeMap<String, usize> = BTreeMap::new();
    let mut years: BTreeMap<String, usize> = BTreeMap::new();
    let mut origins: BTreeMap<String, usize> = BTreeMap::new();
    let mut domains: BTreeSet<&str> = BTreeSet::new();
    for c in &unique {
        *formats
            .entry(c.format.clone().unwrap_or_default())
            .or_default() += 1;
        let year = c
            .created
            .as_deref()
            .and_then(|d| {
                d.split_whitespace()
                    .find(|w| w.len() == 4 && w.starts_with("20"))
            })
            .map(str::to_owned)
            .or_else(|| {
                c.candidate
                    .uploaded
                    .as_deref()
                    .map(|d| d[..4.min(d.len())].to_owned())
            })
            .unwrap_or_else(|| "inconnue".into());
        *years.entry(year).or_default() += 1;
        *origins.entry(c.candidate.origin.clone()).or_default() += 1;
        domains.extend(c.candidate.domains.iter().map(String::as_str));
    }
    serde_json::json!({
        "candidates": col.len(),
        "errors": col.iter().filter(|c| c.error.is_some()).count(),
        "duplicates": col.iter().filter(|c| c.duplicate_of.is_some()).count(),
        "plans": unique.len(),
        "plans_with_domain": unique.iter().filter(|c| !c.candidate.domains.is_empty()).count(),
        "domains_with_plan": domains.len(),
        "census_domains": census.len(),
        "formats": formats,
        "years": years,
        "origins": origins,
    })
}

/// Writes `data/out/france/plans/catalogue.json` (every candidate, downloaded or not,
/// with its description) and its metadata; returns the summary.
pub fn write_catalogue(
    layout: &DataLayout,
    col: &[Collected],
    rejected: &[(String, String)],
    census: &[Domain],
) -> Result<Value> {
    use skimap_common::meta::{FileMeta, LayerMeta, SourceRef};
    let dir = layout.france_layer_dir(LAYER)?;
    DataLayout::ensure(&dir)?;
    skimap_common::output::write_json(&dir.join("catalogue.json"), col)?;
    let rej: Vec<Value> = rejected
        .iter()
        .map(|(u, r)| serde_json::json!({"url": u, "reason": r}))
        .collect();
    skimap_common::output::write_json(&dir.join("rejected.json"), &rej)?;
    let stats = summary(col, census);
    let mut meta = LayerMeta::new(LAYER, skimap_common::layout::FRANCE);
    meta.tickets = vec!["P22 (#121)".to_owned()];
    meta.validity = Some("plans de la saison publiés au moment de la collecte".to_owned());
    meta.sources = vec![
        SourceRef::new(
            "Nordic France, médiathèque (plans des stations)",
            format!("{NF_API}/media"),
            "aucune licence publiée",
            "Nordic France et les stations",
        )
        .personal_use_only()
        .note("plans des stations : jamais redistribués, entrée locale du pipeline seulement"),
        SourceRef::new(
            "Espace Nordique Jurassien, plans des massifs",
            ENJ_LIST,
            "aucune licence publiée",
            "Espace Nordique Jurassien",
        )
        .personal_use_only()
        .note("plans des massifs : jamais redistribués, entrée locale du pipeline seulement"),
    ];
    meta.files = vec![
        FileMeta::new(
            "catalogue.json",
            "plans trouvés : URL, page, station, domaines du recensement, format, pages, date, doublons, erreurs (les fichiers restent dans le cache)",
        )
        .licence("usage personnel")
        .count(col.len()),
        FileMeta::new("rejected.json", "fichiers écartés par leur titre, avec la raison")
            .count(rejected.len()),
    ];
    meta.stats = stats.clone();
    meta.notes = vec![
        "Sources à usage personnel (personal_sources) : les plans appartiennent aux stations ; ils ne sont ni versionnés ni redistribués.".to_owned(),
    ];
    meta.write(&dir)?;
    Ok(stats)
}

/// Reads `data/out/france/plans/catalogue.json` (written by [`write_catalogue`]).
pub fn load_catalogue(layout: &DataLayout) -> Result<Vec<Collected>> {
    let p = layout.france_layer_dir(LAYER)?.join("catalogue.json");
    let bytes = std::fs::read(&p).map_err(|e| Error::io(&p, e))?;
    serde_json::from_slice(&bytes).map_err(|e| Error::Json {
        context: "catalogue.json".to_owned(),
        source: e,
    })
}

/// Whether the pipeline reads this file: downloaded, not a duplicate, a PDF or an image.
pub fn readable(c: &Collected) -> bool {
    c.duplicate_of.is_none()
        && c.error.is_none()
        && c.format
            .as_deref()
            .is_some_and(|f| f.starts_with("pdf") || f == "image")
}

/// The source of a collected plan (its publisher, page and date; no licence is published).
pub fn plan_source(c: &Collected) -> crate::spec::PlanSource {
    crate::spec::PlanSource {
        site: c.candidate.domains.first().cloned().unwrap_or_default(),
        id: c.candidate.id.clone(),
        title: c.candidate.title.clone(),
        publisher: c.candidate.publisher.clone(),
        url: c.candidate.url.clone(),
        page_url: c.candidate.page_url.clone(),
        source: c.candidate.source.clone(),
        edition: c
            .created
            .clone()
            .or_else(|| c.candidate.uploaded.clone())
            .unwrap_or_default(),
        rights: "aucune licence publiée : propriété de l'exploitant, usage personnel".into(),
    }
}

/// `b` grown by `km` on every side.
pub fn grown(b: BBox, km: f64) -> BBox {
    let dlat = km / 111.0;
    let dlon = km / (111.0 * b.center().lat.to_radians().cos());
    BBox::new(b.south - dlat, b.west - dlon, b.north + dlat, b.east + dlon)
}

/// Union of boxes.
pub fn union(bs: &[BBox]) -> Option<BBox> {
    let first = *bs.first()?;
    Some(bs.iter().fold(first, |a, b| {
        BBox::new(
            a.south.min(b.south),
            a.west.min(b.west),
            a.north.max(b.north),
            a.east.max(b.east),
        )
    }))
}

/// Margin of the hint around the domains of a plan, km.
pub const HINT_MARGIN_KM: f64 = 10.0;

/// Extent where a plan is looked for: its census domains, grown by [`HINT_MARGIN_KM`].
pub fn hint(domains: &[String], census: &[Domain]) -> Option<BBox> {
    let boxes: Vec<BBox> = census
        .iter()
        .filter(|d| domains.contains(&d.id))
        .map(|d| d.bbox)
        .collect();
    union(&boxes).map(|b| grown(b, HINT_MARGIN_KM))
}

/// Whether two boxes meet.
pub fn intersects(a: &BBox, b: &BBox) -> bool {
    a.south < b.north && a.north > b.south && a.west < b.east && a.east > b.west
}

/// Plans of the catalogue that may draw an area: readable, with a census domain whose
/// extent meets `area`. The same rule for every site (no hand-picked plan).
pub fn plans_for_area<'a>(
    area: &BBox,
    cat: &'a [Collected],
    census: &[Domain],
) -> Vec<&'a Collected> {
    cat.iter()
        .filter(|c| readable(c))
        .filter(|c| {
            census
                .iter()
                .any(|d| c.candidate.domains.contains(&d.id) && intersects(&d.bbox, area))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_select_cross_country_plans() {
        for ok in [
            "Plan-des-pistes-Cervieres.pdf",
            "Plan_nordique_HAUT-GUIL_25-26.pdf",
            "PLAN-SKI-NORDIQUE-24-25-Site-NF.pdf",
            "Plan des itinéraires raquettes ski nordique du Grandvaux 2026",
            "VALGAU-plan-ski-de-fond-2026-avec-legende.pdf",
        ] {
            assert_eq!(ski_plan_title(ok), Ok(()), "{ok}");
        }
        for no in [
            "PLAN-RAQUETTE.pdf",
            "Plan-des-pistes-Pietons-Puy-Saint-Vincent.pdf",
            "1-Bilan-accompagnement-Nordic-France-Plan-Avenir-Montagnes.pdf",
            "plan-pistes-alpin.pdf",
            "Navettes-du-Giffre_Hiver-25-26.pdf",
            "photo-cervieres.jpg",
        ] {
            assert!(ski_plan_title(no).is_err(), "{no}");
        }
    }

    #[test]
    fn links_and_enj_names_are_read() {
        let html = r#"<a href="/haut-doubs.html">x</a><a href="/csx/scripts/downloader2.php?filename=ENJ_MEDIAS/media/3d/7a/n2kw275c0y&amp;mime=application%2Fpdf&amp;originalname=ENJ_Plans_CCLMHD_VALdeMOUTHE_2511_WEB_ski.pdf&amp;moid=130">plan</a>"#;
        let l = links(html);
        assert_eq!(l.len(), 2);
        assert_eq!(l[0], "/haut-doubs.html");
        assert_eq!(
            enj_original_name(&l[1]).as_deref(),
            Some("ENJ_Plans_CCLMHD_VALdeMOUTHE_2511_WEB_ski.pdf")
        );
        assert_eq!(
            html_text("L&#8217;Izoard &#8211; hiver"),
            "L'Izoard - hiver"
        );
    }

    #[test]
    fn station_names_designate_census_domains() {
        let d = |id: &str, name: &str, commune: &str, nf: &[&str]| Domain {
            id: id.into(),
            name: name.into(),
            commune: commune.into(),
            departement: "25".into(),
            bbox: BBox::new(46.0, 6.0, 46.1, 6.1),
            nf_names: nf.iter().map(|s| (*s).to_owned()).collect(),
        };
        let census = vec![
            d(
                "site-1",
                "Chapelle-des-Bois",
                "Chapelle-des-Bois",
                &["Chapelle des Bois"],
            ),
            d("net-2", "Cervières", "Cervières", &[]),
            d("site-3", "Site nordique de Mouthe", "Chaux-Neuve", &[]),
        ];
        assert_eq!(domains_named("Chapelle des Bois", &census), vec!["site-1"]);
        assert_eq!(domains_named("Nordique Cervières", &census), vec!["net-2"]);
        assert_eq!(
            domains_named("site nordique de mouthe", &census),
            vec!["site-3"]
        );
        assert!(domains_named("Les Saisies", &census).is_empty());
    }
}
