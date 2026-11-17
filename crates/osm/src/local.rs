//! Local OpenStreetMap extracts (X5, #139): the Geofabrik `.osm.pbf` of France and of the
//! neighbouring regions the site boxes reach, read in Rust, instead of the public Overpass
//! API (down or overloaded for hours at a time, 15/11/2026).
//!
//! The extracts go through the shared downloader (source [`SOURCE`], conditional requests,
//! cache of [`skimap_common::download::SourcePolicy`]); when Geofabrik cannot be reached,
//! the copy already in the cache is used, whatever its age. One scan of the extracts keeps
//! the elements every former Overpass query of the pipeline asked for ([`wanted`]: huts and
//! shelters of the refuges layer, points of interest and `site=piste` relations of the
//! stations layer), with their tags and position, in a derived index
//! (`cache/osm-local/index-v<N>-<hash>.jsonl.gz`, rebuilt when an extract changes). The
//! extracts are the regions of France and the neighbouring countries whose polygon (Geofabrik
//! catalogue `index-v1.json`) contains a point of a site box of the registry ([`select`]):
//! on 15/11/2026, 13 regions of France, Switzerland, Italy north-west, Belgium, Navarre,
//! Aragon and Catalonia, 4.9 GB, instead of the whole of France (5.1 GB) plus neighbours. The
//! layers then query the index by box, with the same tag filters as their former Overpass
//! queries, and get the same JSON as `out center tags` ([`Index::query`]).
//!
//! Differences with Overpass, by construction: a way or relation is in a box when its
//! bounding box meets the box (Overpass: when one of its segments does; the elements kept
//! are buildings and small areas, a few tens of metres wide); a relation's bounding box is
//! the one of its node and way members (sub-relations are not followed); `site=piste`
//! relations are kept for their tags only (no position).

use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use osmpbf::{BlobDecode, BlobReader, Element as PbfElement, RelMemberType};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use skimap_common::download::{CachePolicy, Downloader, Fetched, Request};
use skimap_common::geom::BBox;
use skimap_common::layout::DataLayout;
use skimap_common::meta::SourceRef;

use crate::error::{OsmError, Result};

/// Downloader source name.
pub const SOURCE: &str = "geofabrik";
/// Licence of the OSM data.
pub const LICENCE: &str = "ODbL 1.0";
/// Attribution to display.
pub const ATTRIBUTION: &str =
    "© les contributeurs d'OpenStreetMap (ODbL), extraits Geofabrik (download.geofabrik.de)";
/// Directory of the derived index, under the cache root.
pub const INDEX_DIR: &str = "osm-local";
/// Version of the index format and of [`wanted`]: a change rebuilds the index.
pub const INDEX_VERSION: u32 = 1;

/// Geofabrik catalogue of the extracts, with their polygons.
pub const CATALOGUE_URL: &str = "https://download.geofabrik.de/index-v1.json";
/// Extracts outside France that a site box may reach (ids of [`CATALOGUE_URL`]); the
/// regions of France (`parent: france`) are candidates too.
pub const NEIGHBOURS: &[&str] = &[
    "switzerland",
    "nord-ovest",
    "andorra",
    "cataluna",
    "aragon",
    "navarra",
    "pais-vasco",
    "baden-wuerttemberg",
    "saarland",
    "rheinland-pfalz",
    "belgium",
    "luxembourg",
    "monaco",
    "liechtenstein",
];
/// Margin added to each site box before choosing the extracts, degrees (the stations
/// layer queries the domain footprints plus about 6 km).
pub const BOX_MARGIN_DEG: f64 = 0.06;
/// Sample points per box side when choosing the extracts.
pub const BOX_SAMPLES: usize = 11;

/// One extract of the Geofabrik catalogue.
#[derive(Debug, Clone)]
pub struct Extract {
    /// Geofabrik id (`rhone-alpes`, `switzerland`…).
    pub id: String,
    /// `.osm.pbf` URL.
    pub url: String,
    /// Polygon of the extract (WGS84).
    pub polygon: geo::MultiPolygon<f64>,
}

/// Reads the candidate extracts ([`NEIGHBOURS`] and the regions of France) of the
/// Geofabrik catalogue.
pub fn candidates(catalogue: &Value) -> Result<Vec<Extract>> {
    let feats = catalogue
        .get("features")
        .and_then(Value::as_array)
        .ok_or_else(|| OsmError::format(SOURCE, "index-v1.json: no features"))?;
    let mut out = Vec::new();
    for f in feats {
        let p = &f["properties"];
        let (Some(id), Some(url)) = (
            p["id"].as_str(),
            p.pointer("/urls/pbf").and_then(Value::as_str),
        ) else {
            continue;
        };
        if !(p["parent"].as_str() == Some("france") || NEIGHBOURS.contains(&id)) {
            continue;
        }
        let Some(polygon) = multipolygon(&f["geometry"]) else {
            continue;
        };
        out.push(Extract {
            id: id.to_owned(),
            url: url.to_owned(),
            polygon,
        });
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

fn multipolygon(g: &Value) -> Option<geo::MultiPolygon<f64>> {
    let ring = |r: &Value| -> Option<geo::LineString<f64>> {
        Some(geo::LineString::from(
            r.as_array()?
                .iter()
                .filter_map(|c| Some((c.get(0)?.as_f64()?, c.get(1)?.as_f64()?)))
                .collect::<Vec<(f64, f64)>>(),
        ))
    };
    let poly = |p: &Value| -> Option<geo::Polygon<f64>> {
        let rings = p.as_array()?;
        let outer = ring(rings.first()?)?;
        let holes = rings[1..].iter().filter_map(ring).collect();
        Some(geo::Polygon::new(outer, holes))
    };
    match g["type"].as_str()? {
        "MultiPolygon" => Some(geo::MultiPolygon(
            g["coordinates"]
                .as_array()?
                .iter()
                .filter_map(poly)
                .collect(),
        )),
        "Polygon" => Some(geo::MultiPolygon(vec![poly(&g["coordinates"])?])),
        _ => None,
    }
}

/// The extracts covering `boxes` (each grown by [`BOX_MARGIN_DEG`], sampled on a
/// [`BOX_SAMPLES`]² grid): every candidate containing a sample point. Also returns the
/// sample points no candidate contains (a box outside the known regions).
pub fn select(candidates: &[Extract], boxes: &[BBox]) -> (Vec<Extract>, usize) {
    use geo::{BoundingRect, Contains};
    let rects: Vec<Option<geo::Rect<f64>>> = candidates
        .iter()
        .map(|c| c.polygon.bounding_rect())
        .collect();
    let mut used = vec![false; candidates.len()];
    let mut uncovered = 0;
    let n = BOX_SAMPLES.max(2);
    for b in boxes {
        let (s, w) = (b.south - BOX_MARGIN_DEG, b.west - BOX_MARGIN_DEG);
        let (nn, e) = (b.north + BOX_MARGIN_DEG, b.east + BOX_MARGIN_DEG);
        for i in 0..n {
            for j in 0..n {
                let x = w + (e - w) * i as f64 / (n - 1) as f64;
                let y = s + (nn - s) * j as f64 / (n - 1) as f64;
                let pt = geo::Point::new(x, y);
                let mut hit = false;
                for (k, c) in candidates.iter().enumerate() {
                    let inside_rect = rects[k].is_some_and(|r| {
                        x >= r.min().x && x <= r.max().x && y >= r.min().y && y <= r.max().y
                    });
                    if inside_rect && c.polygon.contains(&pt) {
                        used[k] = true;
                        hit = true;
                    }
                }
                if !hit {
                    uncovered += 1;
                }
            }
        }
    }
    let chosen = candidates
        .iter()
        .zip(used)
        .filter(|(_, u)| *u)
        .map(|(c, _)| c.clone())
        .collect();
    (chosen, uncovered)
}

// --- Tag filters (the former Overpass queries) -----------------------------------------

/// Huts and shelters of the refuges layer, formerly
/// `nwr["tourism"~"^(alpine_hut|wilderness_hut)$"]; nwr["amenity"="shelter"];
/// nwr["ref:refuges.info"]`.
pub fn is_hut<'a>(get: impl Fn(&str) -> Option<&'a str>) -> bool {
    matches!(get("tourism"), Some("alpine_hut" | "wilderness_hut"))
        || get("amenity") == Some("shelter")
        || get("ref:refuges.info").is_some()
}

/// Far categories of the stations layer (`skimap_stations::pois::query`), formerly
/// `nwr["emergency"~"^(mountain_rescue|ambulance_station)$"];
/// nwr["amenity"~"^(mountain_rescue|fire_station|ski_school)$"];
/// nwr["tourism"="information"]["information"="office"]; nwr["office"="ski_school"]`.
pub fn is_station_far<'a>(get: impl Fn(&str) -> Option<&'a str>) -> bool {
    matches!(
        get("emergency"),
        Some("mountain_rescue" | "ambulance_station")
    ) || matches!(
        get("amenity"),
        Some("mountain_rescue" | "fire_station" | "ski_school")
    ) || (get("tourism") == Some("information") && get("information") == Some("office"))
        || get("office") == Some("ski_school")
}

/// Words of the name search of the stations layer (case-insensitive), formerly
/// `nwr["name"~"foyer|caisse|billetterie|point de vente|accueil nordique|secours|pisteur|^ESF|cole d[ue] ski",i]`.
pub const NAME_WORDS: &[&str] = &[
    "foyer",
    "caisse",
    "billetterie",
    "point de vente",
    "accueil nordique",
    "secours",
    "pisteur",
    "cole de ski",
    "cole du ski",
];

/// Whether a name matches the name search of the stations layer ([`NAME_WORDS`], or a
/// name starting with « ESF »).
pub fn name_matches(name: &str) -> bool {
    let n = name.to_lowercase();
    n.starts_with("esf") || NAME_WORDS.iter().any(|w| n.contains(w))
}

/// Near categories of the stations layer (`skimap_stations::pois::query_names`), formerly
/// `nwr["emergency"~"^(first_aid|rescue_station|defibrillator|phone|access_point)$"];
/// nwr["amenity"~"^(first_aid|parking)$"]; nwr["healthcare"="first_aid"];
/// nwr["shop"="ticket"]; nwr["vending"~"ski"]` and the name search ([`name_matches`]).
pub fn is_station_near<'a>(get: impl Fn(&str) -> Option<&'a str>) -> bool {
    matches!(
        get("emergency"),
        Some("first_aid" | "rescue_station" | "defibrillator" | "phone" | "access_point")
    ) || matches!(get("amenity"), Some("first_aid" | "parking"))
        || get("healthcare") == Some("first_aid")
        || get("shop") == Some("ticket")
        || get("vending").is_some_and(|v| v.contains("ski"))
        || get("name").is_some_and(name_matches)
}

/// `site=piste` relations (station contacts, formerly `rel(id:…);out tags;`).
pub fn is_site_relation<'a>(get: impl Fn(&str) -> Option<&'a str>) -> bool {
    get("site") == Some("piste")
}

/// What the index keeps: the union of the filters above.
pub fn wanted<'a>(get: impl Fn(&str) -> Option<&'a str> + Copy) -> bool {
    is_hut(get) || is_station_far(get) || is_station_near(get) || is_site_relation(get)
}

// --- Index -------------------------------------------------------------------------------

/// OSM element type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// Node.
    Node,
    /// Way.
    Way,
    /// Relation.
    Relation,
}

impl Kind {
    /// Overpass / OSM name.
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Node => "node",
            Kind::Way => "way",
            Kind::Relation => "relation",
        }
    }
}

/// One element of the index.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Element {
    /// Element type.
    #[serde(rename = "t")]
    pub kind: Kind,
    /// OSM id.
    pub id: i64,
    /// Bounding box, `[south, west, north, east]` in degrees ×10⁷ (the PBF resolution);
    /// `None` for an element without a known position.
    #[serde(rename = "b", default, skip_serializing_if = "Option::is_none")]
    pub bbox: Option<[i32; 4]>,
    /// Every tag of the element.
    #[serde(rename = "g")]
    pub tags: BTreeMap<String, String>,
}

impl Element {
    /// Tag lookup.
    pub fn get(&self, k: &str) -> Option<&str> {
        self.tags.get(k).map(String::as_str)
    }

    /// Whether the element meets a box (bounding boxes).
    pub fn meets(&self, b: &BBox) -> bool {
        let Some([s, w, n, e]) = self.bbox else {
            return false;
        };
        f64::from(n) / 1e7 >= b.south
            && f64::from(s) / 1e7 <= b.north
            && f64::from(e) / 1e7 >= b.west
            && f64::from(w) / 1e7 <= b.east
    }

    /// The element as Overpass writes it with `out center tags`.
    pub fn to_overpass(&self) -> Value {
        let mut v = json!({"type": self.kind.as_str(), "id": self.id});
        if let Some([s, w, n, e]) = self.bbox {
            // Overpass: centre of the bounding box, 7 decimals.
            let lat = ((f64::from(s) + f64::from(n)) / 2.0).round() / 1e7;
            let lon = ((f64::from(w) + f64::from(e)) / 2.0).round() / 1e7;
            if self.kind == Kind::Node {
                v["lat"] = json!(lat);
                v["lon"] = json!(lon);
            } else {
                v["center"] = json!({"lat": lat, "lon": lon});
            }
        }
        if !self.tags.is_empty() {
            v["tags"] = json!(self.tags);
        }
        v
    }
}

/// Header line of the index file.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Header {
    version: u32,
    key: String,
    /// Replication timestamp of each extract (RFC 3339), by name.
    timestamps: BTreeMap<String, Option<String>>,
    elements: usize,
}

/// The index of the kept elements of every extract.
#[derive(Debug)]
pub struct Index {
    /// Elements, sorted by type and id, without duplicates (the extracts overlap at the
    /// borders).
    pub elements: Vec<Element>,
    /// One source per extract (URL, ETag, sha256, download date, OSM data date).
    pub sources: Vec<SourceRef>,
    /// Bounding box of each extract polygon (same order as `sources`).
    pub extents: Vec<BBox>,
    /// Oldest replication timestamp of the extracts (date of the OSM data), RFC 3339.
    pub valid_at: Option<String>,
    /// Index file.
    pub path: PathBuf,
}

impl Index {
    /// Sources of the extracts whose polygon's bounding box meets `b`.
    pub fn sources_for(&self, b: &BBox) -> Vec<SourceRef> {
        self.sources
            .iter()
            .zip(&self.extents)
            .filter(|(_, x)| {
                x.north >= b.south && x.south <= b.north && x.east >= b.west && x.west <= b.east
            })
            .map(|(s, _)| s.clone())
            .collect()
    }

    /// Elements meeting one of `boxes` and passing `filter`, as an Overpass answer
    /// (`{"elements": [...]}`, `out center tags`).
    pub fn query(&self, boxes: &[BBox], filter: impl Fn(&Element) -> bool) -> Value {
        let elements: Vec<Value> = self
            .elements
            .iter()
            .filter(|e| boxes.iter().any(|b| e.meets(b)) && filter(e))
            .map(Element::to_overpass)
            .collect();
        json!({ "elements": elements, "generator": "skimap-osm local index" })
    }

    /// Relations by id, as an Overpass answer (`rel(id:…);out tags;`).
    pub fn relations(&self, ids: &[i64]) -> Value {
        let elements: Vec<Value> = ids
            .iter()
            .filter_map(|id| {
                self.elements
                    .binary_search_by(|e| (e.kind, e.id).cmp(&(Kind::Relation, *id)))
                    .ok()
                    .map(|i| {
                        let e = &self.elements[i];
                        json!({"type": "relation", "id": e.id, "tags": e.tags})
                    })
            })
            .collect();
        json!({ "elements": elements, "generator": "skimap-osm local index" })
    }
}

/// Index of the process, shared by every site of a run (`build-all` builds many sites
/// in parallel): checked once per [`RECHECK`], built or read once per set of extracts.
static LOADED: Mutex<Option<Loaded>> = Mutex::new(None);

struct Loaded {
    key: String,
    at: std::time::Instant,
    index: Arc<Index>,
}

/// Within one process, the extracts are revalidated at most this often.
pub const RECHECK: std::time::Duration = std::time::Duration::from_secs(6 * 3600);

/// Fetches a Geofabrik document; when the request fails (Geofabrik unreachable), the
/// cached copy is used whatever its age.
fn fetch_or_cached(dl: &Downloader, url: &str) -> Result<Fetched> {
    match dl.fetch(&Request::get(SOURCE, url)) {
        Ok(f) => Ok(f),
        Err(e) => match dl.fetch(&Request::get(SOURCE, url).cache(CachePolicy::Offline)) {
            Ok(f) => {
                tracing::warn!(url, error = %e, "geofabrik unreachable, cached copy used");
                Ok(f)
            }
            Err(_) => Err(e.into()),
        },
    }
}

/// Boxes the index must cover: every site of the registry (the evaluation sites when
/// there is no registry yet).
fn registry_boxes(layout: &DataLayout) -> Vec<BBox> {
    let reg = skimap_common::site::Registry::load(layout)
        .unwrap_or_else(|_| skimap_common::site::Registry::evaluation());
    let mut boxes: Vec<BBox> = reg.all().iter().map(|s| s.bbox).collect();
    boxes.extend(skimap_common::site::SITES.iter().map(|s| s.bbox));
    boxes
}

/// Downloads (or revalidates) the extracts covering the registry and returns the index,
/// built if needed.
pub fn load(layout: &DataLayout, dl: &Downloader) -> Result<Arc<Index>> {
    let mut loaded = LOADED.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(l) = loaded.as_ref()
        && l.at.elapsed() < RECHECK
    {
        return Ok(l.index.clone());
    }
    let cat = fetch_or_cached(dl, CATALOGUE_URL)?;
    let (extracts, uncovered) = select(&candidates(&cat.json()?)?, &registry_boxes(layout));
    if uncovered > 0 {
        tracing::warn!(
            uncovered,
            "sample points of site boxes outside every candidate extract (no OSM data there)"
        );
    }
    let mut fetched = Vec::with_capacity(extracts.len());
    for x in &extracts {
        fetched.push((x.id.as_str(), fetch_or_cached(dl, &x.url)?));
    }
    let key = index_key(&fetched);
    if let Some(l) = loaded.as_mut()
        && l.key == key
    {
        l.at = std::time::Instant::now();
        return Ok(l.index.clone());
    }
    let dir = layout.cache_dir(INDEX_DIR)?;
    DataLayout::ensure(&dir)?;
    let path = dir.join(format!("index-v{INDEX_VERSION}-{}.jsonl.gz", &key[..16]));
    let t = std::time::Instant::now();
    let (header, elements) = match read_index(&path, &key) {
        Ok(x) => {
            remove_other_indexes(&dir, &path);
            x
        }
        Err(e) => {
            if path.exists() {
                tracing::warn!(path = %path.display(), error = %e, "index unreadable, rebuilt");
            }
            let mut timestamps = BTreeMap::new();
            let mut all: Vec<Element> = Vec::new();
            for ((name, f), x) in fetched.iter_mut().zip(&extracts) {
                let (ts, els) = match scan(&f.path) {
                    Ok(v) => v,
                    Err(e) => {
                        // A truncated or corrupted copy: downloaded again once.
                        tracing::warn!(extract = %name, error = %e, "extract unreadable, downloaded again");
                        *f = dl.fetch(&Request::get(SOURCE, &x.url).cache(CachePolicy::Reload))?;
                        scan(&f.path)?
                    }
                };
                timestamps.insert((*name).to_owned(), ts);
                all.extend(els);
            }
            all.sort_by_key(|e| (e.kind, e.id));
            all.dedup_by_key(|e| (e.kind, e.id));
            let header = Header {
                version: INDEX_VERSION,
                key: index_key(&fetched),
                timestamps,
                elements: all.len(),
            };
            write_index(&path, &header, &all)?;
            remove_other_indexes(&dir, &path);
            (header, all)
        }
    };
    tracing::info!(
        elapsed_ms = t.elapsed().as_millis() as u64,
        elements = elements.len(),
        extracts = fetched.len(),
        path = %path.display(),
        "local OSM index ready"
    );
    let valid_at = header.timestamps.values().flatten().min().cloned();
    let sources = fetched
        .iter()
        .map(|(name, f)| {
            let mut s = SourceRef::from_fetched(
                format!("OpenStreetMap, extrait Geofabrik {name}"),
                LICENCE,
                ATTRIBUTION,
                f,
            );
            s.edition = header.timestamps.get(*name).cloned().flatten();
            s
        })
        .collect();
    let extents = extracts
        .iter()
        .map(|x| {
            use geo::BoundingRect;
            x.polygon
                .bounding_rect()
                .map_or(BBox::new(-90.0, -180.0, 90.0, 180.0), |r| {
                    BBox::new(r.min().y, r.min().x, r.max().y, r.max().x)
                })
        })
        .collect();
    let index = Arc::new(Index {
        elements,
        sources,
        extents,
        valid_at,
        path,
    });
    *loaded = Some(Loaded {
        key,
        at: std::time::Instant::now(),
        index: index.clone(),
    });
    Ok(index)
}

/// Key of a set of extracts: the index version and the sha256 of every body.
fn index_key(fetched: &[(&str, Fetched)]) -> String {
    use std::hash::{Hash, Hasher};
    // Not a security hash: two 64-bit hashes of the sha256 list are enough to name a file.
    let mut parts = vec![format!("v{INDEX_VERSION}")];
    for (name, f) in fetched {
        parts.push(format!("{name}:{}", f.meta.sha256));
    }
    let joined = parts.join("|");
    let mut h1 = std::collections::hash_map::DefaultHasher::new();
    joined.hash(&mut h1);
    let mut h2 = std::collections::hash_map::DefaultHasher::new();
    (joined.len(), &joined).hash(&mut h2);
    format!("{:016x}{:016x}", h1.finish(), h2.finish())
}

fn read_index(path: &Path, key: &str) -> Result<(Header, Vec<Element>)> {
    let f = std::fs::File::open(path).map_err(|e| OsmError::io(path, e))?;
    let r = BufReader::new(flate2::read::GzDecoder::new(f));
    let mut lines = r.lines();
    let bad = |m: String| OsmError::format(INDEX_DIR, format!("{}: {m}", path.display()));
    let first = lines
        .next()
        .ok_or_else(|| bad("empty".into()))?
        .map_err(|e| OsmError::io(path, e))?;
    let header: Header = serde_json::from_str(&first).map_err(|e| bad(e.to_string()))?;
    if header.version != INDEX_VERSION || header.key != key {
        return Err(bad("other version or extracts".into()));
    }
    let mut elements = Vec::with_capacity(header.elements);
    for l in lines {
        let l = l.map_err(|e| OsmError::io(path, e))?;
        elements.push(serde_json::from_str(&l).map_err(|e| bad(e.to_string()))?);
    }
    if elements.len() != header.elements {
        return Err(bad(format!(
            "{} elements, {} announced (truncated)",
            elements.len(),
            header.elements
        )));
    }
    Ok((header, elements))
}

/// Writes the index atomically (temporary file, fsync, rename).
fn write_index(path: &Path, header: &Header, elements: &[Element]) -> Result<()> {
    let f = skimap_common::output::AtomicFile::create(path)?;
    let tmp = f.tmp().to_owned();
    let io = |e| OsmError::io(&tmp, e);
    let mut w = flate2::write::GzEncoder::new(f, flate2::Compression::fast());
    let line = |w: &mut dyn Write, v: String| -> std::io::Result<()> {
        w.write_all(v.as_bytes())?;
        w.write_all(b"\n")
    };
    let ser = |e: serde_json::Error| OsmError::format(INDEX_DIR, e.to_string());
    line(&mut w, serde_json::to_string(header).map_err(ser)?).map_err(io)?;
    for e in elements {
        line(&mut w, serde_json::to_string(e).map_err(ser)?).map_err(io)?;
    }
    w.finish().map_err(io)?.commit()?;
    Ok(())
}

/// Indexes not used for this long are removed (several data directories may share one
/// cache, each with its registry and so its set of extracts).
pub const INDEX_UNUSED_DAYS: u64 = 14;

/// Removes the indexes unused for [`INDEX_UNUSED_DAYS`] and the temporary files of
/// interrupted writes older than an hour; marks `keep` as used.
fn remove_other_indexes(dir: &Path, keep: &Path) {
    // Read-only: the owner may set the times; the content is never opened for writing.
    if let Ok(f) = std::fs::File::open(keep) {
        let _ = f.set_modified(std::time::SystemTime::now());
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let age = |p: &Path| {
        std::fs::metadata(p)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .unwrap_or_default()
    };
    for e in rd.flatten() {
        let p = e.path();
        let Some(n) = p.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let old_index = n.starts_with("index-")
            && !n.contains(".tmp")
            && age(&p).as_secs() > INDEX_UNUSED_DAYS * 86_400;
        let orphan = n.contains(".tmp") && age(&p).as_secs() > 3600;
        if p != keep && (old_index || orphan) {
            let _ = std::fs::remove_file(&p);
        }
    }
}

// --- PBF scan ----------------------------------------------------------------------------

/// Elements kept by the first pass.
#[derive(Default)]
struct Pass1 {
    nodes: Vec<Element>,
    /// Ways: element (no bbox yet) and node refs.
    ways: Vec<(Element, Vec<i64>)>,
    /// Relations: element and members (type, id).
    rels: Vec<(Element, Vec<(Kind, i64)>)>,
}

impl Pass1 {
    fn merge(mut self, o: Pass1) -> Pass1 {
        self.nodes.extend(o.nodes);
        self.ways.extend(o.ways);
        self.rels.extend(o.rels);
        self
    }
}

/// Second and third passes: coordinates of nodes, refs of ways.
#[derive(Default)]
struct Lookup {
    nodes: Vec<(i64, i32, i32)>,
    ways: Vec<(i64, Vec<i64>)>,
}

impl Lookup {
    fn merge(mut self, o: Lookup) -> Lookup {
        self.nodes.extend(o.nodes);
        self.ways.extend(o.ways);
        self
    }
}

fn pbf_err(path: &Path, e: osmpbf::Error) -> OsmError {
    OsmError::format(SOURCE, format!("{}: {e}", path.display()))
}

fn tags_of<'a>(it: impl Iterator<Item = (&'a str, &'a str)>) -> Vec<(&'a str, &'a str)> {
    it.collect()
}

fn lookup<'a>(t: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<&'a str> + Copy {
    move |k| t.iter().find(|(kk, _)| *kk == k).map(|(_, v)| *v)
}

fn owned(t: &[(&str, &str)]) -> BTreeMap<String, String> {
    t.iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

/// Runs `f` on every data block of the file, in parallel, and reduces the results.
fn par_blocks<T, F, R>(path: &Path, identity: fn() -> T, f: F, reduce: R) -> Result<T>
where
    T: Send,
    F: Fn(&osmpbf::PrimitiveBlock) -> T + Sync + Send,
    R: Fn(T, T) -> T + Sync + Send,
{
    let reader = BlobReader::from_path(path).map_err(|e| pbf_err(path, e))?;
    reader
        .par_bridge()
        .map(|blob| -> Result<T> {
            let blob = blob.map_err(|e| pbf_err(path, e))?;
            match blob.decode().map_err(|e| pbf_err(path, e))? {
                BlobDecode::OsmData(block) => Ok(f(&block)),
                _ => Ok(identity()),
            }
        })
        .try_reduce(identity, |a, b| Ok(reduce(a, b)))
}

/// Replication timestamp of the extract (header block), RFC 3339.
fn timestamp(path: &Path) -> Result<Option<String>> {
    let reader = BlobReader::from_path(path).map_err(|e| pbf_err(path, e))?;
    for blob in reader {
        let blob = blob.map_err(|e| pbf_err(path, e))?;
        if let BlobDecode::OsmHeader(h) = blob.decode().map_err(|e| pbf_err(path, e))? {
            return Ok(h
                .osmosis_replication_timestamp()
                .and_then(skimap_common::meta::unix_to_rfc3339));
        }
    }
    Ok(None)
}

/// Reads one extract: the [`wanted`] elements with their bounding boxes.
pub fn scan(path: &Path) -> Result<(Option<String>, Vec<Element>)> {
    let t = std::time::Instant::now();
    let ts = timestamp(path)?;
    // Pass 1: kept elements.
    let p1 = par_blocks(
        path,
        Pass1::default,
        |block| {
            let mut out = Pass1::default();
            block.for_each_element(|el| match el {
                PbfElement::Node(n) => {
                    let t = tags_of(n.tags());
                    if !t.is_empty() && wanted(lookup(&t)) {
                        let (la, lo) = (n.decimicro_lat(), n.decimicro_lon());
                        out.nodes.push(Element {
                            kind: Kind::Node,
                            id: n.id(),
                            bbox: Some([la, lo, la, lo]),
                            tags: owned(&t),
                        });
                    }
                }
                PbfElement::DenseNode(n) => {
                    let t = tags_of(n.tags());
                    if !t.is_empty() && wanted(lookup(&t)) {
                        let (la, lo) = (n.decimicro_lat(), n.decimicro_lon());
                        out.nodes.push(Element {
                            kind: Kind::Node,
                            id: n.id(),
                            bbox: Some([la, lo, la, lo]),
                            tags: owned(&t),
                        });
                    }
                }
                PbfElement::Way(w) => {
                    let t = tags_of(w.tags());
                    if !t.is_empty() && wanted(lookup(&t)) {
                        out.ways.push((
                            Element {
                                kind: Kind::Way,
                                id: w.id(),
                                bbox: None,
                                tags: owned(&t),
                            },
                            w.refs().collect(),
                        ));
                    }
                }
                PbfElement::Relation(r) => {
                    let t = tags_of(r.tags());
                    if !t.is_empty() && wanted(lookup(&t)) {
                        // `site=piste` relations: tags only (their members are pistes).
                        let geometry = {
                            let g = lookup(&t);
                            is_hut(g) || is_station_far(g) || is_station_near(g)
                        };
                        let members = if geometry {
                            r.members()
                                .filter_map(|m| match m.member_type {
                                    RelMemberType::Node => Some((Kind::Node, m.member_id)),
                                    RelMemberType::Way => Some((Kind::Way, m.member_id)),
                                    RelMemberType::Relation => None,
                                })
                                .collect()
                        } else {
                            Vec::new()
                        };
                        out.rels.push((
                            Element {
                                kind: Kind::Relation,
                                id: r.id(),
                                bbox: None,
                                tags: owned(&t),
                            },
                            members,
                        ));
                    }
                }
            });
            out
        },
        Pass1::merge,
    )?;

    // Pass 2: refs of the relation member ways that are not kept ways, and coordinates
    // of the nodes of the kept ways and of the relation member nodes.
    let mut way_refs: HashMap<i64, Vec<i64>> =
        p1.ways.iter().map(|(e, r)| (e.id, r.clone())).collect();
    let mut want_ways: Vec<i64> = p1
        .rels
        .iter()
        .flat_map(|(_, m)| m.iter())
        .filter(|(k, id)| *k == Kind::Way && !way_refs.contains_key(id))
        .map(|(_, id)| *id)
        .collect();
    want_ways.sort_unstable();
    want_ways.dedup();
    let mut want_nodes: Vec<i64> = p1
        .ways
        .iter()
        .flat_map(|(_, r)| r.iter().copied())
        .chain(
            p1.rels
                .iter()
                .flat_map(|(_, m)| m.iter())
                .filter(|(k, _)| *k == Kind::Node)
                .map(|(_, id)| *id),
        )
        .collect();
    want_nodes.sort_unstable();
    want_nodes.dedup();
    let l2 = lookup_pass(path, &want_nodes, &want_ways)?;
    let mut coords: HashMap<i64, (i32, i32)> = l2
        .nodes
        .into_iter()
        .map(|(id, a, o)| (id, (a, o)))
        .collect();
    let mut more: Vec<i64> = Vec::new();
    for (id, refs) in l2.ways {
        more.extend(refs.iter().filter(|n| !coords.contains_key(n)));
        way_refs.insert(id, refs);
    }
    // Pass 3: nodes of those member ways.
    more.sort_unstable();
    more.dedup();
    if !more.is_empty() {
        let l3 = lookup_pass(path, &more, &[])?;
        coords.extend(l3.nodes.into_iter().map(|(id, a, o)| (id, (a, o))));
    }

    let bbox_of = |nodes: &mut dyn Iterator<Item = i64>| -> Option<[i32; 4]> {
        let mut b: Option<[i32; 4]> = None;
        for n in nodes {
            if let Some(&(la, lo)) = coords.get(&n) {
                b = Some(match b {
                    None => [la, lo, la, lo],
                    Some([s, w, nn, e]) => [s.min(la), w.min(lo), nn.max(la), e.max(lo)],
                });
            }
        }
        b
    };
    let mut out = p1.nodes;
    for (mut e, refs) in p1.ways {
        e.bbox = bbox_of(&mut refs.iter().copied());
        out.push(e);
    }
    for (mut e, members) in p1.rels {
        let mut it = members.iter().flat_map(|(k, id)| match k {
            Kind::Node => vec![*id],
            Kind::Way => way_refs.get(id).cloned().unwrap_or_default(),
            Kind::Relation => Vec::new(),
        });
        e.bbox = bbox_of(&mut it);
        out.push(e);
    }
    out.sort_by_key(|e| (e.kind, e.id));
    tracing::info!(
        path = %path.display(),
        elements = out.len(),
        elapsed_ms = t.elapsed().as_millis() as u64,
        "extract scanned"
    );
    Ok((ts, out))
}

/// Coordinates of `nodes` and refs of `ways` (both sorted).
fn lookup_pass(path: &Path, nodes: &[i64], ways: &[i64]) -> Result<Lookup> {
    par_blocks(
        path,
        Lookup::default,
        |block| {
            let mut out = Lookup::default();
            block.for_each_element(|el| match el {
                PbfElement::Node(n) => {
                    if nodes.binary_search(&n.id()).is_ok() {
                        out.nodes
                            .push((n.id(), n.decimicro_lat(), n.decimicro_lon()));
                    }
                }
                PbfElement::DenseNode(n) => {
                    if nodes.binary_search(&n.id()).is_ok() {
                        out.nodes
                            .push((n.id(), n.decimicro_lat(), n.decimicro_lon()));
                    }
                }
                PbfElement::Way(w) => {
                    if !ways.is_empty() && ways.binary_search(&w.id()).is_ok() {
                        out.ways.push((w.id(), w.refs().collect()));
                    }
                }
                PbfElement::Relation(_) => {}
            });
            out
        },
        Lookup::merge,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn el(kind: Kind, id: i64, bbox: [i32; 4], tags: &[(&str, &str)]) -> Element {
        Element {
            kind,
            id,
            bbox: Some(bbox),
            tags: tags
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
        }
    }

    fn get<'a>(t: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<&'a str> + Copy {
        lookup(t)
    }

    #[test]
    fn filters_match_the_former_queries() {
        assert!(is_hut(get(&[("tourism", "alpine_hut")])));
        assert!(is_hut(get(&[("amenity", "shelter")])));
        assert!(is_hut(get(&[("ref:refuges.info", "12")])));
        assert!(!is_hut(get(&[("tourism", "hotel")])));
        assert!(is_station_far(get(&[
            ("tourism", "information"),
            ("information", "office")
        ])));
        assert!(!is_station_far(get(&[
            ("tourism", "information"),
            ("information", "board")
        ])));
        assert!(is_station_far(get(&[("amenity", "fire_station")])));
        assert!(!is_station_far(get(&[("emergency", "mountain_rescue;x")])));
        assert!(is_station_near(get(&[("amenity", "parking")])));
        assert!(is_station_near(get(&[("vending", "ski_pass")])));
        assert!(
            !is_station_near(get(&[("vending", "SKI")])),
            "case-sensitive"
        );
        assert!(is_station_near(get(&[("name", "ESF Chapelle")])));
        assert!(is_station_near(get(&[("name", "esf")])));
        assert!(!is_station_near(get(&[("name", "Résf")])), "^ESF");
        assert!(is_station_near(get(&[("name", "École du ski français")])));
        assert!(is_station_near(get(&[("name", "Poste de SECOURS")])));
        assert!(is_station_near(get(&[("name", "Foyer nordique")])));
        assert!(!is_station_near(get(&[("name", "Chalet")])));
        assert!(is_site_relation(get(&[
            ("type", "site"),
            ("site", "piste")
        ])));
        assert!(wanted(get(&[("amenity", "shelter")])));
        assert!(!wanted(get(&[("highway", "path")])));
    }

    #[test]
    fn query_by_box_and_overpass_shape() {
        let idx = Index {
            elements: vec![
                el(
                    Kind::Node,
                    1,
                    [448_500_000, 68_100_000, 448_500_000, 68_100_000],
                    &[("tourism", "alpine_hut")],
                ),
                el(
                    Kind::Way,
                    2,
                    [466_000_000, 61_000_000, 466_000_010, 61_000_020],
                    &[("amenity", "shelter")],
                ),
                el(
                    Kind::Relation,
                    3,
                    [0, 0, 0, 0],
                    &[("type", "site"), ("site", "piste")],
                ),
            ],
            sources: vec![],
            extents: vec![],
            valid_at: None,
            path: PathBuf::new(),
        };
        let cerv = BBox::new(44.77, 6.66, 44.92, 6.86);
        let v = idx.query(&[cerv], |e| is_hut(|k| e.get(k)));
        let els = v["elements"].as_array().unwrap();
        assert_eq!(els.len(), 1);
        assert_eq!(els[0]["type"], "node");
        assert_eq!(els[0]["lat"], 44.85);
        assert_eq!(els[0]["lon"], 6.81);
        let jura = BBox::new(46.5, 6.0, 46.7, 6.2);
        let v = idx.query(&[cerv, jura], |e| is_hut(|k| e.get(k)));
        let els = v["elements"].as_array().unwrap();
        assert_eq!(els.len(), 2);
        assert_eq!(els[1]["center"]["lat"], 46.6000005);
        assert_eq!(els[1]["center"]["lon"], 6.100001);
        assert_eq!(els[1]["tags"]["amenity"], "shelter");
        let r = idx.relations(&[3, 4]);
        assert_eq!(r["elements"].as_array().unwrap().len(), 1);
        assert_eq!(r["elements"][0]["tags"]["site"], "piste");
    }

    #[test]
    fn index_round_trip_and_truncation() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("index-v1-x.jsonl.gz");
        let els = vec![el(Kind::Node, 1, [1, 2, 1, 2], &[("amenity", "shelter")])];
        let h = Header {
            version: INDEX_VERSION,
            key: "k".into(),
            timestamps: BTreeMap::new(),
            elements: 1,
        };
        write_index(&p, &h, &els).unwrap();
        let (h2, e2) = read_index(&p, "k").unwrap();
        assert_eq!(h2.elements, 1);
        assert_eq!(e2, els);
        assert!(read_index(&p, "other").is_err(), "other extracts");
        // An index cut short is detected (then rebuilt by `load`).
        let h3 = Header { elements: 2, ..h };
        write_index(&p, &h3, &els).unwrap();
        assert!(read_index(&p, "k").is_err());
    }
}
