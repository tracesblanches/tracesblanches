//! The SRN layer: gathers the sources, builds the areas, writes `data/out/france/srn/`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use geo::{Area, Contains, Intersects};
use geo_types::{MultiPolygon, Point};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use skimap_common::download::{Downloader, Request};
use skimap_common::geom::LonLat;
use skimap_common::layout::{DataLayout, FRANCE};
use skimap_common::meta::{FileMeta, LayerMeta, SourceRef};
use skimap_common::output::{feature, line_string, point, round7, write_geojson};
use skimap_osm::census::{CensusOptions, Commune, build_france_from};
use skimap_osm::extract::{Planet, load_planet};

use crate::areas::{self, AreaGeom, Kind, Placed, StationOverlap};
use crate::error::{Result, SrnError};
use crate::route::{Activity, Hint, Provider};
use crate::{camptocamp, datatourisme, geotrek, osm};

/// Layer name.
pub const LAYER: &str = "srn";
/// Areas file.
pub const AREAS_FILE: &str = "srn_areas.geojson";
/// Itineraries file.
pub const ROUTES_FILE: &str = "srn_routes.geojson";
/// Points without track.
pub const HINTS_FILE: &str = "srn_hints.geojson";
/// Census layer read for the groomed domains.
pub const DOMAINES_LAYER: &str = "domaines";
/// Hand-kept names of well-known areas (`data/names.json`).
pub const NAMES_JSON: &str = include_str!("../data/names.json");

/// A hand-kept area name: applies to a play area whose representative point is within
/// `radius_km` of the point.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct KnownName {
    /// Name.
    pub name: String,
    /// Longitude of the reference point.
    pub lon: f64,
    /// Latitude of the reference point.
    pub lat: f64,
    /// Radius, km.
    pub radius_km: f64,
    /// Where the name and the point come from.
    pub source: String,
}

/// Hand-kept names.
pub fn known_names() -> Result<Vec<KnownName>> {
    serde_json::from_str(NAMES_JSON).map_err(|e| SrnError::format("names.json", e.to_string()))
}

/// Massif group of a département (same groups as research report 08).
pub fn massif_of(dep: &str) -> &'static str {
    match dep {
        "01" | "25" | "39" => "Jura",
        "38" | "73" | "74" => "Alpes du Nord",
        "04" | "05" | "06" | "26" | "84" => "Alpes du Sud",
        "03" | "07" | "12" | "15" | "19" | "23" | "30" | "42" | "43" | "48" | "63" | "81"
        | "87" => "Massif central",
        "54" | "57" | "67" | "68" | "70" | "88" | "90" => "Vosges",
        "09" | "11" | "31" | "64" | "65" | "66" => "Pyrénées",
        "2A" | "2B" => "Corse",
        "08" | "21" | "58" | "71" | "89" => "Ardennes et Morvan",
        _ => "autre",
    }
}

/// One area of the layer.
#[derive(Debug, Clone, Serialize)]
pub struct AreaRecord {
    /// Stable identifier: `srn-<kind>-<first itinerary id>`.
    pub id: String,
    /// Name.
    pub name: String,
    /// Where the name comes from: `known` (`data/names.json`), `route`, `commune`.
    pub name_source: String,
    /// `domaine` or `traversee`.
    pub kind: Kind,
    /// `srn` when an itinerary is explicitly SRN, `compatible` when only easy ski touring.
    pub evidence: String,
    /// Massif group of the département.
    pub massif: String,
    /// Commune of the representative point.
    pub commune: Commune,
    /// Representative point (an itinerary point nearest to their centroid).
    pub lon: f64,
    /// Representative point.
    pub lat: f64,
    /// Footprint area, km².
    pub area_km2: f64,
    /// Total length of the seeding itineraries, km (overlaps counted twice).
    pub km_routes: f64,
    /// Total length of the attached itineraries, km.
    pub km_attached: f64,
    /// Seeding itineraries.
    pub routes: Vec<String>,
    /// Names of the seeding itineraries, longest first.
    pub route_names: Vec<String>,
    /// Attached itineraries.
    pub attached: Vec<String>,
    /// Providers of the itineraries.
    pub providers: Vec<String>,
    /// Licences of the itineraries.
    pub licences: Vec<String>,
    /// Groomed domains overlapping the footprint.
    pub stations: Vec<StationOverlap>,
    /// Other areas crossing this one (a traverse through a play area).
    pub crosses: Vec<String>,
    /// Traverse: length of the itinerary, km.
    pub length_km: Option<f64>,
    /// Traverse: length published by the source, km.
    pub published_length_km: Option<f64>,
    /// Footprint, WGS84.
    #[serde(skip)]
    pub footprint: MultiPolygon<f64>,
}

/// What [`build`] wrote.
#[derive(Debug, Clone)]
pub struct SrnOutput {
    /// Layer directory (`data/out/france/srn`).
    pub dir: PathBuf,
    /// Areas, in output order.
    pub areas: Vec<AreaRecord>,
    /// Key figures (also in the meta file).
    pub stats: Value,
}

/// Downloads the planet extract and builds the layer.
pub fn build(layout: &DataLayout, dl: &Downloader) -> Result<SrnOutput> {
    let planet = load_planet(dl)?;
    build_from(layout, dl, &planet)
}

fn fetch_commune(dl: &Downloader, p: LonLat) -> Result<Option<Commune>> {
    // Same URL as the census (P3): shares its cache entries.
    let url = format!(
        "https://geo.api.gouv.fr/communes?lat={:.5}&lon={:.5}&fields=nom,code,codeDepartement&format=json",
        p.lat, p.lon
    );
    let f = dl.fetch(&Request::get("geo-api", &url).max_age(Duration::from_secs(30 * 86_400)))?;
    let v: Vec<Commune> = f.json()?;
    Ok(v.into_iter().next())
}

fn slug(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_owned()
}

fn multipolygon_json(mp: &MultiPolygon<f64>) -> Value {
    let polys: Vec<Value> =
        mp.0.iter()
            .map(|p| {
                let ring = |ls: &geo_types::LineString<f64>| -> Value {
                    Value::Array(ls.0.iter().map(|c| json!([c.x, c.y])).collect())
                };
                let mut rings = vec![ring(p.exterior())];
                rings.extend(p.interiors().iter().map(ring));
                Value::Array(rings)
            })
            .collect();
    json!({"type": "MultiPolygon", "coordinates": polys})
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

/// Builds the layer from an already read planet extract.
pub fn build_from(layout: &DataLayout, dl: &Downloader, planet: &Planet) -> Result<SrnOutput> {
    let t0 = Instant::now();
    // Groomed domains (P3).
    let dom_dir = layout.france_layer_dir(DOMAINES_LAYER)?;
    let dom_path = dom_dir.join("domaines.geojson");
    if !dom_path.exists() {
        tracing::info!("domaines.geojson missing: running the census (P3)");
        build_france_from(layout, dl, planet, &CensusOptions::default())?;
    }
    let dom_body = std::fs::read(&dom_path).map_err(|e| SrnError::io(&dom_path, e))?;
    let dom: Value = serde_json::from_slice(&dom_body)
        .map_err(|e| SrnError::format("domaines.geojson", e.to_string()))?;
    let stations = areas::stations_from_geojson(&dom)?;
    let dom_meta = LayerMeta::read(&dom_dir.join(LayerMeta::file_name(DOMAINES_LAYER))).ok();

    // Sources.
    let t = Instant::now();
    let mut routes = osm::routes(&planet.data);
    let t_osm = t.elapsed().as_secs_f64();
    let t = Instant::now();
    let snow_boxes: Vec<_> = dom
        .get("features")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|f| f.get("geometry").and_then(geotrek::geometry_bbox))
        .collect();
    let gt = geotrek::fetch(dl, &snow_boxes)?;
    let t_geotrek = t.elapsed().as_secs_f64();
    let t = Instant::now();
    let c2c = camptocamp::fetch(dl)?;
    let t_c2c = t.elapsed().as_secs_f64();
    let t = Instant::now();
    let dt = datatourisme::fetch(dl)?;
    let t_dt = t.elapsed().as_secs_f64();
    let mut per_provider_in: BTreeMap<String, (usize, f64)> = BTreeMap::new();
    routes.extend(gt.routes.iter().cloned());
    routes.extend(c2c.routes.iter().cloned());
    routes.extend(dt.routes.iter().cloned());
    for r in &routes {
        let e = per_provider_in
            .entry(format!("{}:{}", r.provider.as_str(), r.activity.as_str()))
            .or_default();
        e.0 += 1;
        e.1 += r.length_km;
    }

    // Areas.
    let t = Instant::now();
    let groomed = osm::groomed_lines(&planet.set);
    let placed = areas::place(routes, &stations, &groomed)?;
    let geoms = areas::build_areas(&placed)?;
    let t_areas = t.elapsed().as_secs_f64();
    let names = known_names()?;

    let t = Instant::now();
    let mut records: Vec<(AreaRecord, &AreaGeom)> = Vec::new();
    let mut foreign = 0usize;
    for g in &geoms {
        // The representative point first; a border traverse (the GTJ runs along the
        // Swiss border) is French when one of its sample points is.
        let mut commune = None;
        for p in areas::sample_points(g, &placed)? {
            if let Some(c) = fetch_commune(dl, p)? {
                commune = Some((p, c));
                break;
            }
        }
        let Some((rep, commune)) = commune else {
            foreign += 1;
            continue;
        };
        records.push((record(g, &placed, &stations, rep, commune)?, g));
    }
    apply_known_names(records.iter_mut().map(|(r, _)| r), &names);
    let t_communes = t.elapsed().as_secs_f64();
    // Crossings between kept areas.
    for i in 0..records.len() {
        for j in 0..records.len() {
            if i != j && records[i].1.shape.intersects(&records[j].1.shape) {
                let id = records[j].0.id.clone();
                records[i].0.crosses.push(id);
            }
        }
    }
    records.sort_by(|a, b| {
        (
            a.0.massif.as_str(),
            a.0.kind,
            a.0.name.as_str(),
            a.0.id.as_str(),
        )
            .cmp(&(
                b.0.massif.as_str(),
                b.0.kind,
                b.0.name.as_str(),
                b.0.id.as_str(),
            ))
    });
    let records: Vec<AreaRecord> = records.into_iter().map(|(r, _)| r).collect();

    // Itineraries of the kept areas.
    // A traverse of a plateau seeds both its linear area and the play area.
    let mut area_of: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for a in &records {
        for (list, role) in [(&a.routes, "seed"), (&a.attached, "attached")] {
            for r in list {
                area_of
                    .entry(r.clone())
                    .or_default()
                    .push(json!({"id": a.id, "role": role}));
            }
        }
    }
    let mut route_features = Vec::new();
    for p in &placed {
        if let Some(areas) = area_of.get(&p.route.id) {
            route_features.push(route_feature(p, areas));
        }
    }

    // Hints.
    let mut hints: Vec<&Hint> = c2c.without_detail.iter().collect();
    hints.extend(dt.without_track.iter());
    let mut hint_features = Vec::new();
    let mut hints_outside = Vec::new();
    for h in &hints {
        let pt = Point::new(h.pos.lon, h.pos.lat);
        let area = records
            .iter()
            .find(|a| a.footprint.contains(&pt))
            .map(|a| a.id.clone());
        if area.is_none() {
            hints_outside
                .push(json!({"id": h.id, "name": h.name, "activity": h.activity.as_str()}));
        }
        hint_features.push(feature(
            point(h.pos),
            json!({
                "id": h.id,
                "name": h.name,
                "provider": h.provider.as_str(),
                "url": h.url,
                "activity": h.activity.as_str(),
                "reason": h.reason,
                "area": area,
            }),
        ));
    }

    let area_features: Vec<Value> = records
        .iter()
        .map(|a| {
            let mut props = serde_json::to_value(a).unwrap_or(Value::Null);
            if let Some(o) = props.as_object_mut() {
                o.insert("area_km2".into(), json!(round2(a.area_km2)));
                o.insert("km_routes".into(), json!(round2(a.km_routes)));
                o.insert("km_attached".into(), json!(round2(a.km_attached)));
            }
            feature(multipolygon_json(&a.footprint), props)
        })
        .collect();

    let dir = layout.france_layer_dir(LAYER)?;
    DataLayout::ensure(&dir)?;
    write_geojson(&dir.join(AREAS_FILE), &area_features)?;
    write_geojson(&dir.join(ROUTES_FILE), &route_features)?;
    write_geojson(&dir.join(HINTS_FILE), &hint_features)?;

    let stats = stats(StatsInput {
        records: &records,
        placed: &placed,
        per_provider_in: &per_provider_in,
        gt: &gt,
        c2c: &c2c,
        dt: &dt,
        foreign,
        hints: hints.len(),
        hints_outside,
        timing: json!({
            "osm_s": round2(t_osm), "geotrek_s": round2(t_geotrek), "camptocamp_s": round2(t_c2c),
            "datatourisme_s": round2(t_dt), "areas_s": round2(t_areas),
            "communes_s": round2(t_communes), "total_s": round2(t0.elapsed().as_secs_f64()),
        }),
    });
    let meta = layer_meta(
        planet,
        &gt,
        &dt,
        dom_meta.as_ref(),
        &records,
        &stats,
        &route_features,
        hint_features.len(),
    );
    meta.write(&dir)?;
    Ok(SrnOutput {
        dir,
        areas: records,
        stats,
    })
}

/// Gives each hand-kept name to the one play area whose representative point is the
/// nearest to its point, within its radius.
pub fn apply_known_names<'a>(
    records: impl Iterator<Item = &'a mut AreaRecord>,
    names: &[KnownName],
) {
    let mut records: Vec<&mut AreaRecord> = records.collect();
    for k in names {
        let at = LonLat::new(k.lon, k.lat);
        let best = records
            .iter_mut()
            .filter(|r| r.kind == Kind::Domaine)
            .map(|r| (LonLat::new(r.lon, r.lat).haversine_m(at), r))
            .filter(|(d, _)| *d <= k.radius_km * 1000.0)
            .min_by(|a, b| a.0.total_cmp(&b.0));
        if let Some((_, r)) = best {
            r.name = k.name.clone();
            r.name_source = "known".into();
        }
    }
}

fn record(
    g: &AreaGeom,
    placed: &[Placed],
    stations: &[areas::Station],
    rep: LonLat,
    commune: Commune,
) -> Result<AreaRecord> {
    let mut seeds: Vec<&Placed> = g.seeds.iter().map(|&i| &placed[i]).collect();
    seeds.sort_by(|a, b| a.route.id.cmp(&b.route.id));
    let first = &seeds[0].route;
    let mut by_len = seeds.clone();
    by_len.sort_by(|a, b| b.route.length_km.total_cmp(&a.route.length_km));
    let route_names: Vec<String> =
        by_len
            .iter()
            .filter_map(|p| p.route.name.clone())
            .fold(Vec::new(), |mut v, n| {
                if !v.contains(&n) {
                    v.push(n);
                }
                v
            });
    let (name, name_source) = match g.kind {
        Kind::Traversee => (
            first.name.clone().unwrap_or_else(|| commune.name.clone()),
            if first.name.is_some() {
                "route"
            } else {
                "commune"
            },
        ),
        // Hand-kept names are applied afterwards ([`apply_known_names`]).
        Kind::Domaine if seeds.len() == 1 && first.name.is_some() => {
            (first.name.clone().unwrap_or_default(), "route")
        }
        Kind::Domaine => (commune.name.clone(), "commune"),
    };
    let set = |f: &dyn Fn(&Placed) -> String| -> Vec<String> {
        g.seeds
            .iter()
            .chain(&g.attached)
            .map(|&i| f(&placed[i]))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    };
    let footprint = areas::footprint_wgs84(g)?;
    let traverse = g.kind == Kind::Traversee;
    Ok(AreaRecord {
        id: format!("srn-{}-{}", g.kind.as_str(), slug(&first.id)),
        name,
        name_source: name_source.into(),
        kind: g.kind,
        evidence: if seeds.iter().any(|p| p.route.activity == Activity::Srn) {
            "srn".into()
        } else {
            "compatible".into()
        },
        massif: massif_of(&commune.departement).into(),
        commune,
        lon: round7(rep.lon),
        lat: round7(rep.lat),
        area_km2: g.shape.unsigned_area() / 1e6,
        // `+ 0.0`: an empty f64 sum is -0.0.
        km_routes: seeds.iter().map(|p| p.route.length_km).sum::<f64>() + 0.0,
        km_attached: g
            .attached
            .iter()
            .map(|&i| placed[i].route.length_km)
            .sum::<f64>()
            + 0.0,
        routes: seeds.iter().map(|p| p.route.id.clone()).collect(),
        route_names,
        attached: g
            .attached
            .iter()
            .map(|&i| placed[i].route.id.clone())
            .collect(),
        providers: set(&|p| p.route.provider.as_str().to_owned()),
        licences: set(&|p| p.route.licence.clone()),
        stations: areas::station_overlaps(g, placed, stations)
            .into_iter()
            .map(|mut s| {
                s.area_share = round2(s.area_share);
                s.km_inside = round2(s.km_inside);
                s
            })
            .collect(),
        crosses: Vec::new(),
        length_km: traverse.then(|| round2(first.length_km)),
        published_length_km: if traverse {
            first.published_length_km.map(round2)
        } else {
            None
        },
        footprint,
    })
}

fn route_feature(p: &Placed, areas: &[Value]) -> Value {
    let r = &p.route;
    let geometry = if r.lines.len() == 1 {
        line_string(r.lines[0].iter().copied())
    } else {
        json!({
            "type": "MultiLineString",
            "coordinates": r.lines.iter().map(|l| l.iter().map(|q| json!([round7(q.lon), round7(q.lat)])).collect::<Vec<_>>()).collect::<Vec<_>>(),
        })
    };
    feature(
        geometry,
        json!({
            "id": r.id,
            "name": r.name,
            "provider": r.provider.as_str(),
            "publisher": r.publisher,
            "url": r.url,
            "licence": r.licence,
            "attribution": r.attribution,
            "activity": r.activity.as_str(),
            "shape": r.shape.as_str(),
            "length_km": round2(r.length_km),
            "published_length_km": r.published_length_km.map(round2),
            "ascent_m": r.ascent_m,
            "difficulty": r.difficulty,
            "areas": areas,
            "groomed_share": round2(p.groomed_share),
            "stations": p.stations.iter().map(|(id, s)| json!({"id": id, "share": round2(*s)})).collect::<Vec<_>>(),
        }),
    )
}

struct StatsInput<'a> {
    records: &'a [AreaRecord],
    placed: &'a [Placed],
    per_provider_in: &'a BTreeMap<String, (usize, f64)>,
    gt: &'a geotrek::Fetch,
    c2c: &'a camptocamp::Fetch,
    dt: &'a datatourisme::Fetch,
    foreign: usize,
    hints: usize,
    hints_outside: Vec<Value>,
    timing: Value,
}

fn stats(s: StatsInput<'_>) -> Value {
    let mut by_massif: BTreeMap<&str, BTreeMap<&str, (usize, f64, f64)>> = BTreeMap::new();
    for a in s.records {
        let e = by_massif
            .entry(a.massif.as_str())
            .or_default()
            .entry(a.kind.as_str())
            .or_default();
        e.0 += 1;
        e.1 += a.km_routes;
        e.2 += a.km_attached;
    }
    let by_massif: BTreeMap<&str, Value> =
        by_massif
            .into_iter()
            .map(|(m, kinds)| {
                let v: BTreeMap<&str, Value> = kinds
                .into_iter()
                .map(|(k, (n, km, att))| {
                    (k, json!({"areas": n, "km_routes": round2(km), "km_attached": round2(att)}))
                })
                .collect();
                (m, json!(v))
            })
            .collect();
    let kind_count = |k: Kind| s.records.iter().filter(|a| a.kind == k).count();
    // Each seeding itinerary once (a plateau traverse seeds its linear area and the play
    // area), per massif of its first area.
    let length: BTreeMap<&str, f64> = s
        .placed
        .iter()
        .map(|p| (p.route.id.as_str(), p.route.length_km))
        .collect();
    let mut distinct: BTreeMap<&str, (&str, f64)> = BTreeMap::new();
    for a in s.records {
        for r in &a.routes {
            distinct.entry(r.as_str()).or_insert((
                a.massif.as_str(),
                length.get(r.as_str()).copied().unwrap_or(0.0),
            ));
        }
    }
    let mut distinct_by_massif: BTreeMap<&str, f64> = BTreeMap::new();
    for (m, km) in distinct.values() {
        *distinct_by_massif.entry(m).or_default() += km;
    }
    let distinct_by_massif: BTreeMap<&str, f64> = distinct_by_massif
        .into_iter()
        .map(|(m, km)| (m, round2(km)))
        .collect();
    let in_areas: BTreeSet<&str> = s
        .records
        .iter()
        .flat_map(|a| a.routes.iter().chain(&a.attached).map(String::as_str))
        .collect();
    let mut per_provider_kept: BTreeMap<String, (usize, f64)> = BTreeMap::new();
    let mut dropped: BTreeMap<String, usize> = BTreeMap::new();
    let mut seeds_unused = 0usize;
    for p in s.placed {
        let key = format!(
            "{}:{}",
            p.route.provider.as_str(),
            p.route.activity.as_str()
        );
        if p.dropped.is_some() {
            *dropped.entry(key.clone()).or_default() += 1;
        }
        if in_areas.contains(p.route.id.as_str()) {
            let e = per_provider_kept.entry(key).or_default();
            e.0 += 1;
            e.1 += p.route.length_km;
        } else if p.route.activity.seeds() && p.dropped.is_none() {
            seeds_unused += 1;
        }
    }
    let pp = |m: &BTreeMap<String, (usize, f64)>| -> Value {
        json!(
            m.iter()
                .map(|(k, (n, km))| (k.clone(), json!({"routes": n, "km": round2(*km)})))
                .collect::<BTreeMap<_, _>>()
        )
    };
    let instances = &s.gt.instances;
    let geotrek = json!({
        "instances": instances.len(),
        "near_groomed_domains": instances.iter().filter(|i| i.snowy).count(),
        "with_api": instances.iter().filter(|i| i.api.is_some()).count(),
        "unreadable": instances.iter().filter(|i| i.snowy && i.api.is_none()).map(|i| json!({"name": i.name, "error": i.error})).collect::<Vec<_>>(),
        "not_read_no_domain_nearby": instances.iter().filter(|i| !i.snowy).map(|i| i.name.as_str()).collect::<Vec<_>>(),
        "srn_practices": instances.iter().flat_map(|i| i.kept.iter().filter(|k| k.2 == Activity::Srn).map(move |k| json!({"instance": i.name, "api": i.api, "practice": k.1, "treks": i.treks.get(&k.0)}))).collect::<Vec<_>>(),
        "snowshoe_practices_counted": instances.iter().flat_map(|i| i.kept.iter().filter(|k| k.2 == Activity::Snowshoe).map(move |k| json!({"instance": i.name, "api": i.api, "practice": k.1, "treks": i.treks.get(&k.0)}))).collect::<Vec<_>>(),
    });
    json!({
        "areas": s.records.len(),
        "domaines": kind_count(Kind::Domaine),
        "traversees": kind_count(Kind::Traversee),
        "evidence_srn": s.records.iter().filter(|a| a.evidence == "srn").count(),
        "evidence_compatible": s.records.iter().filter(|a| a.evidence == "compatible").count(),
        "km_routes": round2(s.records.iter().map(|a| a.km_routes).sum()),
        "seeding_routes_distinct": distinct.len(),
        "km_seeding_routes_distinct": round2(distinct.values().map(|(_, km)| km).sum()),
        "km_seeding_routes_distinct_by_massif": distinct_by_massif,
        "km_attached": round2(s.records.iter().map(|a| a.km_attached).sum()),
        "touching_stations": s.records.iter().filter(|a| !a.stations.is_empty()).count(),
        "by_massif": by_massif,
        "areas_outside_france": s.foreign,
        "routes_read": pp(s.per_provider_in),
        "routes_in_areas": pp(&per_provider_kept),
        "routes_dropped_groomed": dropped,
        "routes_dropped_groomed_not_osm": s.placed.iter().filter(|p| p.dropped.is_some() && p.route.provider != Provider::Osm).map(|p| json!({"id": p.route.id, "name": p.route.name, "groomed_share": round2(p.groomed_share)})).collect::<Vec<_>>(),
        "seeding_routes_outside_kept_areas": seeds_unused,
        "geotrek": geotrek,
        "camptocamp": {"listed": s.c2c.listed, "kept_without_track": s.c2c.without_detail.len(), "with_track": s.c2c.routes.len()},
        "datatourisme": {"tours": s.dt.tours, "candidates": s.dt.candidates, "with_track": s.dt.routes.len(), "without_track": s.dt.without_track.len(), "rejected": s.dt.rejected.iter().map(|(n, u, w)| json!({"name": n, "uri": u, "why": w})).collect::<Vec<_>>()},
        "hints": s.hints,
        "hints_outside_areas": s.hints_outside,
        "timing": s.timing,
    })
}

#[allow(clippy::too_many_arguments)]
fn layer_meta(
    planet: &Planet,
    gt: &geotrek::Fetch,
    dt: &datatourisme::Fetch,
    dom_meta: Option<&LayerMeta>,
    records: &[AreaRecord],
    stats: &Value,
    route_features: &[Value],
    hints: usize,
) -> LayerMeta {
    let mut m = LayerMeta::new(LAYER, FRANCE);
    m.tickets = vec!["G5 (#131)".into()];
    m.valid_from = planet.valid_at.clone();
    m.validity =
        Some("une saison : les itinéraires changent peu ; à reconstruire chaque automne".into());
    m.crs = Some("EPSG:4326".into());
    m.sources.push(planet.source.clone());
    if let Some(f) = &gt.list {
        m.sources.push(
            SourceRef::from_fetched(
                "Liste officielle des instances Geotrek",
                "non indiquée",
                "Geotrek (Makina Corpus et utilisateurs)",
                f,
            )
            .note("liste des instances et de leurs sites ; l'API de chacune est lue sur son site"),
        );
    }
    let used: BTreeSet<(&str, &str)> = gt
        .routes
        .iter()
        .map(|r| {
            (
                r.publisher.as_str(),
                r.url.split("/trek/").next().unwrap_or_default(),
            )
        })
        .collect();
    for (publisher, api) in used {
        m.sources.push(SourceRef::new(
            format!("Geotrek {publisher}"),
            format!("{api}/trek/"),
            geotrek::licence_of(api),
            format!("© {publisher} (Geotrek)"),
        ));
    }
    m.sources.push(
        SourceRef::new(
            "Camptocamp, itinéraires",
            format!("{}/routes", camptocamp::API),
            camptocamp::LICENCE,
            camptocamp::ATTRIBUTION,
        )
        .note(format!(
            "listes : {} ; licence portée par chaque document (champ license) ; auteurs sur la page d'historique de chaque itinéraire",
            camptocamp::listings().join(" | ")
        )),
    );
    if let Some(f) = &dt.csv {
        m.sources.push(
            SourceRef::from_fetched(
                "DATAtourisme, datatourisme-tour.csv",
                datatourisme::LICENCE,
                "DATAtourisme ; paternité : producteur de chaque fiche (hasBeenCreatedBy), date de mise à jour",
                f,
            )
            .note("fiches et GPX lus sur data.datatourisme.fr (?format=jsonld, ebucore:locator)"),
        );
    }
    let producers: BTreeSet<&str> = dt.routes.iter().map(|r| r.attribution.as_str()).collect();
    for p in producers {
        m.sources.push(SourceRef::new(
            format!("DATAtourisme : {p}"),
            "https://data.datatourisme.fr/",
            datatourisme::LICENCE,
            p,
        ));
    }
    m.sources.push(SourceRef::new(
        "geo.api.gouv.fr communes",
        "https://geo.api.gouv.fr/communes",
        "Licence Ouverte 2.0",
        "geo.api.gouv.fr (Etalab, IGN Admin Express)",
    ));
    m.sources.push(
        SourceRef::new(
            "Recensement des domaines de fond (P3)",
            "data/out/france/domaines/domaines.geojson",
            "ODbL 1.0",
            skimap_osm::extract::ATTRIBUTION,
        )
        .edition(dom_meta.map(|d| d.generated_at.clone()).unwrap_or_default()),
    );
    let count = |p: Provider| {
        records
            .iter()
            .filter(|a| a.providers.iter().any(|x| x == p.as_str()))
            .count()
    };
    m.files = vec![
        FileMeta::new(AREAS_FILE, "domaines de jeu SRN (fermeture des itinéraires : tampon 750 m puis −250 m, réunie au couloir de 500 m) et traversées (couloir de 500 m)")
            .count(records.len())
            .licence("ODbL 1.0 (base dérivée d'OSM) ; voir licences de chaque zone")
            .properties([
                ("id", "srn-<kind>-<identifiant du premier itinéraire>"),
                ("name", "nom (name_source : known = data/names.json, route = nom de l'itinéraire, commune)"),
                ("kind", "domaine (groupe d'itinéraires à moins de 1,5 km) ou traversee (itinéraire point à point d'au moins 25 km)"),
                ("evidence", "srn : au moins un itinéraire dit SRN ; compatible : seulement du ski de randonnée facile (Camptocamp ≤ 1.3, ≤ 500 m D+)"),
                ("massif", "groupe de massifs du département (Jura, Alpes du Nord, Alpes du Sud, Massif central, Vosges, Pyrénées, Corse, Ardennes et Morvan, autre)"),
                ("commune", "commune du point représentatif (name, insee, departement)"),
                ("lon, lat", "point représentatif : point d'itinéraire le plus proche du barycentre"),
                ("area_km2", "surface de l'emprise"),
                ("km_routes", "longueur cumulée des itinéraires de la zone (recouvrements comptés deux fois)"),
                ("km_attached", "longueur des itinéraires rattachés (raquettes, ski de randonnée)"),
                ("routes, route_names, attached", "identifiants des itinéraires (srn_routes.geojson), noms du plus long au plus court"),
                ("providers, licences", "fournisseurs et licences des itinéraires"),
                ("stations", "domaines damés (P3) recoupés : id, name, area_share (part de l'emprise), km_inside (km d'itinéraires dedans)"),
                ("crosses", "autres zones recoupées (traversée qui passe dans un domaine)"),
                ("length_km, published_length_km", "traversée : longueur du tracé et longueur publiée par la source"),
            ]),
        FileMeta::new(ROUTES_FILE, "itinéraires des zones (WGS84)")
            .count(route_features.len())
            .licence("licence de chaque itinéraire (propriété licence)")
            .properties([
                ("id", "osm:relation/<id>, osm:way/<premier way>, geotrek:<hôte>:<id>, camptocamp:<id>, datatourisme:<chemin de l'URI>"),
                ("provider, publisher, url, licence, attribution", "provenance"),
                ("activity", "srn, easy_ski_touring (amorcent une zone), snowshoe, ski_touring (rattachés seulement)"),
                ("shape", "traverse, loop, out_and_back, network"),
                ("length_km, published_length_km, ascent_m, difficulty", "longueur du tracé (haversine) et valeurs publiées"),
                ("areas", "zones de l'itinéraire : id et role (seed : amorce la zone, attached : rattaché) ; une traversée d'un plateau amorce sa zone linéaire et le domaine"),
                ("groomed_share, stations", "part du tracé à moins de 30 m d'une piste nordique damée (OSM) ; part dans chaque enveloppe de domaine damé (P3)"),
            ]),
        FileMeta::new(HINTS_FILE, "documents qui décrivent un itinéraire SRN ou de ski de randonnée facile sans tracé (points à compléter)")
            .count(hints)
            .licence("CC BY-SA 3.0 (Camptocamp), Licence Ouverte 2.0 (DATAtourisme)")
            .properties([
                ("id, name, provider, url", "document"),
                ("activity", "classe qu'aurait l'itinéraire"),
                ("reason", "pourquoi il n'y a pas de tracé"),
                ("area", "zone qui contient le point, sinon null"),
            ]),
    ];
    m.notes = vec![
        "Itinéraires SRN : OSM piste:type=nordic + piste:grooming=backcountry (voies et relations), pratiques Geotrek nommées « nordique » (seul le PNR du Vercors en a une sur 81 instances), Camptocamp (titre « nordique » ou « ski de fond », cotation ≤ 2.1), DATAtourisme (nom « nordique » et thème SkiTouring, GPX de la fiche).".into(),
        "Ski de randonnée facile : Camptocamp skitouring cotation 1.1 à 1.3 et au plus 500 m de dénivelé : amorce une zone (evidence compatible). Raquettes (OSM hike/snowshoe) et autre ski de randonnée (OSM skitour, Camptocamp plus raide, DATAtourisme « ski de randonnée ») : rattachés à une zone qui en contient au moins la moitié, jamais d'amorce.".into(),
        format!("Hors stations : un itinéraire (amorce ou rattaché) dont au moins {:.0} % des points sont à moins de {} m d'une piste nordique damée (OSM, classement P2) est écarté, sauf OSM backcountry ; les enveloppes des domaines (P3) contiennent les forêts entre les pistes, où se pratique le SRN : elles ne servent qu'à lister les domaines recoupés (stations).", areas::GROOMED_DROP_SHARE * 100.0, areas::ON_PISTE_M),
        "France : une zone dont le point représentatif n'a pas de commune française (geo.api.gouv.fr) est écartée.".into(),
        "Snowshoe Geotrek et DATAtourisme : seulement comptés (stats), pas de tracé : licences non affichées (Geotrek) ou centaines de requêtes pour des itinéraires seulement rattachés (DATAtourisme).".into(),
        "Zones propices calculées (pente, forêt, neige, accès) : non faites, il faudrait le relief et la neige de toute la France ; suite à donner (voir docs/srn.md).".into(),
        format!("Distances : lien {} m, fermeture {} m, couloir {} m, traversée ≥ {} km, itinéraire groupé < {} km, domaine ≥ {} km d'itinéraires.", areas::LINK_M, areas::CLOSE_M, areas::CORRIDOR_M, areas::TRAVERSE_AREA_MIN_KM, areas::DOMAIN_MAX_ROUTE_KM, areas::DOMAIN_MIN_KM),
    ];
    m.parameters = json!({
        "densify_m": areas::DENSIFY_M,
        "groomed_drop_share": areas::GROOMED_DROP_SHARE,
        "on_piste_m": areas::ON_PISTE_M,
        "domain_min_km": areas::DOMAIN_MIN_KM,
        "traverse_area_min_km": areas::TRAVERSE_AREA_MIN_KM,
        "domain_max_route_km": areas::DOMAIN_MAX_ROUTE_KM,
        "link_m": areas::LINK_M,
        "corridor_m": areas::CORRIDOR_M,
        "close_m": areas::CLOSE_M,
        "min_part_m2": areas::MIN_PART_M2,
        "min_hole_m2": areas::MIN_HOLE_M2,
        "attach_share": areas::ATTACH_SHARE,
        "simplify_m": areas::SIMPLIFY_M,
        "camptocamp_easy_max_rating": camptocamp::EASY_MAX_RATING,
        "camptocamp_easy_max_ascent_m": camptocamp::EASY_MAX_ASCENT_M,
        "camptocamp_nordic_max_rating": camptocamp::NORDIC_MAX_RATING,
        "areas_with_osm": count(Provider::Osm),
    });
    m.stats = stats.clone();
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers() {
        assert_eq!(massif_of("25"), "Jura");
        assert_eq!(massif_of("26"), "Alpes du Sud");
        assert_eq!(massif_of("88"), "Vosges");
        assert_eq!(massif_of("75"), "autre");
        assert_eq!(
            slug("geotrek:geo.pnr-vercors.fr:29411"),
            "geotrek-geo-pnr-vercors-fr-29411"
        );
        assert_eq!(slug("osm:relation/12"), "osm-relation-12");
        let n = known_names().unwrap();
        assert!(n.iter().all(|k| k.radius_km > 0.0 && !k.source.is_empty()));
    }

    fn rec(id: &str, kind: Kind, lon: f64, lat: f64) -> AreaRecord {
        AreaRecord {
            id: id.into(),
            name: id.into(),
            name_source: "commune".into(),
            kind,
            evidence: "srn".into(),
            massif: "Jura".into(),
            commune: Commune {
                name: "X".into(),
                insee: "00000".into(),
                departement: "25".into(),
            },
            lon,
            lat,
            area_km2: 1.0,
            km_routes: 1.0,
            km_attached: 0.0,
            routes: vec![],
            route_names: vec![],
            attached: vec![],
            providers: vec![],
            licences: vec![],
            stations: vec![],
            crosses: vec![],
            length_km: None,
            published_length_km: None,
            footprint: MultiPolygon::new(vec![]),
        }
    }

    #[test]
    fn known_names_go_to_the_nearest_play_area_only() {
        let mut r = [
            rec("near", Kind::Domaine, 5.50, 44.90),
            rec("other", Kind::Domaine, 5.40, 44.90),
            rec("trav", Kind::Traversee, 5.50, 44.896),
            rec("far", Kind::Domaine, 6.0, 45.5),
        ];
        let names = vec![KnownName {
            name: "Hauts-Plateaux".into(),
            lon: 5.50,
            lat: 44.896,
            radius_km: 15.0,
            source: "test".into(),
        }];
        apply_known_names(r.iter_mut(), &names);
        let named: Vec<&str> = r
            .iter()
            .filter(|a| a.name_source == "known")
            .map(|a| a.id.as_str())
            .collect();
        assert_eq!(named, vec!["near"]);
    }
}
