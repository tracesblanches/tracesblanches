//! P2 (#15): nordic trails of a site from OpenSnowMap's daily `planet_pistes.osm.gz`,
//! written as a GeoJSON layer and a routing graph.
//!
//! Output, in `data/out/<site>/pistes/`:
//! - `pistes.geojson`: one LineString per OSM way (nordic, hike, snowshoe) with at least
//!   one vertex in the site box;
//! - `graph_edges.geojson`: the same ways split at junctions and at crossings without a
//!   shared node, plus connectors closing gaps of at most [`CONNECTOR_MAX_M`] between a
//!   dead end and another piste, and of at most [`DEAD_END_PAIR_MAX_M`] between two dead
//!   ends of disjoint parts of the network ([`Graph::build`], B166 #166);
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
use crate::graph::{DEAD_END_PAIR_MAX_M, EdgeKind, Graph, M_PER_DEG_LAT};
use crate::parse::{Aoi, OsmData, read_file};
use crate::pistes::{Oneway, Piste, PisteSet, Use, classify};
use skimap_common::meta::{FileMeta, LayerMeta, SourceRef};
use skimap_common::output::{feature, line_string, point, write_geojson, write_json};

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
/// Longest outside path kept to bring the site network back into the site box
/// ([`Graph::reentry_paths`]), metres.
pub const REENTRY_MAX_M: f64 = 10_000.0;

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
    pub source: SourceRef,
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
        source: SourceRef::from_fetched(
            "OpenSnowMap planet_pistes.osm.gz",
            LICENCE,
            ATTRIBUTION,
            &f,
        ),
        valid_at,
    })
}

/// Key figures of the pistes layer of a site. Way, length and relation figures count the
/// ways with a vertex in the site box only (the research reports count those); the
/// outside paths kept by [`site_pistes`] have their own figures, and the graph figures
/// cover the whole layer.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PistesStats {
    /// Ways with a vertex in the site box.
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
    /// Ways outside the box kept to bring the network back into it ([`site_pistes`]).
    pub ways_outside_box: usize,
    /// Length of those ways, km.
    pub km_outside_box: f64,
    /// Graph nodes.
    pub graph_nodes: usize,
    /// Graph edges (including connectors).
    pub graph_edges: usize,
    /// Connectors added.
    pub graph_connectors: usize,
    /// Connected components of the graph.
    pub graph_components: usize,
    /// OSM nodes where every edge is one-way and all point in (sink) or all point out
    /// (source): `piste:oneway` contradictions to fix in OSM ([`Graph::oneway_traps`]).
    pub oneway_sinks: Vec<i64>,
    /// See [`PistesStats::oneway_sinks`].
    pub oneway_sources: Vec<i64>,
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
    /// Outside paths kept in the layer.
    pub reentries: Vec<KeptReentry>,
}

/// Builds the pistes layer of `site` (downloads the extract through `dl` if needed).
pub fn build(site: &Site, layout: &DataLayout, dl: &Downloader) -> Result<PistesOutput> {
    let planet = load_planet(dl)?;
    build_from(site, layout, &planet)
}

fn uses_json(p: &Piste) -> Value {
    json!(p.uses.iter().collect::<Vec<&Use>>())
}

fn piste_properties(set: &PisteSet, p: &Piste, bbox: &BBox) -> serde_json::Map<String, Value> {
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
    m.insert("in_box".into(), json!(p.touches(bbox)));
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
        "length_m": "longueur, m (haversine)",
        "in_box": "au moins un sommet dans l'emprise du site ; false : way hors de l'emprise gardé pour que le réseau y revienne (sens uniques qui sortent de l'emprise)"
    })
}

/// An outside path kept in a site layer (see [`site_pistes`]).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct KeptReentry {
    /// OSM node where the path leaves the site network.
    pub from_osm_node: i64,
    /// OSM node where it comes back.
    pub to_osm_node: i64,
    /// Length of the outside part, metres.
    pub length_m: f64,
    /// OSM ways of the outside part, in path order.
    pub osm_way_ids: Vec<i64>,
}

/// Pistes of a site: every piste with a vertex in `bbox`, plus the outside pistes that the
/// network needs to come back into the box.
///
/// Cutting the network at the box turns a one-way trail that leaves the box and returns
/// outside it into a dead end that nothing leaves (loops of a Jura domain that cross the
/// west edge of its box). The full network around the box
/// (pistes within [`REENTRY_MAX_M`] of the site pistes) is built, and the outside paths
/// of [`Graph::reentry_paths`] are added to the site.
pub fn site_pistes(all: &PisteSet, bbox: &BBox) -> (PisteSet, Vec<KeptReentry>) {
    let inside: Vec<usize> = (0..all.pistes.len())
        .filter(|&i| all.pistes[i].touches(bbox))
        .collect();
    // Box of the site pistes (they overhang the site box), widened by the search radius.
    let mut b = BBox::new(f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for (_, p) in inside.iter().flat_map(|&i| &all.pistes[i].points) {
        b.south = b.south.min(p.lat);
        b.north = b.north.max(p.lat);
        b.west = b.west.min(p.lon);
        b.east = b.east.max(p.lon);
    }
    if inside.is_empty() {
        return (all.subset(Vec::new()), Vec::new());
    }
    let dlat = REENTRY_MAX_M / M_PER_DEG_LAT;
    let dlon = dlat
        / b.south
            .abs()
            .max(b.north.abs())
            .to_radians()
            .cos()
            .max(0.05);
    let region_box = BBox::new(b.south - dlat, b.west - dlon, b.north + dlat, b.east + dlon);
    let region: Vec<usize> = (0..all.pistes.len())
        .filter(|&i| all.pistes[i].touches(&region_box))
        .collect();
    let region_set = all.subset(region.iter().map(|&i| all.pistes[i].clone()).collect());
    let is_inside: Vec<bool> = region
        .iter()
        .map(|i| inside.binary_search(i).is_ok())
        .collect();
    let g = Graph::build(&region_set, CONNECTOR_MAX_M);
    let paths = g.reentry_paths(&is_inside, REENTRY_MAX_M);
    let mut keep: Vec<usize> = inside;
    keep.extend(
        paths
            .iter()
            .flat_map(|r| r.pistes.iter().map(|&j| region[j])),
    );
    keep.sort_unstable();
    keep.dedup();
    let kept = paths
        .iter()
        .map(|r| KeptReentry {
            from_osm_node: g.nodes[r.from].osm_id,
            to_osm_node: g.nodes[r.to].osm_id,
            length_m: (r.length_m * 10.0).round() / 10.0,
            osm_way_ids: r
                .pistes
                .iter()
                .map(|&j| region_set.pistes[j].way_id)
                .collect(),
        })
        .collect();
    (
        all.subset(keep.iter().map(|&i| all.pistes[i].clone()).collect()),
        kept,
    )
}

/// OSM id of a graph node as written, `null` for a node added by [`Graph::build`] (at a
/// crossing or at the foot of a connector; negative id).
fn osm_node(id: i64) -> Value {
    if id > 0 { json!(id) } else { Value::Null }
}

/// Builds the pistes layer of `site` from an already loaded extract.
pub fn build_from(site: &Site, layout: &DataLayout, planet: &Planet) -> Result<PistesOutput> {
    let dir = layout.layer_dir(site, LAYER)?;
    let (set, reentries) = site_pistes(&planet.set, &site.bbox);
    for r in &reentries {
        tracing::info!(site = site.id, ?r, "outside path kept");
    }
    // An SRN site is skied on its itineraries (#168): they are ways of its graph, after
    // the OSM ways (`pistes.geojson` keeps the OSM ways only).
    let srn = crate::srn_ways::site_srn(site, layout)?;
    let mut graph_set = set.clone();
    graph_set
        .pistes
        .extend(srn.ways.iter().map(|w| w.piste.clone()));
    let graph = Graph::build(&graph_set, CONNECTOR_MAX_M);

    let mut features = Vec::with_capacity(set.pistes.len());
    for p in &set.pistes {
        let mut pts: Vec<_> = p.points.iter().map(|(_, q)| *q).collect();
        if p.oneway == Oneway::Backward {
            pts.reverse();
        }
        features.push(feature(
            line_string(pts),
            Value::Object(piste_properties(&set, p, &site.bbox)),
        ));
    }
    write_geojson(&dir.join("pistes.geojson"), &features)?;

    let adj = graph.adjacency();
    let mut edge_features = Vec::with_capacity(graph.edges.len());
    for (i, e) in graph.edges.iter().enumerate() {
        let mut props = match e.kind {
            EdgeKind::Piste { piste } => {
                let mut m = piste_properties(&graph_set, &graph_set.pistes[piste], &site.bbox);
                m.remove("length_m");
                m.insert("kind".into(), json!("piste"));
                if let Some(w) = piste
                    .checked_sub(set.pistes.len())
                    .and_then(|i| srn.ways.get(i))
                {
                    m.insert("osm_way_id".into(), Value::Null);
                    m.insert("srn_route".into(), json!(w.route_id));
                }
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
        props.insert("from_osm_node".into(), osm_node(graph.nodes[e.from].osm_id));
        props.insert("to_osm_node".into(), osm_node(graph.nodes[e.to].osm_id));
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
                    "osm_node_id": osm_node(n.osm_id),
                    "degree": adj[i].len(),
                    "component": comp[i],
                }),
            )
        })
        .collect();
    write_geojson(&dir.join("graph_nodes.geojson"), &node_features)?;

    let in_box = set.in_bbox(&site.bbox);
    let km = |f: &dyn Fn(&Piste) -> bool| (in_box.km(f) * 100.0).round() / 100.0;
    let count = |f: &dyn Fn(&Piste) -> bool| in_box.pistes.iter().filter(|p| f(p)).count();
    let traps = graph.oneway_traps();
    let trap_ids = |sink: bool| -> Vec<i64> {
        traps
            .iter()
            .filter(|t| t.sink == sink)
            .map(|t| graph.nodes[t.node].osm_id)
            .collect()
    };
    use crate::pistes::DifficultySource as D;
    let stats = PistesStats {
        ways: in_box.pistes.len(),
        ways_tagged: count(&|p| p.tagged),
        ways_relation_only: count(&|p| !p.tagged),
        km_total: km(&|_| true),
        km_nordic: km(&|p| p.is_nordic()),
        km_hike_only: km(&|p| !p.is_nordic()),
        km_nordic_tagged: km(&|p| p.is_nordic() && p.tagged),
        relations: in_box.relations.len(),
        ways_difficulty_from_colour: count(&|p| p.difficulty_source == Some(D::RelationColour)),
        ways_difficulty_from_relation_tag: count(&|p| {
            p.difficulty_source == Some(D::RelationDifficulty)
        }),
        ways_difficulty_from_way: count(&|p| p.difficulty_source == Some(D::Way)),
        ways_with_grooming: count(&|p| p.grooming.raw.is_some()),
        ways_oneway: count(&|p| matches!(p.oneway, Oneway::Forward | Oneway::Backward)),
        ways_outside_box: set.pistes.len() - in_box.pistes.len(),
        km_outside_box: ((set.km(|_| true) - in_box.km(|_| true)) * 100.0).round() / 100.0,
        graph_nodes: graph.nodes.len(),
        graph_edges: graph.edges.len(),
        graph_connectors: graph
            .edges
            .iter()
            .filter(|e| e.kind == EdgeKind::Connector)
            .count(),
        graph_components: n_comp,
        oneway_sinks: trap_ids(true),
        oneway_sources: trap_ids(false),
    };

    let relations: BTreeMap<i64, Value> = in_box
        .relations
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let km: f64 = in_box
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
            json!("identifiant OSM du nœud de départ (null : nœud ajouté)"),
        );
        m.insert(
            "to_osm_node".into(),
            json!("identifiant OSM du nœud d'arrivée (null : nœud ajouté)"),
        );
        m.insert(
            "oneway".into(),
            json!("circulation seulement de from vers to"),
        );
        m.insert("length_m".into(), json!("longueur de l'arête, m"));
        m.insert(
            "srn_route".into(),
            json!("zones SRN et traversées seulement : identifiant de l'itinéraire SRN (srn/srn_routes.geojson) dont l'arête est un morceau, osm_way_id null"),
        );
    }
    let srn_km = srn.ways.iter().map(|w| w.piste.length_m).sum::<f64>() / 1000.0;
    let mut sources = vec![planet.source.clone()];
    sources.extend(srn.sources.iter().cloned());
    let mut meta = LayerMeta {
        tickets: vec!["P2 (#15)".to_owned()],
        valid_from: planet.valid_at.clone(),
        crs: Some("EPSG:4326 (WGS84, longitude, latitude)".to_owned()),
        sources,
        files: vec![
            FileMeta::new("pistes.geojson", format!("ways OSM de ski nordique, raquettes et randonnée hivernale ayant au moins un sommet dans l'emprise du site, plus les ways hors de l'emprise qui ramènent le réseau dans l'emprise (chemin hors emprise le plus court, {} km au plus, propriété in_box = false)", REENTRY_MAX_M / 1000.0))
                .licence(LICENCE)
                .count(features.len())
                .properties_json(&piste_properties_doc()),
            FileMeta::new("graph_edges.geojson", format!("graphe de routage : ways coupés aux jonctions, un nœud au croisement de deux pistes sans nœud commun (hors pont, tunnel ou niveau), plus des connecteurs fermant les trous de moins de {CONNECTOR_MAX_M} m entre un cul-de-sac et une autre piste et de moins de {DEAD_END_PAIR_MAX_M} m entre deux culs-de-sac de parties disjointes du réseau (largeur d'une route) ; zone SRN ou traversée : plus les itinéraires SRN de la couche srn du site, joints aux pistes de la même façon (propriété srn_route, osm_way_id null)"))
                .licence(LICENCE)
                .count(edge_features.len())
                .properties_json(&edge_doc),
            FileMeta::new("graph_nodes.geojson", "nœuds du graphe : jonctions, extrémités, cibles des connecteurs")
                .licence(LICENCE)
                .count(node_features.len())
                .properties_json(&json!({
                    "node": "index du nœud",
                    "osm_node_id": "identifiant OSM (null : nœud ajouté au croisement de deux pistes ou au pied d'un connecteur)",
                    "degree": "nombre d'arêtes incidentes",
                    "component": "composante connexe du graphe",
                })),
        ],
        parameters: json!({
            "bbox_south_west_north_east": [site.bbox.south, site.bbox.west, site.bbox.north, site.bbox.east],
            "selection": "ways dont piste:type contient nordic, hike ou snowshoe, ou membres d'une relation de piste de ces types (ou route=ski) ; area=yes exclus ; au moins un sommet dans l'emprise",
            "connector_max_m": CONNECTOR_MAX_M,
            "reentry_max_m": REENTRY_MAX_M,
            "oneway": "piste:oneway du way seulement : ni oneway=* (sens des voitures sur les routes), ni oneway=yes ni rôles forward / backward des relations (une relation aller-retour comme « Fabrice Guy » parcourt les mêmes ways dans les deux sens)",
        }),
        stats: json!({ "summary": stats, "relations": relations, "reentries": reentries }),
        ..LayerMeta::new(LAYER, site.id)
    };
    if crate::srn_ways::takes_itineraries(site) {
        meta.parameters["srn_itineraries"] =
            json!({"ways": srn.ways.len(), "km": (srn_km * 100.0).round() / 100.0});
    }
    write_json(&dir.join(format!("{LAYER}.meta.json")), &meta)?;
    Ok(PistesOutput {
        dir,
        set,
        stats,
        reentries,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::{Tags, Way};
    use skimap_common::geom::LonLat;

    fn way(id: i64, nodes: &[i64]) -> Way {
        Way {
            id,
            nodes: nodes.to_vec(),
            tags: Tags(vec![
                ("piste:type".to_owned(), "nordic".to_owned()),
                ("piste:oneway".to_owned(), "yes".to_owned()),
            ]),
        }
    }

    /// A one-way loop crossing the west edge of a box at 6.0° E: ways 1 and 3 have a vertex
    /// in the box, way 2 runs 1 km west of it; way 4 is a one-way trail 50 km away.
    #[test]
    fn site_keeps_the_outside_part_of_a_loop() {
        let lat = 46.6;
        let nodes = [
            (1, LonLat::new(6.01, lat)),
            (2, LonLat::new(5.99, lat + 0.005)),
            (3, LonLat::new(5.98, lat)),
            (4, LonLat::new(5.99, lat - 0.005)),
            (5, LonLat::new(6.6, lat)),
            (6, LonLat::new(6.61, lat)),
        ];
        let data = OsmData::from_elements(
            nodes,
            vec![
                way(1, &[1, 2]),
                way(2, &[2, 3, 4]),
                way(3, &[4, 1]),
                way(4, &[5, 6]),
            ],
            vec![],
        );
        let all = classify(&data);
        let bbox = BBox::new(46.5, 6.0, 46.7, 6.2);
        assert_eq!(all.in_bbox(&bbox).pistes.len(), 2);
        // Cut at the box, the loop has a dead end at 2 and a start at 4.
        let cut = Graph::build(&all.in_bbox(&bbox), CONNECTOR_MAX_M);
        assert_eq!(cut.components().1, 1);
        let (set, kept) = site_pistes(&all, &bbox);
        let ids: Vec<i64> = set.pistes.iter().map(|p| p.way_id).collect();
        assert_eq!(ids, vec![1, 2, 3]);
        assert_eq!(kept.len(), 1);
        assert_eq!(
            (kept[0].from_osm_node, kept[0].to_osm_node),
            (2, 4),
            "{kept:?}"
        );
        assert_eq!(kept[0].osm_way_ids, vec![2]);
        // The site graph is now one loop, every node reachable from every other.
        let g = Graph::build(&set, CONNECTOR_MAX_M);
        assert!(g.oneway_traps().is_empty());
        assert_eq!(g.components().1, 1);
        assert!(g.edges.iter().all(|e| e.oneway));
        let out = g.out_adjacency();
        assert!(out.iter().all(|o| o.len() == 1));
    }
}
