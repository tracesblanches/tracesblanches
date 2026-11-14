//! P2 (#15): nordic trails of a site from OpenSnowMap's daily `planet_pistes.osm.gz`,
//! written as a GeoJSON layer and a routing graph.
//!
//! Output, in `data/out/<site>/pistes/`:
//! - `pistes.geojson`: one LineString per OSM way (nordic, hike, snowshoe) with at least
//!   one vertex in the site box;
//! - `graph_edges.geojson`: the same ways split at junctions, plus connectors closing gaps
//!   of at most [`CONNECTOR_MAX_M`] between a dead end and another piste;
//! - `graph_nodes.geojson`: junctions and ends;
//! - `pistes.meta.json`: sources, licence, dates, documented properties, key figures.
//!
//! The extract is read once (≈ 3.5 s for the whole planet file) and kept for the area of
//! interest [`france_aoi`], which also serves the domain census (P3).

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::Serialize;
use serde_json::{Value, json};
use skimap_common::download::{Downloader, Request};
use skimap_common::geom::BBox;
use skimap_common::layout::DataLayout;
use skimap_common::site::Site;

use crate::error::Result;
use crate::graph::{EdgeKind, Graph};
use crate::output::{
    FileMeta, LayerMeta, SourceMeta, feature, line_string, now_rfc3339, point, write_geojson,
    write_json,
};
use crate::parse::{Aoi, OsmData, read_file};
use crate::pistes::{Oneway, Piste, PisteSet, Use, classify};

/// Daily extract of every piste of the planet (OpenSnowMap).
pub const PLANET_URL: &str = "https://www.opensnowmap.org/download/planet_pistes.osm.gz";
/// Timestamp of the OSM data in the extract.
pub const STATE_URL: &str = "https://www.opensnowmap.org/download/planet_pistes-state.txt";
/// Downloader source name.
pub const SOURCE: &str = "opensnowmap";
/// Licence of the OSM data.
pub const LICENCE: &str = "ODbL 1.0";
/// Attribution to display.
pub const ATTRIBUTION: &str =
    "© les contributeurs d'OpenStreetMap (ODbL), extrait quotidien OpenSnowMap.org";
/// Largest gap closed by a connector in the routing graph, metres.
pub const CONNECTOR_MAX_M: f64 = 10.0;
/// Layer name.
pub const LAYER: &str = "pistes";

/// Metropolitan France with a margin (Switzerland, Italy, Spain borders included), plus
/// the two sites: the area kept from the planet file.
pub fn france_aoi() -> Aoi {
    let mut boxes = vec![BBox::new(41.2, -5.6, 51.3, 10.0)];
    boxes.extend(skimap_common::site::SITES.iter().map(|s| s.bbox));
    Aoi(boxes)
}

/// The planet extract restricted to [`france_aoi`], classified.
pub struct Planet {
    /// Raw OSM data of the area.
    pub data: OsmData,
    /// Pistes of the area.
    pub set: PisteSet,
    /// Source description.
    pub source: SourceMeta,
    /// Timestamp of the OSM data (`planet_pistes-state.txt`).
    pub valid_at: Option<String>,
}

/// Downloads (or reuses from cache) and reads the planet extract.
pub fn load_planet(dl: &Downloader) -> Result<Planet> {
    let f = dl.fetch(&Request::get(SOURCE, PLANET_URL))?;
    let valid_at = match dl.fetch(&Request::get(SOURCE, STATE_URL)) {
        Ok(s) => s.text().ok().map(|t| t.trim().to_owned()),
        Err(e) => {
            tracing::warn!(error = %e, "planet_pistes-state.txt unavailable");
            None
        }
    };
    let t = std::time::Instant::now();
    let data = read_file(&f.path, &france_aoi())?;
    let set = classify(&data);
    tracing::info!(
        elapsed_ms = t.elapsed().as_millis() as u64,
        nodes = data.node_count(),
        ways = data.ways.len(),
        relations = data.relations.len(),
        pistes = set.pistes.len(),
        "planet_pistes read"
    );
    Ok(Planet {
        data,
        set,
        source: SourceMeta::from_fetched(
            "OpenSnowMap planet_pistes.osm.gz",
            LICENCE,
            ATTRIBUTION,
            &f,
        ),
        valid_at,
    })
}

/// Key figures of the pistes layer of a site.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PistesStats {
    /// Ways in the layer.
    pub ways: usize,
    /// Ways carrying their own kept `piste:type`.
    pub ways_tagged: usize,
    /// Ways that are pistes only through a relation.
    pub ways_relation_only: usize,
    /// Total length, km (each way once).
    pub km_total: f64,
    /// Length of ways used for nordic skiing, km.
    pub km_nordic: f64,
    /// Length of ways used only for hiking / snowshoeing, km.
    pub km_hike_only: f64,
    /// Length of nordic ways that carry their own `piste:type` (no relation-only member).
    pub km_nordic_tagged: f64,
    /// Piste relations used by the ways.
    pub relations: usize,
    /// Ways with a relation colour giving a difficulty.
    pub ways_difficulty_from_colour: usize,
    /// Ways whose difficulty comes from the relations' `piste:difficulty`.
    pub ways_difficulty_from_relation_tag: usize,
    /// Ways whose difficulty comes from the way only.
    pub ways_difficulty_from_way: usize,
    /// Ways with a grooming value (own or inherited).
    pub ways_with_grooming: usize,
    /// Ways tagged one-way.
    pub ways_oneway: usize,
    /// Graph nodes.
    pub graph_nodes: usize,
    /// Graph edges (including connectors).
    pub graph_edges: usize,
    /// Connectors added.
    pub graph_connectors: usize,
    /// Connected components of the graph.
    pub graph_components: usize,
}

/// Result of [`build`] for one site.
#[derive(Debug, Clone)]
pub struct PistesOutput {
    /// Layer directory.
    pub dir: PathBuf,
    /// The site's pistes (for further analysis).
    pub set: PisteSet,
    /// Key figures.
    pub stats: PistesStats,
}

/// Builds the pistes layer of `site` (downloads the extract through `dl` if needed).
pub fn build(site: &Site, layout: &DataLayout, dl: &Downloader) -> Result<PistesOutput> {
    let planet = load_planet(dl)?;
    build_from(site, layout, &planet)
}

fn uses_json(p: &Piste) -> Value {
    json!(p.uses.iter().collect::<Vec<&Use>>())
}

fn piste_properties(set: &PisteSet, p: &Piste) -> serde_json::Map<String, Value> {
    let relations: Vec<Value> = p
        .relations
        .iter()
        .map(|&i| {
            let r = &set.relations[i];
            json!({
                "id": r.id,
                "name": r.name,
                "ref": r.ref_,
                "colour": r.colour,
                "colour_raw": r.colour_raw,
                "difficulty": r.difficulty,
            })
        })
        .collect();
    let mut m = serde_json::Map::new();
    m.insert("osm_way_id".into(), json!(p.way_id));
    m.insert("name".into(), json!(set.display_name(p)));
    m.insert("way_name".into(), json!(p.way_name));
    m.insert("uses".into(), uses_json(p));
    m.insert("tagged".into(), json!(p.tagged));
    m.insert("relations".into(), Value::Array(relations));
    m.insert("colours".into(), json!(set.colours(p)));
    m.insert("difficulty".into(), json!(p.difficulty));
    m.insert("difficulty_max".into(), json!(p.difficulty_max));
    m.insert("difficulty_source".into(), json!(p.difficulty_source));
    m.insert("way_difficulty".into(), json!(p.way_difficulty));
    m.insert("grooming".into(), json!(p.grooming.raw));
    m.insert("classic".into(), json!(p.grooming.classic));
    m.insert("skating".into(), json!(p.grooming.skating));
    m.insert("backcountry".into(), json!(p.grooming.backcountry));
    m.insert("scooter".into(), json!(p.grooming.scooter));
    m.insert(
        "oneway".into(),
        json!(matches!(p.oneway, Oneway::Forward | Oneway::Backward)),
    );
    m.insert("oneway_tag".into(), json!(p.oneway));
    m.insert("lit".into(), json!(p.lit));
    m.insert("length_m".into(), json!((p.length_m * 10.0).round() / 10.0));
    m
}

/// Documented properties of `pistes.geojson`.
fn piste_properties_doc() -> Value {
    json!({
        "osm_way_id": "identifiant du way OSM",
        "name": "nom du way (piste:name, sinon name), sinon noms des relations joints par « / »",
        "way_name": "nom porté par le way lui-même",
        "uses": "usages : nordic, hike (raquettes / piéton), snowshoe ; union du way et de ses relations",
        "tagged": "le way porte lui-même piste:type (sinon il n'est piste que par une relation)",
        "relations": "relations de piste contenant le way : id, name, ref, colour (green, blue, red, black, other), colour_raw, difficulty",
        "colours": "couleurs distinctes des relations",
        "difficulty": "difficulté la plus facile parmi les relations (couleur d'abord, sinon piste:difficulty des relations, sinon celle du way) ; novice = verte, easy = bleue, intermediate = rouge, advanced = noire",
        "difficulty_max": "difficulté la plus dure parmi les mêmes sources",
        "difficulty_source": "relation_colour, relation_difficulty ou way",
        "way_difficulty": "piste:difficulty du way (incohérent, gardé pour comparaison)",
        "grooming": "piste:grooming brut (way, sinon relation)",
        "classic": "traces classiques",
        "skating": "piste de skating",
        "backcountry": "non damé (traces faites par les skieurs)",
        "scooter": "damé à la motoneige",
        "oneway": "sens unique ; la géométrie est alors dans le sens de circulation",
        "oneway_tag": "piste:oneway : forward, backward (géométrie inversée), no, unknown (absent, double sens)",
        "lit": "éclairée (piste:lit ou lit), null si inconnu",
        "length_m": "longueur, m (haversine)"
    })
}

/// Builds the pistes layer of `site` from an already loaded extract.
pub fn build_from(site: &Site, layout: &DataLayout, planet: &Planet) -> Result<PistesOutput> {
    let dir = layout.layer_dir(site, LAYER)?;
    let set = planet.set.in_bbox(&site.bbox);
    let graph = Graph::build(&set, CONNECTOR_MAX_M);

    let mut features = Vec::with_capacity(set.pistes.len());
    for p in &set.pistes {
        let mut pts: Vec<_> = p.points.iter().map(|(_, q)| *q).collect();
        if p.oneway == Oneway::Backward {
            pts.reverse();
        }
        features.push(feature(
            line_string(pts),
            Value::Object(piste_properties(&set, p)),
        ));
    }
    write_geojson(&dir.join("pistes.geojson"), &features)?;

    let adj = graph.adjacency();
    let mut edge_features = Vec::with_capacity(graph.edges.len());
    for (i, e) in graph.edges.iter().enumerate() {
        let mut props = match e.kind {
            EdgeKind::Piste { piste } => {
                let mut m = piste_properties(&set, &set.pistes[piste]);
                m.remove("length_m");
                m.insert("kind".into(), json!("piste"));
                m
            }
            EdgeKind::Connector => {
                let mut m = serde_json::Map::new();
                m.insert("kind".into(), json!("connector"));
                m.insert("osm_way_id".into(), Value::Null);
                m
            }
        };
        props.insert("edge".into(), json!(i));
        props.insert("from".into(), json!(e.from));
        props.insert("to".into(), json!(e.to));
        props.insert("from_osm_node".into(), json!(graph.nodes[e.from].osm_id));
        props.insert("to_osm_node".into(), json!(graph.nodes[e.to].osm_id));
        props.insert("oneway".into(), json!(e.oneway));
        props.insert("length_m".into(), json!((e.length_m * 10.0).round() / 10.0));
        edge_features.push(feature(
            line_string(e.points.iter().copied()),
            Value::Object(props),
        ));
    }
    write_geojson(&dir.join("graph_edges.geojson"), &edge_features)?;

    let (comp, n_comp) = graph.components();
    let node_features: Vec<Value> = graph
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| {
            feature(
                point(n.pos),
                json!({
                    "node": i,
                    "osm_node_id": n.osm_id,
                    "degree": adj[i].len(),
                    "component": comp[i],
                }),
            )
        })
        .collect();
    write_geojson(&dir.join("graph_nodes.geojson"), &node_features)?;

    let km = |f: &dyn Fn(&Piste) -> bool| (set.km(f) * 100.0).round() / 100.0;
    let count = |f: &dyn Fn(&Piste) -> bool| set.pistes.iter().filter(|p| f(p)).count();
    use crate::pistes::DifficultySource as D;
    let stats = PistesStats {
        ways: set.pistes.len(),
        ways_tagged: count(&|p| p.tagged),
        ways_relation_only: count(&|p| !p.tagged),
        km_total: km(&|_| true),
        km_nordic: km(&|p| p.is_nordic()),
        km_hike_only: km(&|p| !p.is_nordic()),
        km_nordic_tagged: km(&|p| p.is_nordic() && p.tagged),
        relations: set.relations.len(),
        ways_difficulty_from_colour: count(&|p| p.difficulty_source == Some(D::RelationColour)),
        ways_difficulty_from_relation_tag: count(&|p| {
            p.difficulty_source == Some(D::RelationDifficulty)
        }),
        ways_difficulty_from_way: count(&|p| p.difficulty_source == Some(D::Way)),
        ways_with_grooming: count(&|p| p.grooming.raw.is_some()),
        ways_oneway: count(&|p| matches!(p.oneway, Oneway::Forward | Oneway::Backward)),
        graph_nodes: graph.nodes.len(),
        graph_edges: graph.edges.len(),
        graph_connectors: graph
            .edges
            .iter()
            .filter(|e| e.kind == EdgeKind::Connector)
            .count(),
        graph_components: n_comp,
    };

    let relations: BTreeMap<i64, Value> = set
        .relations
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let km: f64 = set
                .pistes
                .iter()
                .filter(|p| p.relations.contains(&i))
                .map(|p| p.length_m)
                .sum::<f64>()
                / 1000.0;
            (
                r.id,
                json!({
                    "name": r.name,
                    "colour": r.colour,
                    "uses": r.uses,
                    "km_in_site": (km * 100.0).round() / 100.0,
                }),
            )
        })
        .collect();

    let mut edge_doc = piste_properties_doc();
    if let Value::Object(m) = &mut edge_doc {
        m.insert(
            "kind".into(),
            json!("piste (morceau de way) ou connector (jonction ajoutée)"),
        );
        m.insert("edge".into(), json!("index de l'arête"));
        m.insert(
            "from".into(),
            json!("index du nœud de départ (graph_nodes.geojson)"),
        );
        m.insert("to".into(), json!("index du nœud d'arrivée"));
        m.insert(
            "from_osm_node".into(),
            json!("identifiant OSM du nœud de départ"),
        );
        m.insert(
            "to_osm_node".into(),
            json!("identifiant OSM du nœud d'arrivée"),
        );
        m.insert(
            "oneway".into(),
            json!("circulation seulement de from vers to"),
        );
        m.insert("length_m".into(), json!("longueur de l'arête, m"));
    }
    let meta = LayerMeta {
        layer: LAYER.to_owned(),
        site: site.id.to_owned(),
        tickets: vec!["P2 (#15)".to_owned()],
        generated_at: now_rfc3339(),
        valid_at: planet.valid_at.clone(),
        crs: "EPSG:4326 (WGS84, longitude, latitude)".to_owned(),
        sources: vec![planet.source.clone()],
        files: vec![
            FileMeta {
                path: "pistes.geojson".to_owned(),
                description: "ways OSM de ski nordique, raquettes et randonnée hivernale ayant au moins un sommet dans l'emprise du site".to_owned(),
                licence: LICENCE.to_owned(),
                count: features.len(),
                properties: piste_properties_doc(),
            },
            FileMeta {
                path: "graph_edges.geojson".to_owned(),
                description: format!("graphe de routage : ways coupés aux jonctions, plus des connecteurs fermant les trous de moins de {CONNECTOR_MAX_M} m entre un cul-de-sac et une autre piste"),
                licence: LICENCE.to_owned(),
                count: edge_features.len(),
                properties: edge_doc,
            },
            FileMeta {
                path: "graph_nodes.geojson".to_owned(),
                description: "nœuds du graphe : jonctions, extrémités, cibles des connecteurs".to_owned(),
                licence: LICENCE.to_owned(),
                count: node_features.len(),
                properties: json!({
                    "node": "index du nœud",
                    "osm_node_id": "identifiant OSM",
                    "degree": "nombre d'arêtes incidentes",
                    "component": "composante connexe du graphe",
                }),
            },
        ],
        parameters: json!({
            "bbox_south_west_north_east": [site.bbox.south, site.bbox.west, site.bbox.north, site.bbox.east],
            "selection": "ways dont piste:type contient nordic, hike ou snowshoe, ou membres d'une relation de piste de ces types (ou route=ski) ; area=yes exclus ; au moins un sommet dans l'emprise",
            "connector_max_m": CONNECTOR_MAX_M,
        }),
        stats: json!({ "summary": stats, "relations": relations }),
    };
    write_json(&dir.join(format!("{LAYER}.meta.json")), &meta)?;
    Ok(PistesOutput { dir, set, stats })
}
