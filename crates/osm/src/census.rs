//! P3 (#16): census of the cross-country ski areas of France.
//!
//! Domains come from [`crate::domains::cluster`] run on every piste of the France area of
//! the planet extract. Then:
//!
//! - **France only**: the representative point of each domain (point of its network
//!   nearest to its centroid) is looked up on geo.api.gouv.fr (`communes?lat=&lon=`);
//!   domains without a French commune are dropped. The commune also names domains that
//!   have no OSM `site=piste` relation.
//! - **OpenSkiMap** (ODbL, `ski_areas.csv` and `runs.csv`, at most one download a day):
//!   overlap of each of our domains with OpenSkiMap's areas, way by way, which measures how
//!   both groupings agree (OpenSkiMap counts 241 French areas with nordic trails).
//! - **SkiRadar** (geo-feed, reuse with attribution « skiradar.fr »): stations with a
//!   nordic block matched to the nearest domain.
//! - **Nordic France** (white-label widget, no licence: personal use only, behind
//!   [`CensusOptions::personal_sources`], off by default): station names matched by name
//!   within the departments of their association.
//!
//! Layers: `data/out/france/domaines/` and `data/out/<site>/domaines/`. The domain layer is
//! derived from OSM and OpenSkiMap (both ODbL) and geo.api.gouv.fr (Licence Ouverte);
//! SkiRadar matches are in a separate file with their attribution, Nordic France matches
//! in another one, written only when personal sources are enabled.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use geo::{Buffer, ConcaveHull, Simplify};
use geo_types::{Coord, LineString, MultiPoint, MultiPolygon, Point, Polygon};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use skimap_common::crs::{from_lambert93, to_lambert93};
use skimap_common::download::{Downloader, Request};
use skimap_common::geom::{BBox, LonLat};
use skimap_common::layout::DataLayout;
use skimap_common::site::Site;

use crate::domains::{Domain, Params, Seed, cluster};
use crate::error::{OsmError, Result};
use crate::extract::{Planet, load_planet};
use crate::pistes::{Piste, PisteSet};
use skimap_common::meta::{FileMeta, LayerMeta, SourceRef};
use skimap_common::output::{feature, point, round7, write_geojson, write_json};

/// Layer name.
pub const LAYER: &str = "domaines";
/// Output directory name for the France-wide layer, next to the site directories.
pub const FRANCE_DIR: &str = "france";
/// OpenSkiMap ski areas (CSV).
pub const OPENSKIMAP_AREAS_URL: &str = "https://tiles.openskimap.org/csv/ski_areas.csv";
/// OpenSkiMap runs (CSV).
pub const OPENSKIMAP_RUNS_URL: &str = "https://tiles.openskimap.org/csv/runs.csv";
/// SkiRadar geo-feed (paged).
pub const SKIRADAR_URL: &str = "https://api.skiradar.fr/api/v1/geo-feed";
/// Nordic France white-label widget, per association.
pub const NORDIC_FRANCE_URL: &str = "https://www.nordicfrance.fr/widget/marqueblanche/association/";
/// Nordic France associations and the departments they cover.
pub const NORDIC_FRANCE_ASSOCIATIONS: &[(&str, &[&str])] = &[
    ("espace-nordique-jurassien", &["01", "25", "39"]),
    ("nordic-alpes-du-sud", &["04", "05", "06"]),
    ("haute-savoie-nordic", &["74"]),
    ("nordic-isere", &["38"]),
    ("savoie-nordic", &["73"]),
    ("les-stations-de-la-drome", &["26"]),
    ("nordic-alsace", &["67", "68", "88"]),
    (
        "nordic-massif-central",
        &["03", "07", "12", "15", "19", "23", "42", "43", "48", "63"],
    ),
    ("ariege-pyrenees-nordiques", &["09"]),
];
/// Buffer around the concave hull of a domain, m.
pub const HULL_BUFFER_M: f64 = 150.0;
/// Largest distance between a SkiRadar station and a domain network, m.
pub const SKIRADAR_MAX_M: f64 = 3000.0;

/// Options of the census.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CensusOptions {
    /// Parameters of the grouping.
    pub params: Params,
    /// Also use sources restricted to personal use (Nordic France widget). Off by
    /// default: the widget has no licence; its station list may be used to check our
    /// census privately, not redistributed.
    pub personal_sources: bool,
}

/// Commune of a point (geo.api.gouv.fr).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Commune {
    /// Name.
    #[serde(rename(deserialize = "nom"))]
    pub name: String,
    /// INSEE code.
    #[serde(rename(deserialize = "code"))]
    pub insee: String,
    /// Department code.
    #[serde(rename(deserialize = "codeDepartement"))]
    pub departement: String,
}

/// Reference to an OSM `site=piste` relation.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SiteRef {
    /// Relation id.
    pub id: i64,
    /// Name.
    pub name: Option<String>,
}

/// Overlap of a domain with an OpenSkiMap area.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OsmAreaOverlap {
    /// OpenSkiMap id.
    pub id: String,
    /// Name (may be empty for generated areas).
    pub name: Option<String>,
    /// Length of the domain's pistes in that area, km.
    pub km: f64,
}

/// One domain of the census, with its derived attributes.
#[derive(Debug, Clone, Serialize)]
pub struct DomainRecord {
    /// Identifier (`site-<relation>` or `net-<way>`).
    pub id: String,
    /// Display name.
    pub name: Option<String>,
    /// Origin of the name: `osm_site`, `openskimap`, `commune`.
    pub name_source: Option<&'static str>,
    /// OSM `site=piste` relation giving the identity, if any.
    pub site_relation: Option<i64>,
    /// `site=piste` relations containing the domain's site relation (umbrella areas).
    pub parent_sites: Vec<SiteRef>,
    /// Network cores merged into the domain.
    pub merged: Vec<String>,
    /// Commune of the representative point.
    pub commune: Option<Commune>,
    /// Representative point (on the network).
    #[serde(skip)]
    pub point: LonLat,
    /// Nordic length, km (each way once).
    pub km_nordic: f64,
    /// Length of hike / snowshoe-only ways, km.
    pub km_hike_only: f64,
    /// Number of ways.
    pub ways: usize,
    /// Names of the piste relations.
    pub piste_names: Vec<String>,
    /// OpenSkiMap areas overlapping the domain, largest first.
    pub openskimap: Vec<OsmAreaOverlap>,
    /// Names of the OpenSkiMap areas covering at least half of the domain (not built from
    /// the site relation of another domain).
    pub alt_names: Vec<String>,
    /// Way ids.
    #[serde(skip)]
    pub way_ids: Vec<i64>,
    /// Footprint (WGS84).
    #[serde(skip)]
    pub footprint: Option<MultiPolygon<f64>>,
}

/// An OpenSkiMap area (subset of `ski_areas.csv`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OpenSkiMapArea {
    /// Id.
    pub id: String,
    /// Name.
    pub name: Option<String>,
    /// Countries (`;` separated).
    pub countries: String,
    /// Nordic length announced by OpenSkiMap, km.
    pub nordic_km: Option<f64>,
    /// OSM `site=piste` relation it was built from, if any.
    pub site_relation: Option<i64>,
}

/// OpenSkiMap data needed by the census.
#[derive(Debug, Clone, Default)]
pub struct OpenSkiMap {
    /// Areas by id.
    pub areas: BTreeMap<String, OpenSkiMapArea>,
    /// Areas of the nordic runs, by OSM element.
    pub runs: RunAreas,
    /// Sources.
    pub sources: Vec<SourceRef>,
}

/// A SkiRadar station with a nordic block.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SkiRadarStation {
    /// Id.
    pub id: String,
    /// Name.
    pub name: String,
    /// Page.
    pub url: Option<String>,
    /// Position.
    #[serde(skip)]
    pub pos: LonLat,
    /// Nordic km announced.
    pub nordic_km: Option<f64>,
}

/// A Nordic France station from the widget.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NordicFranceStation {
    /// Association slug.
    pub association: String,
    /// Station id.
    pub id: u64,
    /// Name.
    pub name: String,
    /// Total km (trails and snowshoes) shown by the widget.
    pub km_total: Option<f64>,
}

/// Result of the census.
#[derive(Debug, Clone)]
pub struct CensusOutput {
    /// France layer directory.
    pub dir: PathBuf,
    /// French domains.
    pub domains: Vec<DomainRecord>,
    /// Key figures (also in the meta file).
    pub stats: Value,
}

// ---------------------------------------------------------------------------------------
// Sources.

fn fetch_commune(dl: &Downloader, p: LonLat) -> Result<Option<Commune>> {
    let url = format!(
        "https://geo.api.gouv.fr/communes?lat={:.5}&lon={:.5}&fields=nom,code,codeDepartement&format=json",
        p.lat, p.lon
    );
    let f = dl.fetch(&Request::get("geo-api", &url))?;
    let v: Vec<Commune> = f.json()?;
    Ok(v.into_iter().next())
}

fn split_list(s: &str) -> impl Iterator<Item = &str> {
    s.split([';', ',', ' '])
        .map(str::trim)
        .filter(|x| !x.is_empty())
}

fn osm_ids(sources: &str, kind: &str) -> Vec<i64> {
    let prefix = format!("openstreetmap.org/{kind}/");
    sources
        .split_whitespace()
        .filter_map(|u| u.split_once(&prefix).map(|(_, id)| id))
        .filter_map(|id| id.trim_end_matches('/').parse().ok())
        .collect()
}

/// Parses OpenSkiMap `ski_areas.csv` (areas with nordic trails only).
pub fn parse_openskimap_areas(csv_text: &[u8]) -> Result<BTreeMap<String, OpenSkiMapArea>> {
    let mut r = csv::Reader::from_reader(csv_text);
    let headers = r
        .headers()
        .map_err(|e| OsmError::format("openskimap", e.to_string()))?
        .clone();
    let col = |name: &str| {
        headers
            .iter()
            .position(|h| h == name)
            .ok_or_else(|| OsmError::format("openskimap", format!("no column {name}")))
    };
    let (c_id, c_name, c_countries, c_nordic, c_km, c_sources) = (
        col("id")?,
        col("name")?,
        col("countries")?,
        col("has_nordic")?,
        col("nordic_distance_km")?,
        col("sources")?,
    );
    let mut out = BTreeMap::new();
    for rec in r.records() {
        let rec = rec.map_err(|e| OsmError::format("openskimap", e.to_string()))?;
        if rec.get(c_nordic) != Some("yes") {
            continue;
        }
        let id = rec.get(c_id).unwrap_or_default().to_owned();
        let name = rec
            .get(c_name)
            .filter(|n| !n.trim().is_empty())
            .map(str::to_owned);
        out.insert(
            id.clone(),
            OpenSkiMapArea {
                id,
                name,
                countries: rec.get(c_countries).unwrap_or_default().to_owned(),
                nordic_km: rec.get(c_km).and_then(|v| v.parse().ok()),
                site_relation: osm_ids(rec.get(c_sources).unwrap_or_default(), "relation")
                    .first()
                    .copied(),
            },
        );
    }
    Ok(out)
}

/// Areas of OpenSkiMap's nordic runs, by OSM source element. A run built from a route
/// relation lists the relation (and sometimes no way at all).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RunAreas {
    /// OSM way → area ids.
    pub ways: HashMap<i64, Vec<String>>,
    /// OSM relation → area ids.
    pub relations: HashMap<i64, Vec<String>>,
}

impl RunAreas {
    /// Areas of a piste: those of its way, else those of its relations.
    pub fn of(&self, set: &PisteSet, p: &Piste) -> Vec<String> {
        if let Some(v) = self.ways.get(&p.way_id) {
            return v.clone();
        }
        let mut out: Vec<String> = Vec::new();
        for &ri in &p.relations {
            for a in self
                .relations
                .get(&set.relations[ri].id)
                .into_iter()
                .flatten()
            {
                if !out.contains(a) {
                    out.push(a.clone());
                }
            }
        }
        out
    }
}

fn add_areas(map: &mut HashMap<i64, Vec<String>>, id: i64, areas: &[String]) {
    let v = map.entry(id).or_default();
    for a in areas {
        if !v.contains(a) {
            v.push(a.clone());
        }
    }
}

/// Parses OpenSkiMap `runs.csv`: OSM ways and relations → area ids, for nordic runs.
pub fn parse_openskimap_runs<R: std::io::Read>(input: R) -> Result<RunAreas> {
    let mut r = csv::Reader::from_reader(input);
    let headers = r
        .headers()
        .map_err(|e| OsmError::format("openskimap", e.to_string()))?
        .clone();
    let col = |name: &str| {
        headers
            .iter()
            .position(|h| h == name)
            .ok_or_else(|| OsmError::format("openskimap", format!("no column {name}")))
    };
    let (c_uses, c_areas, c_sources) = (col("uses")?, col("ski_area_ids")?, col("sources")?);
    let mut out = RunAreas::default();
    let mut rec = csv::StringRecord::new();
    while r
        .read_record(&mut rec)
        .map_err(|e| OsmError::format("openskimap", e.to_string()))?
    {
        if !rec
            .get(c_uses)
            .is_some_and(|u| u.split(';').any(|x| x == "nordic"))
        {
            continue;
        }
        let areas: Vec<String> = split_list(rec.get(c_areas).unwrap_or_default())
            .map(str::to_owned)
            .collect();
        if areas.is_empty() {
            continue;
        }
        let sources = rec.get(c_sources).unwrap_or_default();
        for w in osm_ids(sources, "way") {
            add_areas(&mut out.ways, w, &areas);
        }
        for r in osm_ids(sources, "relation") {
            add_areas(&mut out.relations, r, &areas);
        }
    }
    Ok(out)
}

fn load_openskimap(dl: &Downloader) -> Result<OpenSkiMap> {
    let areas_f = dl.fetch(&Request::get("openskimap", OPENSKIMAP_AREAS_URL))?;
    let runs_f = dl.fetch(&Request::get("openskimap", OPENSKIMAP_RUNS_URL))?;
    let areas = parse_openskimap_areas(&areas_f.bytes()?)?;
    let runs = std::fs::File::open(&runs_f.path).map_err(|e| OsmError::io(&runs_f.path, e))?;
    let runs = parse_openskimap_runs(std::io::BufReader::new(runs))?;
    let attribution = "© OpenSkiMap.org, © les contributeurs d'OpenStreetMap (ODbL)";
    Ok(OpenSkiMap {
        areas,
        runs,
        sources: vec![
            SourceRef::from_fetched(
                "OpenSkiMap ski_areas.csv",
                "ODbL 1.0",
                attribution,
                &areas_f,
            ),
            SourceRef::from_fetched("OpenSkiMap runs.csv", "ODbL 1.0", attribution, &runs_f),
        ],
    })
}

/// Parses one page of the SkiRadar geo-feed: stations with a nordic block, and the
/// number of pages.
pub fn parse_skiradar(v: &Value) -> Result<(Vec<SkiRadarStation>, u64)> {
    let pages = v.get("totalPages").and_then(Value::as_u64).unwrap_or(1);
    let stations = v
        .get("stations")
        .and_then(Value::as_array)
        .ok_or_else(|| OsmError::format("skiradar", "no stations array"))?;
    let mut out = Vec::new();
    for s in stations {
        let Some(n) = s.get("nordique").filter(|n| !n.is_null()) else {
            continue;
        };
        let (Some(lat), Some(lng)) = (
            s.pointer("/geo/lat").and_then(Value::as_f64),
            s.pointer("/geo/lng").and_then(Value::as_f64),
        ) else {
            continue;
        };
        out.push(SkiRadarStation {
            id: s
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            name: s
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            url: s.get("url").and_then(Value::as_str).map(str::to_owned),
            pos: LonLat::new(lng, lat),
            nordic_km: n.get("km").and_then(Value::as_f64),
        });
    }
    Ok((out, pages))
}

fn load_skiradar(dl: &Downloader) -> Result<(Vec<SkiRadarStation>, Vec<SourceRef>)> {
    let mut all = Vec::new();
    let mut sources = Vec::new();
    let mut page = 1;
    loop {
        let url = format!("{SKIRADAR_URL}?page={page}");
        let f = dl.fetch(&Request::get("skiradar", &url))?;
        let v: Value = f.json()?;
        let (st, pages) = parse_skiradar(&v)?;
        all.extend(st);
        sources.push(SourceRef::from_fetched(
            format!("SkiRadar geo-feed page {page}"),
            "réutilisation avec attribution « skiradar.fr » et lien vers la fiche",
            "skiradar.fr",
            &f,
        ));
        if page >= pages || page >= 20 {
            break;
        }
        page += 1;
    }
    Ok((all, sources))
}

fn decode_entities(s: &str) -> String {
    s.replace("&#039;", "'")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&quot;", "\"")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

/// Parses the Nordic France white-label widget of one association.
pub fn parse_nordic_france(association: &str, html: &str) -> Vec<NordicFranceStation> {
    const ITEM: &str = "class=\"Weather-itemContainer";
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(i) = rest.find(ITEM) {
        rest = &rest[i + ITEM.len()..];
        let block_end = rest.find(ITEM).unwrap_or(rest.len());
        let block = &rest[..block_end];
        let id = block
            .split_once("id=\"")
            .and_then(|(_, r)| r.split_once('"'))
            .and_then(|(id, _)| id.parse::<u64>().ok());
        let name = block
            .split_once("class=\"Weather-name\">")
            .and_then(|(_, r)| r.split_once("</p>"))
            .map(|(n, _)| decode_entities(n.trim()));
        let km_total = block
            .split_once("Weather-pistes--km")
            .and_then(|(_, r)| r.split_once("km<").or_else(|| r.split_once("km\n")))
            .and_then(|(t, _)| t.rsplit_once('/'))
            .and_then(|(_, n)| n.trim().replace(',', ".").parse::<f64>().ok());
        if let (Some(id), Some(name)) = (id, name) {
            out.push(NordicFranceStation {
                association: association.to_owned(),
                id,
                name,
                km_total,
            });
        }
    }
    out
}

fn load_nordic_france(dl: &Downloader) -> Result<(Vec<NordicFranceStation>, Vec<SourceRef>)> {
    let mut all = Vec::new();
    let mut sources = Vec::new();
    for (slug, _) in NORDIC_FRANCE_ASSOCIATIONS {
        let url = format!("{NORDIC_FRANCE_URL}{slug}");
        match dl.fetch(&Request::get("nordicfrance", &url)) {
            Ok(f) => {
                let html = String::from_utf8_lossy(&f.bytes()?).into_owned();
                all.extend(parse_nordic_france(slug, &html));
                sources.push(
                    SourceRef::from_fetched(
                        format!("Nordic France widget {slug}"),
                        "aucune licence publiée : usage personnel uniquement",
                        "Nordic France",
                        &f,
                    )
                    .personal_use_only(),
                );
            }
            Err(e) => tracing::warn!(association = slug, error = %e, "Nordic France widget"),
        }
    }
    Ok((all, sources))
}

// ---------------------------------------------------------------------------------------
// Name matching.

/// Lower-case ASCII words of a name, without accents and without generic words.
pub fn name_tokens(s: &str) -> BTreeSet<String> {
    const STOP: &[&str] = &[
        "le",
        "la",
        "les",
        "l",
        "de",
        "des",
        "du",
        "d",
        "et",
        "en",
        "sur",
        "sous",
        "aux",
        "au",
        "a",
        "domaine",
        "nordique",
        "nordiques",
        "site",
        "station",
        "espace",
        "ski",
        "fond",
        "centre",
        "foyer",
        "plateau",
        "col",
        "massif",
        "village",
        "st",
        "saint",
        "sainte",
        "ste",
        "ski-de-fond",
        "pays",
        "val",
        "vallee",
        "haut",
        "haute",
        "hautes",
        "bas",
        "basse",
        "grand",
        "grande",
        "petit",
        "petite",
        "mont",
        "monts",
        "lac",
        "lacs",
        "alpe",
        "chalet",
        "montagne",
        "montagnard",
        "nordic",
        "neige",
        "loisirs",
        "skiable",
        "jura",
    ];
    let folded: String = s
        .chars()
        .map(|c| match c {
            'à' | 'â' | 'ä' | 'á' | 'À' | 'Â' | 'Ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' | 'É' | 'È' | 'Ê' | 'Ë' => 'e',
            'î' | 'ï' | 'Î' | 'Ï' | 'í' => 'i',
            'ô' | 'ö' | 'Ô' | 'Ö' | 'ó' => 'o',
            'ù' | 'û' | 'ü' | 'Ù' | 'Û' | 'Ü' | 'ú' => 'u',
            'ç' | 'Ç' => 'c',
            'ÿ' => 'y',
            c if c.is_alphanumeric() => c.to_ascii_lowercase(),
            _ => ' ',
        })
        .collect();
    folded
        .split_whitespace()
        .filter(|w| !STOP.contains(w))
        .map(str::to_owned)
        .collect()
}

/// Similarity of two token sets: shared tokens over the smaller set (1 when one name is
/// contained in the other).
pub fn name_score(a: &BTreeSet<String>, b: &BTreeSet<String>) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let shared = a.intersection(b).count() as f64;
    shared / a.len().min(b.len()) as f64
}

/// Jaccard index of two token sets.
pub fn jaccard(a: &BTreeSet<String>, b: &BTreeSet<String>) -> f64 {
    let union = a.union(b).count();
    if union == 0 {
        return 0.0;
    }
    a.intersection(b).count() as f64 / union as f64
}

/// A station matched to a domain.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StationMatch {
    /// Index of the domain.
    pub domain: usize,
    /// Containment score of the best name (1 = one name contains the other).
    pub score: f64,
    /// The domain was already the best match of another station (several Nordic France
    /// stations in one OSM network).
    pub shared: bool,
}

/// Score of a domain for a station name: (containment, matched by a domain-level name
/// rather than a piste name, Jaccard, rank of the name).
type NameScore = (f64, u8, f64, u8);

fn rank_names(tok: &BTreeSet<String>, r: &DomainRecord) -> NameScore {
    // Candidate names, ranked: the domain's own name, OpenSkiMap names, the commune, then
    // the names of its pistes (these only when they contain every word of the station
    // name: « La Draye »). Containment first, then domain-level names before piste
    // names, then Jaccard (closest name), then rank.
    let mut names: Vec<(&str, u8)> = Vec::new();
    names.extend(r.name.as_deref().map(|n| (n, 3)));
    names.extend(r.alt_names.iter().map(|n| (n.as_str(), 2)));
    names.extend(r.commune.as_ref().map(|c| (c.name.as_str(), 1)));
    names.extend(r.piste_names.iter().map(|n| (n.as_str(), 0)));
    names
        .iter()
        .map(|(n, rank)| {
            let t = name_tokens(n);
            if *rank == 0 && !t.is_superset(tok) {
                (0.0, 0, 0.0, 0)
            } else {
                (
                    name_score(tok, &t),
                    u8::from(*rank > 0),
                    jaccard(tok, &t),
                    *rank,
                )
            }
        })
        .fold((0.0, 0, 0.0, 0), |a, b| if b > a { b } else { a })
}

/// Matches Nordic France stations to domains by name, within the departments of each
/// station's association. Assignment is greedy by score and one to one first; a station
/// whose candidates are all taken shares its best one.
pub fn match_stations(
    stations: &[NordicFranceStation],
    domains: &[DomainRecord],
) -> Vec<Option<StationMatch>> {
    let mut cands: Vec<(NameScore, f64, usize, usize)> = Vec::new();
    for (si, st) in stations.iter().enumerate() {
        let depts: &[&str] = NORDIC_FRANCE_ASSOCIATIONS
            .iter()
            .find(|(s, _)| *s == st.association)
            .map(|(_, d)| *d)
            .unwrap_or(&[]);
        let tok = name_tokens(&st.name);
        for (di, r) in domains.iter().enumerate() {
            if !r
                .commune
                .as_ref()
                .is_some_and(|c| depts.contains(&c.departement.as_str()))
            {
                continue;
            }
            let score = rank_names(&tok, r);
            if score.0 >= 0.5 {
                cands.push((score, r.km_nordic, si, di));
            }
        }
    }
    cands.sort_by(|a, b| {
        b.0.0
            .total_cmp(&a.0.0)
            .then(b.0.1.cmp(&a.0.1))
            .then(b.0.2.total_cmp(&a.0.2))
            .then(b.0.3.cmp(&a.0.3))
            .then(b.1.total_cmp(&a.1))
            .then(a.2.cmp(&b.2))
            .then(a.3.cmp(&b.3))
    });
    let mut out: Vec<Option<StationMatch>> = vec![None; stations.len()];
    let mut claimed = vec![false; domains.len()];
    for &(score, _, si, di) in &cands {
        if out[si].is_none() && !claimed[di] {
            out[si] = Some(StationMatch {
                domain: di,
                score: score.0,
                shared: false,
            });
            claimed[di] = true;
        }
    }
    for &(score, _, si, di) in &cands {
        if out[si].is_none() {
            out[si] = Some(StationMatch {
                domain: di,
                score: score.0,
                shared: true,
            });
        }
    }
    out
}

// ---------------------------------------------------------------------------------------
// Geometry.

/// Footprint of a set of pistes: concave hull of their vertices in Lambert-93, buffered,
/// simplified, back in WGS84.
pub fn footprint(pistes: &[&Piste], buffer_m: f64) -> Result<MultiPolygon<f64>> {
    let mut pts: Vec<Point<f64>> = Vec::new();
    let mut lines: Vec<LineString<f64>> = Vec::new();
    for p in pistes {
        let mut line = Vec::with_capacity(p.points.len());
        for (_, q) in &p.points {
            let (x, y) = to_lambert93(*q)?;
            pts.push(Point::new(x, y));
            line.push(Coord { x, y });
        }
        lines.push(LineString::new(line));
    }
    let hull: Polygon<f64> = MultiPoint::new(pts).concave_hull();
    let shape = if hull.exterior().0.len() >= 4 && geo::Area::unsigned_area(&hull) > 1.0 {
        hull.buffer(buffer_m)
    } else {
        geo_types::MultiLineString::new(lines).buffer(buffer_m)
    };
    let shape = shape.simplify(10.0);
    let mut out = Vec::new();
    for poly in shape.0 {
        let ring = |ls: &LineString<f64>| -> Result<LineString<f64>> {
            ls.0.iter()
                .map(|c| {
                    from_lambert93(c.x, c.y).map(|p| Coord {
                        x: round7(p.lon),
                        y: round7(p.lat),
                    })
                })
                .collect::<std::result::Result<Vec<_>, _>>()
                .map(LineString::new)
                .map_err(OsmError::from)
        };
        let ext = ring(poly.exterior())?;
        let ints = poly
            .interiors()
            .iter()
            .map(ring)
            .collect::<Result<Vec<_>>>()?;
        out.push(Polygon::new(ext, ints));
    }
    Ok(MultiPolygon::new(out))
}

fn multipolygon_json(mp: &MultiPolygon<f64>) -> Value {
    let rings = |ls: &LineString<f64>| -> Value {
        Value::Array(ls.0.iter().map(|c| json!([c.x, c.y])).collect())
    };
    json!({
        "type": "MultiPolygon",
        "coordinates": mp.0.iter().map(|p| {
            let mut r = vec![rings(p.exterior())];
            r.extend(p.interiors().iter().map(rings));
            Value::Array(r)
        }).collect::<Vec<_>>(),
    })
}

// ---------------------------------------------------------------------------------------
// Census.

fn record(set: &PisteSet, d: &Domain, osm: &OpenSkiMap) -> DomainRecord {
    let pistes: Vec<&Piste> = d.pistes.iter().map(|&i| &set.pistes[i]).collect();
    let mut overlap: BTreeMap<String, f64> = BTreeMap::new();
    for p in &pistes {
        for a in osm.runs.of(set, p) {
            *overlap.entry(a).or_default() += p.length_m / 1000.0;
        }
    }
    let mut openskimap: Vec<OsmAreaOverlap> = overlap
        .into_iter()
        .map(|(id, km)| OsmAreaOverlap {
            name: osm.areas.get(&id).and_then(|a| a.name.clone()),
            id,
            km: (km * 100.0).round() / 100.0,
        })
        .collect();
    openskimap.sort_by(|a, b| b.km.total_cmp(&a.km).then(a.id.cmp(&b.id)));
    let (site_relation, name) = match &d.seed {
        Seed::Site { relation, name } => (Some(*relation), name.clone()),
        Seed::Network { .. } => (None, None),
    };
    let parent_sites = site_relation
        .and_then(|id| set.sites.iter().find(|s| s.id == id))
        .map(|s| {
            s.parents
                .iter()
                .map(|p| SiteRef {
                    id: *p,
                    name: set
                        .sites
                        .iter()
                        .find(|x| x.id == *p)
                        .and_then(|x| x.name.clone()),
                })
                .collect()
        })
        .unwrap_or_default();
    DomainRecord {
        parent_sites,
        id: d.id.clone(),
        name_source: name.as_ref().map(|_| "osm_site"),
        name,
        site_relation,
        merged: d
            .merged
            .iter()
            .map(|s| match s {
                Seed::Site { relation, .. } => format!("site-{relation}"),
                Seed::Network { way } => format!("net-{way}"),
            })
            .collect(),
        commune: None,
        point: d.representative_point(set),
        km_nordic: (d.km(set, |p| p.is_nordic()) * 100.0).round() / 100.0,
        km_hike_only: (d.km(set, |p| !p.is_nordic()) * 100.0).round() / 100.0,
        ways: d.pistes.len(),
        piste_names: d.relation_names(set).into_iter().collect(),
        openskimap,
        way_ids: pistes.iter().map(|p| p.way_id).collect(),
        alt_names: Vec::new(),
        footprint: None,
    }
}

/// Runs the census of France and writes the France layer; also writes the domain layer
/// of `site`.
pub fn build(site: &Site, layout: &DataLayout, dl: &Downloader) -> Result<CensusOutput> {
    let planet = load_planet(dl)?;
    let out = build_france_from(layout, dl, &planet, &CensusOptions::default())?;
    build_site_from(site, layout, &planet, &out)?;
    Ok(out)
}

/// Runs the census of France from a loaded extract and writes `data/out/france/domaines/`.
pub fn build_france_from(
    layout: &DataLayout,
    dl: &Downloader,
    planet: &Planet,
    opts: &CensusOptions,
) -> Result<CensusOutput> {
    let set = &planet.set;
    let t = std::time::Instant::now();
    let clustering = cluster(set, &opts.params);
    let cluster_s = t.elapsed().as_secs_f64();
    let osm = load_openskimap(dl)?;

    let mut all: Vec<DomainRecord> = clustering
        .domains
        .iter()
        .map(|d| record(set, d, &osm))
        .collect();
    let hike_only = all.iter().filter(|r| r.km_nordic == 0.0).count();
    all.retain(|r| r.km_nordic > 0.0);
    let candidates = all.len();

    // France only, commune of the representative point.
    let mut domains = Vec::new();
    for mut r in all {
        r.commune = fetch_commune(dl, r.point)?;
        if r.commune.is_some() {
            domains.push(r);
        }
    }
    // Names: OSM site, else OpenSkiMap area not built from another site, else commune.
    // An OpenSkiMap area names a domain when it covers at least half of it and was not
    // built from the site relation of another of our domains (« Queyras - Arvieux » does
    // not name Cervières).
    let site_domains: BTreeSet<i64> = domains.iter().filter_map(|r| r.site_relation).collect();
    for r in &mut domains {
        let total = r.km_nordic + r.km_hike_only;
        r.alt_names = r
            .openskimap
            .iter()
            .filter(|o| o.km >= 0.5 * total)
            .filter_map(|o| {
                let a = osm.areas.get(&o.id)?;
                let foreign = a
                    .site_relation
                    .is_some_and(|s| site_domains.contains(&s) && Some(s) != r.site_relation);
                (!foreign).then(|| a.name.clone()).flatten()
            })
            .collect();
        if r.name.is_some() {
            continue;
        }
        if let Some(n) = r.alt_names.first().cloned() {
            r.name = Some(n);
            r.name_source = Some("openskimap");
        } else if let Some(c) = &r.commune {
            r.name = Some(c.name.clone());
            r.name_source = Some("commune");
        }
    }
    // Footprints.
    let piste_of_way: HashMap<i64, usize> = set
        .pistes
        .iter()
        .enumerate()
        .map(|(i, p)| (p.way_id, i))
        .collect();
    for r in &mut domains {
        let ps: Vec<&Piste> = r
            .way_ids
            .iter()
            .map(|w| &set.pistes[piste_of_way[w]])
            .collect();
        r.footprint = Some(footprint(&ps, HULL_BUFFER_M)?);
    }

    // OpenSkiMap comparison.
    let fr_areas: Vec<&OpenSkiMapArea> = osm
        .areas
        .values()
        .filter(|a| a.countries.split(';').any(|c| c == "France"))
        .collect();
    let mut area_in_domains: BTreeMap<&str, Vec<(&str, f64)>> = BTreeMap::new();
    for r in &domains {
        for o in &r.openskimap {
            area_in_domains
                .entry(o.id.as_str())
                .or_default()
                .push((r.id.as_str(), o.km));
        }
    }
    let mut osm_matched = 0;
    let mut osm_split = Vec::new();
    let mut osm_unmatched = Vec::new();
    for a in &fr_areas {
        match area_in_domains.get(a.id.as_str()) {
            Some(v) => {
                let total: f64 = v.iter().map(|x| x.1).sum();
                let best = v.iter().map(|x| x.1).fold(0.0, f64::max);
                if best >= 0.5 * total {
                    osm_matched += 1;
                }
                let big: Vec<&(&str, f64)> = v.iter().filter(|x| x.1 >= 1.0).collect();
                if big.len() >= 2 {
                    osm_split.push(json!({
                        "openskimap_id": a.id,
                        "name": a.name,
                        "domains": big.iter().map(|(d, km)| json!({"domain": d, "km": km})).collect::<Vec<_>>(),
                    }));
                }
            }
            None => osm_unmatched
                .push(json!({"openskimap_id": a.id, "name": a.name, "nordic_km": a.nordic_km})),
        }
    }
    let ours_with_osm = domains.iter().filter(|r| !r.openskimap.is_empty()).count();

    // SkiRadar.
    let (skiradar, skiradar_sources) = load_skiradar(dl)?;
    let tree: rstar::RTree<rstar::primitives::GeomWithData<[f64; 2], usize>> =
        rstar::RTree::bulk_load(
            domains
                .iter()
                .enumerate()
                .flat_map(|(di, r)| {
                    let pw = &piste_of_way;
                    r.way_ids.iter().flat_map(move |w| {
                        set.pistes[pw[w]].points.iter().map(move |(_, p)| {
                            rstar::primitives::GeomWithData::new([p.lon, p.lat], di)
                        })
                    })
                })
                .collect(),
        );
    let mut skiradar_matches = Vec::new();
    let mut skiradar_matched = 0;
    for s in &skiradar {
        let best = tree
            .locate_in_envelope(&crate::graph::search_box(s.pos, SKIRADAR_MAX_M))
            .map(|q| {
                (
                    LonLat::new(q.geom()[0], q.geom()[1]).haversine_m(s.pos),
                    q.data,
                )
            })
            .filter(|(d, _)| *d <= SKIRADAR_MAX_M)
            .min_by(|a, b| a.0.total_cmp(&b.0));
        if best.is_some() {
            skiradar_matched += 1;
        }
        skiradar_matches.push(json!({
            "skiradar_id": s.id,
            "name": s.name,
            "url": s.url,
            "nordic_km": s.nordic_km,
            "domain": best.map(|(_, di)| domains[di].id.clone()),
            "domain_name": best.and_then(|(_, di)| domains[di].name.clone()),
            "distance_m": best.map(|(d, _)| d.round()),
        }));
    }

    // Nordic France (personal use only).
    let mut nf_summary = Value::Null;
    if opts.personal_sources {
        let (stations, nf_sources) = load_nordic_france(dl)?;
        let assigned = match_stations(&stations, &domains);
        let matched = assigned.iter().filter(|a| a.is_some()).count();
        let matches: Vec<Value> = stations
            .iter()
            .zip(&assigned)
            .map(|(st, a)| {
                json!({
                    "association": st.association,
                    "station_id": st.id,
                    "name": st.name,
                    "km_total": st.km_total,
                    "domain": a.map(|m| domains[m.domain].id.clone()),
                    "domain_name": a.and_then(|m| domains[m.domain].name.clone()),
                    "score": a.map(|m| (m.score * 100.0).round() / 100.0),
                    "shared": a.map(|m| m.shared),
                })
            })
            .collect();
        let dir = layout.out_root().join(FRANCE_DIR).join(LAYER);
        write_json(
            &dir.join("nordic_france.json"),
            &json!({
                "licence": "aucune licence publiée : usage personnel uniquement, ne pas redistribuer",
                "sources": nf_sources,
                "stations": stations.len(),
                "matched": matched,
                "matches": matches,
            }),
        )?;
        nf_summary = json!({
            "stations": stations.len(),
            "matched": matched,
            "rate": if stations.is_empty() { 0.0 } else { (matched as f64 / stations.len() as f64 * 1000.0).round() / 1000.0 },
        });
    }

    // Write the France layer.
    let dir = layout.out_root().join(FRANCE_DIR).join(LAYER);
    write_domain_files(&dir, &domains, None)?;
    let count_km = |km: f64| domains.iter().filter(|r| r.km_nordic >= km).count();
    let named = |src: &str| {
        domains
            .iter()
            .filter(|r| r.name_source == Some(src))
            .count()
    };
    let stats = json!({
        "cluster_seconds": (cluster_s * 100.0).round() / 100.0,
        "clusters_in_area": clustering.domains.len(),
        "clusters_hike_only_dropped": hike_only,
        "clusters_with_nordic": candidates,
        "domains_france": domains.len(),
        "domains_france_nordic_ge_1km": count_km(1.0),
        "domains_france_nordic_ge_5km": count_km(5.0),
        "km_nordic_france": (domains.iter().map(|r| r.km_nordic).sum::<f64>()).round(),
        "named_by_osm_site": named("osm_site"),
        "named_by_openskimap": named("openskimap"),
        "named_by_commune": named("commune"),
        "connectors": clustering.connectors,
        "liaison_bridges": clustering.liaison_bridges,
        "merges": clustering.merges.len(),
        "separations": clustering.separations.len(),
        "coherence_moves": clustering.coherence_moves,
        "openskimap": {
            "areas_france_nordic": fr_areas.len(),
            "areas_matched_to_one_domain": osm_matched,
            "rate": (osm_matched as f64 / fr_areas.len().max(1) as f64 * 1000.0).round() / 1000.0,
            "areas_without_any_of_our_ways": osm_unmatched.len(),
            "areas_split_over_several_domains": osm_split.len(),
            "our_domains_overlapping_an_area": ours_with_osm,
        },
        "skiradar": {
            "stations_with_nordic": skiradar.len(),
            "matched_within_m": SKIRADAR_MAX_M,
            "matched": skiradar_matched,
            "rate": (skiradar_matched as f64 / skiradar.len().max(1) as f64 * 1000.0).round() / 1000.0,
        },
        "nordic_france": nf_summary,
    });
    write_json(
        &dir.join("rapprochements.json"),
        &json!({
            "openskimap_split": osm_split,
            "openskimap_unmatched": osm_unmatched,
            "skiradar": {
                "licence": "réutilisation avec attribution « skiradar.fr » et lien vers la fiche",
                "matches": skiradar_matches,
            },
        }),
    )?;
    let mut sources = vec![planet.source.clone()];
    sources.extend(osm.sources.iter().cloned());
    sources.push(SourceRef::new(
        "geo.api.gouv.fr communes",
        "https://geo.api.gouv.fr/communes",
        "Licence Ouverte 2.0",
        "geo.api.gouv.fr (Etalab, IGN Admin Express)",
    ));
    sources.extend(skiradar_sources);
    let meta = layer_meta(
        FRANCE_DIR,
        planet,
        sources,
        &domains,
        opts,
        stats.clone(),
        true,
    );
    write_json(&dir.join(format!("{LAYER}.meta.json")), &meta)?;
    Ok(CensusOutput {
        dir,
        domains,
        stats,
    })
}

fn domain_properties(r: &DomainRecord) -> Value {
    let mut v = serde_json::to_value(r).unwrap_or(Value::Null);
    if let Value::Object(m) = &mut v {
        m.insert("lon".into(), json!(round7(r.point.lon)));
        m.insert("lat".into(), json!(round7(r.point.lat)));
    }
    v
}

fn write_domain_files(dir: &Path, domains: &[DomainRecord], only: Option<&BBox>) -> Result<usize> {
    let keep: Vec<&DomainRecord> = domains
        .iter()
        .filter(|r| only.is_none_or(|b| r.footprint.as_ref().is_some_and(|f| touches(f, b))))
        .collect();
    let polys: Vec<Value> = keep
        .iter()
        .map(|r| {
            feature(
                r.footprint
                    .as_ref()
                    .map(multipolygon_json)
                    .unwrap_or(Value::Null),
                domain_properties(r),
            )
        })
        .collect();
    write_geojson(&dir.join("domaines.geojson"), &polys)?;
    let points: Vec<Value> = keep
        .iter()
        .map(|r| {
            feature(
                point(r.point),
                json!({"id": r.id, "name": r.name, "km_nordic": r.km_nordic}),
            )
        })
        .collect();
    write_geojson(&dir.join("domaines_points.geojson"), &points)?;
    let ways: BTreeMap<&str, &Vec<i64>> =
        keep.iter().map(|r| (r.id.as_str(), &r.way_ids)).collect();
    write_json(&dir.join("domaines_ways.json"), &ways)?;
    Ok(keep.len())
}

fn touches(mp: &MultiPolygon<f64>, b: &BBox) -> bool {
    use geo::Intersects;
    let rect: geo_types::Rect<f64> = (*b).into();
    mp.intersects(&rect)
}

fn layer_meta(
    site: &str,
    planet: &Planet,
    sources: Vec<SourceRef>,
    domains: &[DomainRecord],
    opts: &CensusOptions,
    stats: Value,
    france: bool,
) -> LayerMeta {
    let mut files = vec![
        FileMeta::new("domaines.geojson", "domaines de ski de fond : enveloppe concave des pistes + tampon")
            .licence("ODbL 1.0 (OSM, OpenSkiMap) ; communes : Licence Ouverte 2.0")
            .count(domains.len())
            .properties_json(&json!({
                "id": "identifiant stable : site-<relation OSM site=piste> ou net-<plus petit way OSM du réseau>",
                "name": "nom affiché",
                "name_source": "osm_site (relation site=piste), openskimap (domaine OpenSkiMap non issu d'une autre relation site), commune (commune du point représentatif)",
                "site_relation": "relation OSM site=piste qui donne l'identité du domaine",
                "parent_sites": "relations site=piste englobantes (id, name) : la relation la plus précise donne le domaine",
                "merged": "cœurs de réseau sans relation site rattachés par une liaison courte",
                "commune": "commune du point représentatif : name, insee, departement",
                "km_nordic": "longueur des ways nordiques, km (chaque way une fois)",
                "km_hike_only": "longueur des ways seulement raquettes / piéton, km",
                "ways": "nombre de ways",
                "piste_names": "noms des relations de piste du domaine",
                "openskimap": "domaines OpenSkiMap recouvrant nos ways : id, name, km",
                "alt_names": "noms des domaines OpenSkiMap couvrant au moins la moitié du domaine (hors domaines issus d'une autre relation site)",
                "lon": "longitude du point représentatif (sur le réseau)",
                "lat": "latitude du point représentatif",
            })),
        FileMeta::new("domaines_points.geojson", "point représentatif de chaque domaine (point du réseau le plus proche du barycentre)")
            .licence("ODbL 1.0")
            .count(domains.len())
            .properties_json(&json!({"id": "identifiant", "name": "nom", "km_nordic": "km nordiques"})),
        FileMeta::new("domaines_ways.json", "ways OSM de chaque domaine (identifiant de domaine → liste d'identifiants de ways)")
            .licence("ODbL 1.0")
            .count(domains.len()),
    ];
    if france {
        files.push(FileMeta::new("rapprochements.json", "rapprochements : domaines OpenSkiMap partagés entre plusieurs de nos domaines ou sans aucun de nos ways ; stations SkiRadar et domaine le plus proche")
                .licence("ODbL 1.0 (OpenSkiMap) ; SkiRadar : attribution « skiradar.fr »"));
        if opts.personal_sources {
            files.push(FileMeta::new("nordic_france.json", "stations Nordic France (widget) et domaine rapproché par le nom ; usage personnel")
                    .licence("aucune licence publiée : usage personnel uniquement"));
        }
    }
    LayerMeta {
        tickets: vec!["P3 (#16)".to_owned()],
        valid_from: planet.valid_at.clone(),
        crs: Some("EPSG:4326 (WGS84) ; calculs d'enveloppe en Lambert-93 (EPSG:2154)".to_owned()),
        sources,
        files,
        parameters: json!({
            "grouping": opts.params,
            "hull_buffer_m": HULL_BUFFER_M,
            "skiradar_max_m": SKIRADAR_MAX_M,
            "personal_sources": opts.personal_sources,
            "france_filter": "commune française au point représentatif (geo.api.gouv.fr)",
        }),
        stats,
        ..LayerMeta::new(LAYER, site)
    }
}

/// Writes the domain layer of `site` from a census: domains whose footprint meets the
/// site box.
pub fn build_site_from(
    site: &Site,
    layout: &DataLayout,
    planet: &Planet,
    census: &CensusOutput,
) -> Result<PathBuf> {
    let dir = layout.layer_dir(site, LAYER)?;
    let n = write_domain_files(&dir, &census.domains, Some(&site.bbox))?;
    let kept: Vec<DomainRecord> = census
        .domains
        .iter()
        .filter(|r| r.footprint.as_ref().is_some_and(|f| touches(f, &site.bbox)))
        .cloned()
        .collect();
    let stats = json!({
        "domains": n,
        "list": kept.iter().map(|r| json!({"id": r.id, "name": r.name, "km_nordic": r.km_nordic, "km_hike_only": r.km_hike_only})).collect::<Vec<_>>(),
    });
    // Sources of the France layer, without SkiRadar (not used by the site layer).
    let mut sources = vec![planet.source.clone()];
    if let Ok(m) = LayerMeta::read(&census.dir.join(LayerMeta::file_name(LAYER))) {
        sources = m
            .sources
            .into_iter()
            .filter(|s| !s.name.starts_with("SkiRadar"))
            .collect();
    }
    let meta = layer_meta(
        site.id,
        planet,
        sources,
        &kept,
        &CensusOptions::default(),
        stats,
        false,
    );
    write_json(&dir.join(format!("{LAYER}.meta.json")), &meta)?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_and_scores() {
        let a = name_tokens("Cervières - Izoard");
        assert_eq!(
            a,
            BTreeSet::from(["cervieres".to_owned(), "izoard".to_owned()])
        );
        let b = name_tokens("Cervières");
        assert_eq!(name_score(&a, &b), 1.0);
        let c = name_tokens("Domaine Nordique de Sur-Lyand / Grand Colombier");
        assert!(c.contains("lyand") && c.contains("colombier") && !c.contains("domaine"));
        assert_eq!(
            name_score(
                &name_tokens("Chapelle des Bois"),
                &name_tokens("Chapelle-des-Bois")
            ),
            1.0
        );
        assert_eq!(name_score(&a, &name_tokens("Arvieux")), 0.0);
        assert_eq!(name_score(&BTreeSet::new(), &a), 0.0);
        assert_eq!(jaccard(&a, &b), 0.5);
        assert_eq!(jaccard(&BTreeSet::new(), &BTreeSet::new()), 0.0);
    }

    fn domain(id: &str, name: &str, dept: &str, km: f64) -> DomainRecord {
        DomainRecord {
            id: id.to_owned(),
            name: Some(name.to_owned()),
            name_source: Some("osm_site"),
            site_relation: None,
            parent_sites: vec![],
            merged: vec![],
            commune: Some(Commune {
                name: name.to_owned(),
                insee: "00000".to_owned(),
                departement: dept.to_owned(),
            }),
            point: LonLat::new(0.0, 0.0),
            km_nordic: km,
            km_hike_only: 0.0,
            ways: 1,
            piste_names: vec![],
            openskimap: vec![],
            alt_names: vec![],
            way_ids: vec![],
            footprint: None,
        }
    }

    #[test]
    fn stations_match_one_to_one_first() {
        let station = |assoc: &str, id, name: &str| NordicFranceStation {
            association: assoc.to_owned(),
            id,
            name: name.to_owned(),
            km_total: None,
        };
        let mut haut = domain("d1", "Espace Nordique du Haut vercors", "38", 80.0);
        haut.piste_names = vec!["La Draye".to_owned()];
        let domains = vec![
            haut,
            domain("d2", "Lans en Vercors", "38", 40.0),
            domain("d3", "Cervières", "05", 35.0),
        ];
        let stations = vec![
            station(
                "nordic-isere",
                1,
                "Espace nordique du Haut Vercors - Villard de Lans / Corrençon",
            ),
            station("nordic-isere", 2, "Lans en Vercors"),
            station("nordic-alpes-du-sud", 3, "Cervières - Izoard"),
            station("nordic-isere", 4, "La Draye"),
            station("savoie-nordic", 5, "Cervières"),
            station("espace-nordique-jurassien", 6, "Monts Jura"),
        ];
        let m = match_stations(&stations, &domains);
        assert_eq!(m[1].unwrap().domain, 1, "exact name wins");
        assert_eq!(m[0].unwrap().domain, 0, "second best, still free");
        assert_eq!(m[2].unwrap().domain, 2);
        assert!(!m[2].unwrap().shared);
        let draye = m[3].unwrap();
        assert_eq!(draye.domain, 0, "piste name contains the station name");
        assert!(draye.shared);
        assert!(m[4].is_none(), "wrong department");
        assert!(m[5].is_none(), "only generic words");
    }

    #[test]
    fn nordic_france_widget() {
        let html = r##"
        <div class="Weather-itemContainer " id="3167" data-lettrine="">
            <p class="Weather-name">Cervières - Izoard</p>
            <div class="Weather-pistes Weather-pistes--km">
                <svg><use xlink:href="#sign"/></svg>
                0/98km
            </div>
        </div>
        <div class="Weather-itemContainer " id="4774" data-lettrine="">
            <p class="Weather-name">Chapelle des Bois &amp; Risoux</p>
            <div class="Weather-pistes Weather-pistes--km">
                12/231.2km
            </div>
        </div>"##;
        let st = parse_nordic_france("x", html);
        assert_eq!(st.len(), 2);
        assert_eq!(st[0].id, 3167);
        assert_eq!(st[0].name, "Cervières - Izoard");
        assert_eq!(st[0].km_total, Some(98.0));
        assert_eq!(st[1].name, "Chapelle des Bois & Risoux");
        assert_eq!(st[1].km_total, Some(231.2));
    }

    #[test]
    fn openskimap_csv() {
        let areas = "name,countries,has_nordic,nordic_distance_km,id,sources\n\
            Chapelle-des-Bois,France,yes,84,a1,https://www.openstreetmap.org/relation/13681086 https://www.skimap.org/SkiAreas/view/914\n\
            Alpine,France,no,,a2,\n\
            ,Switzerland,yes,3,a3,\n";
        let a = parse_openskimap_areas(areas.as_bytes()).unwrap();
        assert_eq!(a.len(), 2);
        assert_eq!(a["a1"].site_relation, Some(13681086));
        assert_eq!(a["a1"].nordic_km, Some(84.0));
        assert_eq!(a["a3"].name, None);
        let runs = "name,uses,ski_area_ids,sources\n\
            x,nordic,a1,https://www.openstreetmap.org/way/10 https://www.openstreetmap.org/way/11\n\
            y,downhill,a2,https://www.openstreetmap.org/way/12\n\
            z,nordic;hike,a1;a3,https://www.openstreetmap.org/relation/5 https://www.openstreetmap.org/way/13\n";
        let r = parse_openskimap_runs(runs.as_bytes()).unwrap();
        assert_eq!(r.ways[&10], vec!["a1".to_owned()]);
        assert!(!r.ways.contains_key(&12));
        assert_eq!(r.ways[&13], vec!["a1".to_owned(), "a3".to_owned()]);
        assert_eq!(r.relations[&5], vec!["a1".to_owned(), "a3".to_owned()]);
    }

    #[test]
    fn skiradar_feed() {
        let v = json!({
            "totalPages": 2,
            "stations": [
                {"id": "abries", "name": "Abriès", "url": "https://skiradar.fr/stations/abries",
                 "geo": {"lat": 44.79, "lng": 6.92}, "nordique": {"km": 32}},
                {"id": "alpine", "name": "Alpine", "geo": {"lat": 45.0, "lng": 6.0}, "nordique": null}
            ]
        });
        let (s, pages) = parse_skiradar(&v).unwrap();
        assert_eq!(pages, 2);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].nordic_km, Some(32.0));
        assert!((s[0].pos.lon - 6.92).abs() < 1e-12);
    }

    #[test]
    fn footprint_of_a_square_loop() {
        use crate::pistes::{Grooming, Oneway};
        let pts: Vec<(i64, LonLat)> = [
            (6.0, 45.0),
            (6.01, 45.0),
            (6.01, 45.01),
            (6.0, 45.01),
            (6.0, 45.0),
        ]
        .iter()
        .enumerate()
        .map(|(i, (x, y))| (i as i64, LonLat::new(*x, *y)))
        .collect();
        let p = Piste {
            way_id: 1,
            uses: BTreeSet::new(),
            tagged: true,
            way_name: None,
            relations: vec![],
            difficulty: None,
            difficulty_max: None,
            difficulty_source: None,
            way_difficulty: None,
            grooming: Grooming::default(),
            oneway: Oneway::Unknown,
            lit: None,
            length_m: 0.0,
            points: pts,
        };
        let fp = footprint(&[&p], 150.0).unwrap();
        assert_eq!(fp.0.len(), 1);
        // The square (≈ 790 m × 1110 m) grown by 150 m on each side.
        let b = geo::BoundingRect::bounding_rect(&fp).unwrap();
        let dlat = 150.0 / crate::graph::M_PER_DEG_LAT;
        assert!((b.min().y - (45.0 - dlat)).abs() < 2e-4, "{:?}", b);
        assert!((b.max().y - (45.01 + dlat)).abs() < 2e-4, "{:?}", b);
        assert!(touches(&fp, &BBox::new(45.005, 6.005, 45.006, 6.006)));
        assert!(!touches(&fp, &BBox::new(46.0, 6.0, 46.1, 6.1)));
    }
}
