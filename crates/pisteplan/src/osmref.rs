//! What OSM already knows in the extent of a plan: piste ways and relations (OpenSnowMap
//! extract, as the `pistes` layer) and named landmarks (places, summits, passes, huts,
//! lakes: local Geofabrik extracts, `skimap_osm::local`, X5 #139; formerly Overpass),
//! projected to Lambert-93.

use std::collections::{BTreeMap, HashMap};

use serde_json::Value;
use skimap_common::crs::to_lambert93;
use skimap_common::download::{Downloader, Fetched, Request};
use skimap_common::geom::{BBox, LonLat};
use skimap_common::layout::DataLayout;
use skimap_common::meta::SourceRef;
use skimap_osm::extract::Planet;
use skimap_osm::pistes::{Colour, Difficulty, Oneway, Use};

use crate::error::{Error, Result};
use crate::transcript::LandmarkKind;
use crate::warp::P2;

/// A piste way of the extent.
#[derive(Debug, Clone)]
pub struct OsmWay {
    /// OSM way id.
    pub id: i64,
    /// Node ids.
    pub nodes: Vec<i64>,
    /// Positions.
    pub ll: Vec<LonLat>,
    /// Lambert-93 positions, metres.
    pub xy: Vec<P2>,
    /// Uses.
    pub uses: Vec<String>,
    /// Way name.
    pub name: Option<String>,
    /// Indices of the relations containing the way (into [`OsmRef::relations`]).
    pub relations: Vec<usize>,
    /// Difficulty colour of the way (relation colour, else difficulty), `green`…`black`.
    pub colour: Option<String>,
    /// One-way tag.
    pub oneway: Oneway,
    /// Length, m.
    pub length_m: f64,
}

/// A piste relation of the extent.
#[derive(Debug, Clone)]
pub struct OsmRelation {
    /// OSM relation id.
    pub id: i64,
    /// Name.
    pub name: Option<String>,
    /// `ref`.
    pub ref_: Option<String>,
    /// Difficulty colour (`green`…`black`) from `colour`, else `piste:difficulty`.
    pub colour: Option<String>,
    /// Raw `colour`.
    pub colour_raw: Option<String>,
    /// Uses.
    pub uses: Vec<String>,
    /// Member ways present in the extent (indices into [`OsmRef::ways`]).
    pub ways: Vec<usize>,
}

/// A named landmark.
#[derive(Debug, Clone)]
pub struct Landmark {
    /// `node/1`, `way/2`, `relation/3`.
    pub osm: String,
    /// Name.
    pub name: String,
    /// Kind.
    pub kind: LandmarkKind,
    /// Elevation tag, m.
    pub ele: Option<f64>,
    /// Position.
    pub ll: LonLat,
    /// Lambert-93 position.
    pub xy: P2,
}

/// OSM reference data of a plan.
#[derive(Debug, Clone, Default)]
pub struct OsmRef {
    /// Piste ways touching the extent.
    pub ways: Vec<OsmWay>,
    /// Piste relations with at least one way in the extent.
    pub relations: Vec<OsmRelation>,
    /// Landmarks.
    pub landmarks: Vec<Landmark>,
    /// Junction nodes (shared by ways, degree ≥ 3): node id → Lambert-93 position.
    pub junctions: BTreeMap<i64, P2>,
}

/// Difficulty colour name of an OSM colour.
pub fn colour_name(c: Colour) -> Option<&'static str> {
    match c {
        Colour::Green => Some("green"),
        Colour::Blue => Some("blue"),
        Colour::Red => Some("red"),
        Colour::Black => Some("black"),
        Colour::Other => None,
    }
}

/// Difficulty colour name of an OSM difficulty (European nordic convention).
pub fn difficulty_colour(d: Difficulty) -> Option<&'static str> {
    match d {
        Difficulty::Novice => Some("green"),
        Difficulty::Easy => Some("blue"),
        Difficulty::Intermediate => Some("red"),
        Difficulty::Advanced | Difficulty::Expert | Difficulty::Freeride => Some("black"),
    }
}

fn use_name(u: &Use) -> String {
    serde_json::to_value(u)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| format!("{u:?}").to_lowercase())
}

fn l93(p: LonLat) -> Result<P2> {
    let (x, y) = to_lambert93(p)?;
    Ok([x, y])
}

/// Piste ways and relations of `extent` from the planet extract.
pub fn pistes(planet: &Planet, extent: &BBox) -> Result<OsmRef> {
    let set = &planet.set;
    let mut out = OsmRef::default();
    let mut rel_index: HashMap<usize, usize> = HashMap::new();
    for p in set.pistes.iter().filter(|p| p.touches(extent)) {
        let ll: Vec<LonLat> = p.points.iter().map(|(_, q)| *q).collect();
        let xy = ll.iter().map(|&q| l93(q)).collect::<Result<Vec<_>>>()?;
        let mut rels = Vec::new();
        for &ri in &p.relations {
            let next = rel_index.len();
            let k = *rel_index.entry(ri).or_insert(next);
            if k == out.relations.len() {
                let r = &set.relations[ri];
                out.relations.push(OsmRelation {
                    id: r.id,
                    name: r.name.clone(),
                    ref_: r.ref_.clone(),
                    colour: r
                        .colour
                        .and_then(colour_name)
                        .or_else(|| r.difficulty_tag.and_then(difficulty_colour))
                        .map(str::to_owned),
                    colour_raw: r.colour_raw.clone(),
                    uses: r.uses.iter().map(use_name).collect(),
                    ways: Vec::new(),
                });
            }
            rels.push(k);
        }
        let colour = p.difficulty.and_then(difficulty_colour).map(str::to_owned);
        let wi = out.ways.len();
        for &k in &rels {
            out.relations[k].ways.push(wi);
        }
        out.ways.push(OsmWay {
            id: p.way_id,
            nodes: p.points.iter().map(|(id, _)| *id).collect(),
            ll,
            xy,
            uses: p.uses.iter().map(use_name).collect(),
            name: p.way_name.clone(),
            relations: rels,
            colour,
            oneway: p.oneway,
            length_m: p.length_m,
        });
    }
    // Junctions of the nordic network: node degree over the nordic ways (ends count 1,
    // inner vertices 2).
    let mut deg: HashMap<i64, (usize, P2)> = HashMap::new();
    for w in out
        .ways
        .iter()
        .filter(|w| w.uses.iter().any(|u| u == "nordic"))
    {
        let n = w.nodes.len();
        for (i, (&id, &xy)) in w.nodes.iter().zip(&w.xy).enumerate() {
            let d = if i == 0 || i + 1 == n { 1 } else { 2 };
            deg.entry(id).or_insert((0, xy)).0 += d;
        }
    }
    out.junctions = deg
        .into_iter()
        .filter(|(_, (d, _))| *d >= 3)
        .map(|(id, (_, xy))| (id, xy))
        .collect();
    Ok(out)
}

fn kind_of(tags: &serde_json::Map<String, Value>) -> LandmarkKind {
    let t = |k: &str| tags.get(k).and_then(Value::as_str).unwrap_or("");
    if !t("place").is_empty() {
        LandmarkKind::Place
    } else if t("natural") == "peak" || t("natural") == "volcano" {
        LandmarkKind::Summit
    } else if t("natural") == "saddle" || t("mountain_pass") == "yes" {
        LandmarkKind::Pass
    } else if matches!(t("tourism"), "alpine_hut" | "wilderness_hut") || t("amenity") == "shelter" {
        LandmarkKind::Refuge
    } else if t("natural") == "water" {
        LandmarkKind::Lake
    } else {
        LandmarkKind::Other
    }
}

/// Parses an answer in the Overpass `out center tags` shape (the local index,
/// [`skimap_osm::local::Index::query`]) into landmarks.
pub fn parse_landmarks(v: &Value) -> Result<Vec<Landmark>> {
    let els = v
        .get("elements")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Data("OSM answer without elements".into()))?;
    let mut out = Vec::new();
    for e in els {
        let (Some(ty), Some(id), Some(tags)) = (
            e.get("type").and_then(Value::as_str),
            e.get("id").and_then(Value::as_i64),
            e.get("tags").and_then(Value::as_object),
        ) else {
            continue;
        };
        let Some(name) = tags.get("name").and_then(Value::as_str) else {
            continue;
        };
        let pos = e.get("center").unwrap_or(e);
        let (Some(lat), Some(lon)) = (
            pos.get("lat").and_then(Value::as_f64),
            pos.get("lon").and_then(Value::as_f64),
        ) else {
            continue;
        };
        let ll = LonLat::new(lon, lat);
        out.push(Landmark {
            osm: format!("{ty}/{id}"),
            name: name.to_owned(),
            kind: kind_of(tags),
            ele: tags
                .get("ele")
                .and_then(Value::as_str)
                .and_then(|s| s.trim_end_matches(" m").replace(',', ".").parse().ok()),
            ll,
            xy: l93(ll)?,
        });
    }
    out.sort_by(|a, b| a.osm.cmp(&b.osm));
    out.dedup_by(|a, b| a.osm == b.osm);
    Ok(out)
}

/// IGN Géoplateforme WFS (BD TOPO).
pub const IGN_WFS_URL: &str = "https://data.geopf.fr/wfs/ows";

/// Kind of a BD TOPO toponym (`classe_de_l_objet`, `nature_de_l_objet`); `None` for the
/// classes that are not landmarks of a plan (rivers, roads, lines…).
pub fn ign_kind(class: &str, nature: &str) -> Option<LandmarkKind> {
    let n = crate::labels::fold(nature);
    Some(match class {
        "Zone d'habitation" => LandmarkKind::Place,
        "Détail orographique" if n.starts_with("col") => LandmarkKind::Pass,
        "Détail orographique"
            if n.starts_with("sommet") || n.starts_with("pic") || n.starts_with("crete") =>
        {
            LandmarkKind::Summit
        }
        "Plan d'eau" => LandmarkKind::Lake,
        "Zone d'activité ou d'intérêt" if n.contains("refuge") || n.contains("abri") => {
            LandmarkKind::Refuge
        }
        "Détail orographique"
        | "Lieu-dit non habité"
        | "Zone d'activité ou d'intérêt"
        | "Equipement de transport"
        | "Transport par cable" => LandmarkKind::Other,
        _ => return None,
    })
}

/// Parses a BD TOPO `toponymie` GeoJSON page into landmarks (`osm`: `ign/<cleabs>`).
pub fn parse_ign_toponyms(v: &Value) -> Result<Vec<Landmark>> {
    let feats = v
        .get("features")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Data("WFS answer without features".into()))?;
    let mut out = Vec::new();
    for f in feats {
        let p = &f["properties"];
        let (Some(name), Some(class)) = (
            p["graphie_du_toponyme"].as_str(),
            p["classe_de_l_objet"].as_str(),
        ) else {
            continue;
        };
        let Some(kind) = ign_kind(class, p["nature_de_l_objet"].as_str().unwrap_or("")) else {
            continue;
        };
        let c = &f["geometry"]["coordinates"];
        let (Some(lon), Some(lat)) = (c[0].as_f64(), c[1].as_f64()) else {
            continue;
        };
        let ll = LonLat::new(lon, lat);
        out.push(Landmark {
            osm: format!(
                "ign/{}",
                p["cleabs_de_l_objet"].as_str().unwrap_or_default()
            ),
            name: name.to_owned(),
            kind,
            ele: None,
            ll,
            xy: l93(ll)?,
        });
    }
    Ok(out)
}

/// Toponyms of `b` from the BD TOPO (IGN WFS, source `ign`, pages of 5 000).
pub fn landmarks_ign(dl: &Downloader, b: &BBox) -> Result<(Vec<Landmark>, Fetched)> {
    let mut out = Vec::new();
    let mut first = None;
    for page in 0..20 {
        let url = format!(
            "{IGN_WFS_URL}?SERVICE=WFS&VERSION=2.0.0&REQUEST=GetFeature&TYPENAMES=BDTOPO_V3:toponymie&BBOX={},{},{},{},urn:ogc:def:crs:EPSG::4326&COUNT=5000&STARTINDEX={}&OUTPUTFORMAT=application/json",
            b.south,
            b.west,
            b.north,
            b.east,
            page * 5000
        );
        let f = dl.fetch(&Request::get("ign", &url))?;
        let v: Value = f.json()?;
        let n = v["features"].as_array().map_or(0, Vec::len);
        out.extend(parse_ign_toponyms(&v)?);
        first.get_or_insert(f);
        if n < 5000 {
            break;
        }
    }
    let first = first.ok_or_else(|| Error::Data("no WFS page".into()))?;
    Ok((out, first))
}

/// Where the landmarks of a registration come from.
#[derive(Debug, Clone)]
pub struct LandmarkSource {
    /// Name of the source.
    pub name: &'static str,
    /// Sources to cite (one per Geofabrik extract read, or the IGN WFS answer).
    pub sources: Vec<SourceRef>,
}

/// Name of the OSM landmarks source.
pub const OSM_LANDMARKS: &str = "OpenStreetMap, repères nommés (extraits Geofabrik)";

/// Name of the IGN landmarks source.
pub const IGN_LANDMARKS: &str = "IGN BD TOPO, toponymie (Géoplateforme, WFS)";

/// Named landmarks of `b` from the local OSM index ([`skimap_osm::local::is_landmark`]).
pub fn landmarks_osm(
    layout: &DataLayout,
    dl: &Downloader,
    b: &BBox,
) -> Result<(Vec<Landmark>, LandmarkSource)> {
    let idx = skimap_osm::local::load_landmarks(layout, dl)
        .map_err(|e| Error::Data(format!("local OSM index: {e}")))?;
    let v = idx.query(&[*b], |e| {
        skimap_osm::local::is_landmark(e.kind, |k| e.get(k))
    });
    Ok((
        parse_landmarks(&v)?,
        LandmarkSource {
            name: OSM_LANDMARKS,
            sources: idx.sources_for(b),
        },
    ))
}

/// Overpass endpoint whose answers may already be in the cache (read offline only, never
/// requested: [`landmarks_any`]).
pub const OVERPASS_URL: &str = "https://overpass-api.de/api/interpreter";

/// Former Overpass query of the named landmarks of `b`: the cache key of the answers
/// downloaded before X5 (the filter is [`skimap_osm::local::is_landmark`]).
pub fn landmark_query(b: &BBox) -> String {
    let bb = b.overpass();
    format!(
        "[out:json][timeout:90];(\
node[\"name\"][\"place\"]({bb});\
way[\"name\"][\"place\"]({bb});\
node[\"name\"][\"natural\"~\"^(peak|saddle|volcano|spring)$\"]({bb});\
node[\"name\"][\"mountain_pass\"=\"yes\"]({bb});\
nwr[\"name\"][\"tourism\"~\"^(alpine_hut|wilderness_hut|museum|hotel|chalet)$\"]({bb});\
nwr[\"name\"][\"amenity\"~\"^(shelter|restaurant|place_of_worship)$\"]({bb});\
nwr[\"name\"][\"natural\"=\"water\"]({bb});\
nwr[\"name\"][\"historic\"]({bb});\
);out center tags;"
    )
}

/// Landmarks of `b`, in the order the registrations always used, so that every plan keeps
/// its landmarks: an Overpass answer already in the cache (read offline, no request), else
/// the BD TOPO toponyms (IGN), else, instead of the former Overpass request, the local OSM
/// extracts ([`landmarks_osm`]; same elements as Overpass, `crates/osm/docs/local.md`).
pub fn landmarks_any(
    layout: &DataLayout,
    dl: &Downloader,
    b: &BBox,
) -> Result<(Vec<Landmark>, LandmarkSource)> {
    let q = landmark_query(b);
    let cached = dl.fetch(
        &Request::post_form("overpass", OVERPASS_URL, &[("data", &q)])
            .cache(skimap_common::download::CachePolicy::Offline),
    );
    if let Ok(f) = cached {
        let v: Value = f.json()?;
        let name = "OpenStreetMap, repères nommés (Overpass, réponse en cache)";
        return Ok((
            parse_landmarks(&v)?,
            LandmarkSource {
                name,
                sources: vec![SourceRef::from_fetched(
                    name,
                    skimap_osm::local::LICENCE,
                    "© les contributeurs d'OpenStreetMap (ODbL)",
                    &f,
                )],
            },
        ));
    }
    match landmarks_ign(dl, b) {
        Ok((l, f)) => Ok((
            l,
            LandmarkSource {
                name: IGN_LANDMARKS,
                sources: vec![SourceRef::from_fetched(
                    IGN_LANDMARKS,
                    "Licence Ouverte Etalab 2.0",
                    "IGN – BD TOPO",
                    &f,
                )],
            },
        )),
        Err(ign) => {
            tracing::warn!(error = %ign, "IGN toponyms unreachable, local OSM landmarks used");
            landmarks_osm(layout, dl, b)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn landmarks_are_parsed_with_kind_and_elevation() {
        let v = json!({"elements": [
            {"type": "node", "id": 1, "lat": 44.85, "lon": 6.72, "tags": {"natural": "peak", "name": "Lasseron", "ele": "2703"}},
            {"type": "way", "id": 2, "center": {"lat": 46.6, "lon": 6.1}, "tags": {"natural": "water", "name": "Lac des Mortes"}},
            {"type": "node", "id": 3, "lat": 46.6, "lon": 6.1, "tags": {"place": "village", "name": "Chapelle-des-Bois"}},
            {"type": "node", "id": 4, "lat": 46.6, "lon": 6.1, "tags": {"place": "hamlet"}}
        ]});
        let l = parse_landmarks(&v).unwrap();
        assert_eq!(l.len(), 3);
        assert_eq!(l[0].kind, LandmarkKind::Summit);
        assert_eq!(l[0].ele, Some(2703.0));
        assert_eq!(l[1].kind, LandmarkKind::Place);
        assert_eq!(l[2].kind, LandmarkKind::Lake);
        assert!(landmark_query(&BBox::new(1.0, 2.0, 3.0, 4.0)).contains("(1,2,3,4)"));
    }
}
