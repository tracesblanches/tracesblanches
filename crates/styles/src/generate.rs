//! Writes the six styles (`fond`, `srn`, `guidage` × `clair`, `sombre`) from the same
//! tiles and layers of the pack.
//!
//! Every URL is relative to the pack root, as in the basemap styles of the pack
//! (`pmtiles://basemap/plan-ign.pmtiles`, `layers/pistes.geojson`): the viewer and the app
//! make them absolute. Sources marked as runtime (`position`, `route`, `decision`) start
//! empty and are filled by the app (and the viewer) with `setData`.
//!
//! Density is reduced selectively from the IGN « standard » style (434 layers): by class
//! of object (`symbo`, `txt_typo`) and by zoom, for each practice.

use serde_json::{Map, Value, json};

use skimap_zones::ZoneKind;

use crate::palette::{Palette, Theme};

/// The three styles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Groomed trails first: pistes, their state and direction, discreet relief, few names.
    Fond,
    /// Nordic backcountry: contours, slopes, avalanche exposure, forest, zones, huts,
    /// network; pistes second.
    Srn,
    /// Glance while moving: route of the day, next decision, position.
    Guidage,
}

impl Kind {
    /// The three styles.
    pub const ALL: [Kind; 3] = [Kind::Fond, Kind::Srn, Kind::Guidage];

    /// Name used in file names and metadata.
    pub fn name(self) -> &'static str {
        match self {
            Kind::Fond => "fond",
            Kind::Srn => "srn",
            Kind::Guidage => "guidage",
        }
    }

    /// Title, French.
    pub fn title(self) -> &'static str {
        match self {
            Kind::Fond => "Fond (pistes damées)",
            Kind::Srn => "SRN (hors piste)",
            Kind::Guidage => "Guidage",
        }
    }
}

/// File name of a style (`fond-clair.json`).
pub fn file_name(kind: Kind, theme: Theme) -> String {
    format!("{}-{}.json", kind.name(), theme.name())
}

/// Glyphs of the pack (Plan IGN fonts: Source Sans Pro).
pub const GLYPHS: &str = "basemap/plan-ign/fonts/{fontstack}/{range}.pbf";
/// Sprite of the pack (Plan IGN: hut icons).
pub const SPRITE: &str = "basemap/plan-ign/sprite/PlanIgn";

const REGULAR: &str = "Source Sans Pro Regular";
const SEMIBOLD: &str = "Source Sans Pro Semibold";
const BOLD: &str = "Source Sans Pro Bold";
const ITALIC: &str = "Source Sans Pro Italic";

/// Empty GeoJSON, the initial data of runtime sources.
fn empty() -> Value {
    json!({"type": "FeatureCollection", "features": []})
}

/// Every source a style may use; [`style`] keeps only those its layers reference.
fn all_sources() -> Map<String, Value> {
    let geojson = |file: &str| json!({"type": "geojson", "data": file});
    let runtime = || json!({"type": "geojson", "data": empty()});
    let mut m = Map::new();
    m.insert(
        "ign".into(),
        json!({"type": "vector", "url": "pmtiles://basemap/plan-ign.pmtiles",
               "attribution": "© IGN Plan IGN v2"}),
    );
    m.insert(
        "osm".into(),
        json!({"type": "vector", "url": "pmtiles://basemap/osm.pmtiles",
               "attribution": "Protomaps © OpenStreetMap"}),
    );
    for id in [
        "pistes",
        "zones",
        "no-network",
        "avalanche-areas",
        "avalanche-segments",
        "refuges-info",
        "refuges-osm",
        "refuges-camptocamp",
        "piste-labels",
        "srn-areas",
        "srn-routes",
        "fee-zones",
        "stations-pois",
    ] {
        m.insert(id.into(), geojson(&format!("layers/{id}.geojson")));
    }
    // The app marks the zones of the pass of the user by feature state, keyed by pass.
    m["fee-zones"]["promoteId"] = json!("fee_id");
    // Class polygons of the pack (vector tiles), drawn in the colours of the theme.
    for id in ["slope", "avalanche-exposure", "snow-groomed", "snow-srn"] {
        m.insert(
            id.into(),
            json!({"type": "vector", "url": format!("pmtiles://layers/{id}.pmtiles")}),
        );
    }
    for id in ["position", "route", "decision"] {
        m.insert(id.into(), runtime());
    }
    m
}

/// Sources a pack may lack: the app and the viewer then leave out the source and its
/// layers (`metadata.skimap:optional_sources`). Layers of personal-use sources (CLPA,
/// and the exposure fused with it at Cervières), layers only some sites have (SRN areas
/// and itineraries, snow outside the simulated massifs, avalanche exposure outside the
/// mountains), layers added after older packs were built (piste name lines) and the
/// trail-pass zones (no producer yet). The basemaps, pistes, zones, network, huts and
/// slope are in every pack: their absence is an error.
pub const OPTIONAL_SOURCES: [&str; 10] = [
    "avalanche-areas",
    "avalanche-segments",
    "avalanche-exposure",
    "snow-groomed",
    "snow-srn",
    "srn-areas",
    "srn-routes",
    "piste-labels",
    "fee-zones",
    "stations-pois",
];

/// Feature states the app sets (`metadata.skimap:feature_states`): source → state → what
/// it means. Without them, the layers draw the neutral case.
pub fn feature_states() -> Value {
    json!({
        "fee-zones": {
            "valid": "true : le titre de l'utilisateur est valable dans la zone (« valable avec ton titre ») ; false : il faut un autre titre (« autre redevance ») ; absent : aucun titre choisi, trait neutre. Clé : fee_id (promoteId)."
        }
    })
}

/// Properties the app writes in the runtime sources (checked by the validator like the
/// properties of the pack layers).
pub fn runtime_schema() -> Value {
    json!({
        "position": {"geometry": "Point", "properties": {"accuracy_m": "number"}},
        "route": {"geometry": "LineString", "properties": {"part": ["ahead", "done"]}},
        "decision": {"geometry": "Point", "properties": {"label": "string"}}
    })
}

// ---------------------------------------------------------------------------------------
// Expression helpers.

/// `["interpolate", ["linear"], ["zoom"], z0, v0, ...]`.
fn zl(stops: &[(f64, f64)]) -> Value {
    let mut v = vec![json!("interpolate"), json!(["linear"]), json!(["zoom"])];
    for &(z, x) in stops {
        v.push(json!(z));
        v.push(json!(x));
    }
    Value::Array(v)
}

/// `["match", ["get", key], [values], true, false]`.
fn one_of(key: &str, values: &[&str]) -> Value {
    json!(["match", ["get", key], values, true, false])
}

/// `["==", ["get", key], value]`.
fn eq(key: &str, value: Value) -> Value {
    json!(["==", ["get", key], value])
}

/// Membership in an array property of a GeoJSON layer (`uses`, `practices`).
fn has_item(key: &str, item: &str) -> Value {
    json!(["in", item, ["coalesce", ["get", key], ["literal", []]]])
}

/// Piste colour by difficulty.
fn piste_color(p: &Palette) -> Value {
    json!([
        "match",
        ["coalesce", ["get", "difficulty"], ""],
        "novice",
        p.piste[0],
        "easy",
        p.piste[1],
        "intermediate",
        p.piste[2],
        "advanced",
        p.piste[3],
        p.piste[4]
    ])
}

/// Casing colour of pistes: the black piste gets a contrasting casing.
fn piste_casing(p: &Palette) -> Value {
    json!([
        "match",
        ["coalesce", ["get", "difficulty"], ""],
        "advanced",
        p.piste_black_casing,
        p.halo
    ])
}

/// Nordic pistes (groomed or not), not snowshoe / foot only.
fn nordic() -> Value {
    has_item("uses", "nordic")
}

/// Snowshoe or foot trails only.
fn hike_only() -> Value {
    json!([
        "all",
        has_item("uses", "hike"),
        ["!", has_item("uses", "nordic")]
    ])
}

/// Text paint with a strong halo (sun, gloves: no thin text without halo).
fn text_paint(color: &str, halo: &str, halo_width: f64) -> Value {
    json!({"text-color": color, "text-halo-color": halo, "text-halo-width": halo_width})
}

/// A layer from an object literal: `id`, `type`, `source` and the rest.
fn layer(id: &str, ty: &str, source: Option<&str>, rest: Value) -> Value {
    let mut m = Map::new();
    m.insert("id".into(), json!(id));
    m.insert("type".into(), json!(ty));
    if let Some(s) = source {
        m.insert("source".into(), json!(s));
    }
    if let Value::Object(r) = rest {
        m.extend(r);
    }
    Value::Object(m)
}

/// An IGN layer.
fn ign(id: &str, ty: &str, source_layer: &str, rest: Value) -> Value {
    let mut l = layer(id, ty, Some("ign"), rest);
    l["source-layer"] = json!(source_layer);
    l
}

/// An OSM (Protomaps) layer.
fn osm(id: &str, ty: &str, source_layer: &str, rest: Value) -> Value {
    let mut l = layer(id, ty, Some("osm"), rest);
    l["source-layer"] = json!(source_layer);
    l
}

/// Marks a layer for the contrast check: its colour against the colour of `against`
/// (`background` or a layer id) must reach the WCAG non-text ratio (3:1).
fn contrast(mut l: Value, against: &str) -> Value {
    l["metadata"]["skimap:contrast"] = json!(against);
    l
}

// ---------------------------------------------------------------------------------------
// Shared groups of layers.

const FOREST: [&str; 4] = [
    "ZONE_BOISEE",
    "ZONE_FORET_FERMEE_CONI",
    "ZONE_FORET_FERMEE_FEUIL",
    "ZONE_FORET_FERMEE_MIXTE",
];

fn background(p: &Palette) -> Value {
    layer(
        "background",
        "background",
        None,
        json!({"paint": {"background-color": p.ground}}),
    )
}

/// Built-up areas, forest, rock, glacier, water. `marked`: SRN (forest kinds, rock).
fn ground_cover(p: &Palette, marked: bool) -> Vec<Value> {
    let mut v = vec![ign(
        "ign-built",
        "fill",
        "bati_zone_surf",
        json!({"maxzoom": 15, "filter": eq("symbo", json!("ZONE_BATI")),
               "paint": {"fill-color": p.built}}),
    )];
    if marked {
        v.push(ign(
            "ign-forest",
            "fill",
            "ocs_vegetation_surf",
            json!({"filter": one_of("symbo", &FOREST), "paint": {"fill-color": p.forest}}),
        ));
        v.push(ign(
            "ign-forest-open",
            "fill",
            "ocs_vegetation_surf",
            json!({"filter": one_of("symbo", &["ZONE_FORET_OUVERTE", "ZONE_LANDE_LIGNEUSE"]),
                   "paint": {"fill-color": p.forest_open}}),
        ));
        v.push(ign(
            "ign-rock",
            "fill",
            "ocs_nature_sol_surf",
            json!({"filter": one_of("symbo", &["ZONE_ROCHEUSE", "ZONE_EBOULIS", "ZONE_RAVINEE"]),
                   "paint": {"fill-color": p.rock}}),
        ));
    } else {
        let mut all = FOREST.to_vec();
        all.push("ZONE_FORET_OUVERTE");
        v.push(ign(
            "ign-forest",
            "fill",
            "ocs_vegetation_surf",
            json!({"filter": one_of("symbo", &all), "paint": {"fill-color": p.forest_faint}}),
        ));
    }
    v.push(ign(
        "ign-glacier",
        "fill",
        "ocs_nature_sol_surf",
        json!({"filter": eq("symbo", json!("ZONE_GLACIER")), "paint": {"fill-color": p.glacier}}),
    ));
    v
}

fn water(p: &Palette, streams_from: f64) -> Vec<Value> {
    vec![
        ign(
            "ign-water",
            "fill",
            "hydro_surf",
            json!({"filter": one_of("symbo", &["SURFACE_D_EAU", "BASSIN", "SURFACE_D_EAU_TEMP"]),
                   "paint": {"fill-color": p.water}}),
        ),
        ign(
            "ign-stream",
            "line",
            "hydro_reseau",
            json!({"minzoom": streams_from,
                   "filter": one_of("symbo", &["COURS_D_EAU", "COURS_D_EAU_LAR", "COURS_D_EAU_MOY", "CANAL"]),
                   "layout": {"line-cap": "round", "line-join": "round"},
                   "paint": {"line-color": p.stream,
                             "line-width": zl(&[(11.0, 0.5), (14.0, 1.0), (16.0, 2.0)])}}),
        ),
    ]
}

/// Contour lines. `marked`: SRN (every contour from z12, index contours bold, labelled).
fn contours(p: &Palette, marked: bool) -> Vec<Value> {
    let index = [
        "CNV_MAITRESSE",
        "CNV_GLACIER_MAITRESSE",
        "CNV_ROCHER_MAITRESSE",
    ];
    let normal = [
        "CNV_NORMALE",
        "CNV_GLACIER_NORMALE",
        "CNV_ROCHER_NORMALE",
        "CUVETTE_NORMALE",
    ];
    if marked {
        vec![
            ign(
                "ign-contour",
                "line",
                "oro_courbe",
                json!({"minzoom": 12, "filter": one_of("symbo", &normal),
                       "paint": {"line-color": p.contour,
                                 "line-width": zl(&[(12.0, 0.4), (14.0, 0.8), (16.0, 1.1)])}}),
            ),
            ign(
                "ign-contour-index",
                "line",
                "oro_courbe",
                json!({"minzoom": 10, "filter": one_of("symbo", &index),
                       "paint": {"line-color": p.contour_index,
                                 "line-width": zl(&[(10.0, 0.6), (13.0, 1.2), (16.0, 1.8)])}}),
            ),
        ]
    } else {
        vec![
            ign(
                "ign-contour",
                "line",
                "oro_courbe",
                json!({"minzoom": 14, "filter": one_of("symbo", &normal),
                       "paint": {"line-color": p.contour_faint, "line-width": 0.5}}),
            ),
            ign(
                "ign-contour-index",
                "line",
                "oro_courbe",
                json!({"minzoom": 12, "filter": one_of("symbo", &index),
                       "paint": {"line-color": p.contour_faint,
                                 "line-width": zl(&[(12.0, 0.5), (15.0, 1.0)])}}),
            ),
        ]
    }
}

fn contour_labels(p: &Palette) -> Value {
    ign(
        "ign-contour-label",
        "symbol",
        "oro_courbe",
        json!({"minzoom": 13,
               "filter": ["all", one_of("symbo", &["CNV_MAITRESSE", "CNV_GLACIER_MAITRESSE", "CNV_ROCHER_MAITRESSE"]),
                          ["has", "texte"]],
               "layout": {"symbol-placement": "line", "text-field": ["get", "texte"],
                          "text-font": [SEMIBOLD], "text-size": zl(&[(13.0, 11.0), (16.0, 13.0)]),
                          "symbol-spacing": 400, "text-max-angle": 25, "text-padding": 10},
               "paint": text_paint(p.contour_index, p.halo, 2.0)}),
    )
}

const ROAD_MAJOR: [&str; 9] = [
    "AUTOROU_LIBRE",
    "AUTOROU_PEAGE",
    "PRINCIPALE_1",
    "PRINCIPALE_2",
    "PRINCIPALE_3",
    "PRINCIPALE_4",
    "REGIONALE_1",
    "REGIONALE_2",
    "REGIONALE_4",
];
const ROAD_MINOR: [&str; 5] = [
    "LOCALE_1",
    "LOCALE_2",
    "LOCALE_3",
    "NON_CLASSEE",
    "NON_CLASSEE_RESTREINT",
];
const ROAD_TRACK: [&str; 2] = ["NON_REVETUE_CARRO", "NON_REVETUE_CARRO_RESTREINT"];

/// Roads (surface and bridges). `tracks`: also unclassified roads and dirt tracks.
fn roads(p: &Palette, tracks: bool, minor_from: f64) -> Vec<Value> {
    let mut v = Vec::new();
    for (suffix, src) in [("", "routier_route"), ("-bridge", "routier_route_sup")] {
        // Bridges carry the class with `_SUP`; the IGN has no bridge of the smallest local
        // roads nor of REGIONALE_4.
        let sup = |list: &[&str]| -> Vec<String> {
            list.iter()
                .filter(|s| suffix.is_empty() || !matches!(**s, "LOCALE_3" | "REGIONALE_4"))
                .map(|s| {
                    if suffix.is_empty() {
                        (*s).to_string()
                    } else {
                        format!("{s}_SUP")
                    }
                })
                .collect()
        };
        if tracks {
            v.push(ign(
                &format!("ign-road-track{suffix}"),
                "line",
                src,
                json!({"minzoom": 13, "filter": ["match", ["get", "symbo"], sup(&ROAD_TRACK), true, false],
                       "layout": {"line-cap": "round"},
                       "paint": {"line-color": p.road_minor, "line-width": zl(&[(13.0, 0.6), (16.0, 1.5)]),
                                 "line-dasharray": [3, 2]}}),
            ));
        }
        v.push(ign(
            &format!("ign-road-minor{suffix}"),
            "line",
            src,
            json!({"minzoom": minor_from, "filter": ["match", ["get", "symbo"], sup(&ROAD_MINOR), true, false],
                   "layout": {"line-cap": "round", "line-join": "round"},
                   "paint": {"line-color": p.road_minor, "line-width": zl(&[(11.0, 0.6), (14.0, 1.5), (16.0, 3.0)])}}),
        ));
        v.push(ign(
            &format!("ign-road-major{suffix}"),
            "line",
            src,
            json!({"filter": ["match", ["get", "symbo"], sup(&ROAD_MAJOR), true, false],
                   "layout": {"line-cap": "round", "line-join": "round"},
                   "paint": {"line-color": p.road_major, "line-width": zl(&[(9.0, 0.8), (13.0, 1.8), (16.0, 4.0)])}}),
        ));
    }
    v
}

fn buildings(p: &Palette) -> Value {
    ign(
        "ign-building",
        "fill",
        "bati_surf",
        json!({"minzoom": 14, "paint": {"fill-color": p.building}}),
    )
}

fn lifts(p: &Palette) -> Value {
    ign(
        "ign-lift",
        "line",
        "bati_lin",
        json!({"minzoom": 12, "filter": eq("symbo", json!("REMONTEE_MEC")),
               "paint": {"line-color": p.lift, "line-width": zl(&[(12.0, 0.8), (16.0, 1.6)])}}),
    )
}

/// Parkings and nordic centres (ticket office, foyer) from OSM.
fn access_points(p: &Palette, centres: bool) -> Vec<Value> {
    // Public parkings: Protomaps gives private and tiny ones a `min_zoom` of 17.
    let parking = json!([
        "all",
        eq("kind", json!("parking")),
        ["<=", ["get", "min_zoom"], 16]
    ]);
    let mut v = vec![
        osm(
            "osm-parking",
            "circle",
            "pois",
            json!({"minzoom": 13, "filter": parking.clone(),
                   "paint": {"circle-color": p.parking, "circle-radius": zl(&[(13.0, 7.0), (16.0, 10.0)]),
                             "circle-stroke-color": p.halo, "circle-stroke-width": 1.5}}),
        ),
        contrast(
            osm(
                "osm-parking-p",
                "symbol",
                "pois",
                json!({"minzoom": 13, "filter": parking,
                   "layout": {"text-field": "P", "text-font": [BOLD], "text-size": zl(&[(13.0, 11.0), (16.0, 15.0)]),
                              "text-allow-overlap": true, "text-ignore-placement": true},
                   "paint": {"text-color": "#ffffff"}}),
            ),
            "osm-parking",
        ),
    ];
    if centres {
        let kinds = ["ticket", "winter_sports", "ski_rental", "ski"];
        v.push(osm(
            "osm-centre",
            "circle",
            "pois",
            json!({"minzoom": 12, "filter": ["all", one_of("kind", &kinds), ["has", "name"]],
                   "paint": {"circle-color": p.centre, "circle-radius": zl(&[(12.0, 5.0), (16.0, 8.0)]),
                             "circle-stroke-color": p.halo, "circle-stroke-width": 2}}),
        ));
        v.push(osm(
            "osm-centre-label",
            "symbol",
            "pois",
            json!({"minzoom": 14, "filter": ["all", one_of("kind", &kinds), ["has", "name"]],
                   "layout": {"text-field": ["get", "name"], "text-font": [SEMIBOLD], "text-size": 13,
                              "text-anchor": "top", "text-offset": [0, 0.9], "text-max-width": 9,
                              "text-optional": true},
                   "paint": text_paint(p.centre, p.halo, 2.0)}),
        ));
    }
    v
}

/// Localities. `hamlets_from`: zoom of hamlets and inhabited lieux-dits (typo A6-A7);
/// `small_from`: smaller ones (A8-A9), `None` to hide.
fn localities(p: &Palette, size: f64, hamlets_from: f64, small_from: Option<f64>) -> Vec<Value> {
    let communes_low = [
        "BAT_CHEF_LIEU_COM",
        "BAT_CHEF_LIEU_COM_T",
        "BAT_COMMUNE_3",
        "BAT_COMMUNE_3_T",
        "BAT_COMMUNE_4",
        "BAT_COMMUNE_4_T",
        "BAT_COMMUNE_5",
        "BAT_COMMUNE_5_T",
        "BAT_COMMUNE_ASSOCIEE",
    ];
    let label = |id: &str, filter: Value, minzoom: f64, maxzoom: f64, font: &str, s: f64| {
        ign(
            id,
            "symbol",
            "toponyme_localite_ponc",
            json!({"minzoom": minzoom, "maxzoom": maxzoom, "filter": filter,
                   "layout": {"text-field": ["get", "texte"], "text-font": [font], "text-size": s,
                              "text-max-width": 8, "text-padding": 6},
                   "paint": text_paint(p.text, p.halo, 2.5)}),
        )
    };
    let mut v = vec![
        label(
            "ign-place-commune-low",
            one_of("txt_typo", &communes_low),
            10.0,
            13.0,
            BOLD,
            size,
        ),
        label(
            "ign-place-commune",
            json!([
                "all",
                one_of("symbo", &["COMMUNE_CHEF_LIEU", "COMMUNE_FUSIONNEE"]),
                one_of(
                    "txt_typo",
                    &[
                        "TYPO_A_1", "TYPO_A_2", "TYPO_A_3", "TYPO_A_4", "TYPO_A_5", "TYPO_A_6"
                    ]
                )
            ]),
            13.0,
            24.0,
            BOLD,
            size + 1.0,
        ),
        label(
            "ign-place-hamlet",
            json!([
                "all",
                eq("symbo", json!("LIEU-DIT-HABITE")),
                one_of("txt_typo", &["TYPO_A_5", "TYPO_A_6", "TYPO_A_7"])
            ]),
            hamlets_from,
            24.0,
            SEMIBOLD,
            size - 2.0,
        ),
    ];
    if let Some(z) = small_from {
        v.push(label(
            "ign-place-small",
            json!([
                "all",
                eq("symbo", json!("LIEU-DIT-HABITE")),
                one_of("txt_typo", &["TYPO_A_8", "TYPO_A_9"])
            ]),
            z,
            24.0,
            REGULAR,
            size - 3.0,
        ));
    }
    v
}

/// Summits, passes, ridges and combes. The IGN types them by zoom band: `ORO_SOMMET_n`,
/// `ORO_COL_n` up to z11, `ORO_RELIEF_n` at z12, `TYPO_G_n` from z13 (4 = most important).
/// `all`: every one (SRN), else the main ones only.
///
/// One layer per band, shown only at the zooms of its band: a map that still shows the
/// tile of the zoom below or above while the new one loads (MapLibre Native keeps them
/// on screen) would otherwise draw a summit twice, once per band (« PIC DE ROCHEBRUNE »
/// of z12 next to « Pic de Rochebrune » of z13).
fn oronyms(p: &Palette, all: bool) -> Vec<Value> {
    let (main, more): (Vec<&str>, Vec<&str>) = if all {
        (
            vec![
                "ORO_SOMMET_1",
                "ORO_SOMMET_2",
                "ORO_COL_1",
                "ORO_RELIEF_2_T",
                "ORO_RELIEF_3_T",
                "TYPO_G_4",
                "TYPO_G_5",
                "TYPO_G_6",
            ],
            vec![
                "ORO_SOMMET_3",
                "ORO_COL_2",
                "ORO_RELIEF_3",
                "ORO_RELIEF_4",
                "ORO_RELIEF_4_T",
                "TYPO_G_7",
            ],
        )
    } else {
        (
            vec![
                "ORO_SOMMET_1",
                "ORO_SOMMET_2",
                "ORO_RELIEF_2_T",
                "TYPO_G_4",
                "TYPO_G_5",
            ],
            vec![],
        )
    };
    let label = |id: &str, list: &[&str], minzoom: f64, maxzoom: Option<f64>, s: f64| {
        let mut l = ign(
            id,
            "symbol",
            "toponyme_oro_ponc",
            json!({"minzoom": minzoom, "filter": one_of("txt_typo", list),
                   "layout": {"text-field": ["get", "texte"], "text-font": [ITALIC], "text-size": s,
                              "text-max-width": 8, "text-padding": 4},
                   "paint": text_paint(p.text_soft, p.halo, 2.0)}),
        );
        if let Some(z) = maxzoom {
            l["maxzoom"] = json!(z);
        }
        l
    };
    // (prefix of the IGN class, id suffix, minzoom, maxzoom) of each band.
    let bands: [(&[&str], &str, f64, Option<f64>); 3] = [
        (&["ORO_SOMMET", "ORO_COL"], "-z11", 11.0, Some(12.0)),
        (&["ORO_RELIEF"], "-z12", 12.0, Some(13.0)),
        (&["TYPO_G"], "", 13.0, None),
    ];
    let mut v = Vec::new();
    for (base, list, size) in [("ign-oro-main", &main, 13.5), ("ign-oro", &more, 12.0)] {
        for (prefixes, suffix, z0, z1) in bands {
            let band: Vec<&str> = list
                .iter()
                .copied()
                .filter(|c| prefixes.iter().any(|p| c.starts_with(p)))
                .collect();
            if !band.is_empty() {
                v.push(label(&format!("{base}{suffix}"), &band, z0, z1, size));
            }
        }
    }
    if !more.is_empty() {
        v.push(label("ign-oro-minor", &["TYPO_G_8"], 15.0, None, 11.5));
    }
    v
}

// ---------------------------------------------------------------------------------------
// Our layers.

/// Pistes. `primary`: fond (wide, labelled, arrows); else SRN (narrow, second plane).
fn pistes(p: &Palette, primary: bool) -> Vec<Value> {
    let (w_casing, w_line) = if primary {
        (
            zl(&[(10.0, 4.0), (12.0, 6.0), (14.0, 9.0), (16.0, 13.0)]),
            zl(&[(10.0, 2.5), (12.0, 3.5), (14.0, 6.0), (16.0, 9.0)]),
        )
    } else {
        (
            zl(&[(10.0, 2.0), (13.0, 3.0), (16.0, 6.0)]),
            zl(&[(10.0, 1.0), (13.0, 1.6), (16.0, 3.5)]),
        )
    };
    let groomed = json!(["all", groomed_json(), known_state()]);
    let unknown = json!(["all", groomed_json(), ["!", known_state()]]);
    let w_core = if primary {
        zl(&[(10.0, 1.2), (12.0, 1.6), (14.0, 2.6), (16.0, 3.8)])
    } else {
        zl(&[(10.0, 0.6), (13.0, 0.9), (16.0, 1.6)])
    };
    let backcountry = json!(["all", nordic(), ["==", ["get", "grooming"], "backcountry"]]);
    let mut v = vec![
        layer(
            "pistes-hike",
            "line",
            Some("pistes"),
            json!({"minzoom": if primary { 13 } else { 12 }, "filter": hike_only(),
                   "layout": {"line-cap": "round"},
                   "paint": {"line-color": p.hike, "line-width": zl(&[(12.0, 1.0), (16.0, 2.5)]),
                             "line-dasharray": [1, 2]}}),
        ),
        layer(
            "pistes-casing",
            "line",
            Some("pistes"),
            json!({"filter": ["all", nordic(), ["any", known_state(), ["==", ["get", "grooming"], "backcountry"]]],
                   "layout": {"line-cap": "round", "line-join": "round"},
                   "paint": {"line-color": piste_casing(p), "line-width": w_casing.clone()}}),
        ),
        // State unknown: a neutral band with a thin core of the difficulty colour, never
        // the full colour of an open piste.
        layer(
            "pistes-unknown-casing",
            "line",
            Some("pistes"),
            json!({"filter": unknown.clone(), "layout": {"line-cap": "round", "line-join": "round"},
                   "paint": {"line-color": ["match", ["coalesce", ["get", "difficulty"], ""], "advanced", p.unknown_black, p.unknown],
                             "line-width": w_casing}}),
        ),
        contrast(
            layer(
                "pistes-unknown",
                "line",
                Some("pistes"),
                json!({"filter": unknown, "layout": {"line-cap": "round", "line-join": "round"},
                       "paint": {"line-color": piste_color(p), "line-width": w_core}}),
            ),
            "pistes-unknown-casing",
        ),
        contrast(
            layer(
                "pistes-line",
                "line",
                Some("pistes"),
                json!({"filter": groomed, "layout": {"line-cap": "round", "line-join": "round"},
                       "paint": {"line-color": piste_color(p), "line-width": w_line}}),
            ),
            "pistes-casing",
        ),
        contrast(
            layer(
                "pistes-backcountry",
                "line",
                Some("pistes"),
                json!({"filter": backcountry, "layout": {"line-cap": "butt"},
                       "paint": {"line-color": piste_color(p), "line-width": w_line, "line-dasharray": [2, 1.5]}}),
            ),
            "pistes-casing",
        ),
    ];
    if primary {
        // Classic only: a light centre line, the two tracks of the classic style.
        v.push(layer(
            "pistes-classic-only",
            "line",
            Some("pistes"),
            json!({"minzoom": 13, "filter": ["all", groomed_json(), eq("classic", json!(true)), eq("skating", json!(false))],
                   "paint": {"line-color": p.halo, "line-width": zl(&[(13.0, 0.8), (16.0, 2.0)]),
                             "line-dasharray": [3, 2]}}),
        ));
    }
    v
}

fn groomed_json() -> Value {
    json!(["all", nordic(), ["!=", ["get", "grooming"], "backcountry"]])
}

/// The state of the piste is known (`open` or `closed`); a piste without the field (pack
/// without trail status) is unknown.
fn known_state() -> Value {
    one_of_coalesced("state", &["open", "closed"])
}

/// `["match", ["coalesce", ["get", key], ""], [values], true, false]`.
fn one_of_coalesced(key: &str, values: &[&str]) -> Value {
    json!(["match", ["coalesce", ["get", key], ""], values, true, false])
}

/// Closed pistes (field `state` of the pistes layer, joined way by way from the trail
/// status): the colour is softened by a veil of the ground colour and broken by dashes of
/// the ground colour, on the exact geometry of the piste; open pistes stay vivid and
/// solid.
fn status_closed(p: &Palette, primary: bool) -> Vec<Value> {
    let (veil, dash) = if primary {
        (
            zl(&[(10.0, 3.0), (12.0, 4.0), (14.0, 7.0), (16.0, 10.0)]),
            zl(&[(10.0, 1.0), (12.0, 1.5), (14.0, 2.5), (16.0, 3.5)]),
        )
    } else {
        (
            zl(&[(10.0, 1.5), (13.0, 2.2), (16.0, 4.5)]),
            zl(&[(10.0, 0.6), (13.0, 0.9), (16.0, 1.6)]),
        )
    };
    let closed = json!(["all", nordic(), eq("state", json!("closed"))]);
    vec![
        layer(
            "pistes-closed",
            "line",
            Some("pistes"),
            json!({"filter": closed.clone(), "layout": {"line-cap": "round", "line-join": "round"},
                   "paint": {"line-color": p.closed, "line-width": veil, "line-opacity": 0.4}}),
        ),
        layer(
            "pistes-closed-dash",
            "line",
            Some("pistes"),
            json!({"filter": closed, "layout": {"line-join": "round"},
                   "paint": {"line-color": p.closed, "line-width": dash, "line-dasharray": [2, 2]}}),
        ),
    ]
}

/// Oneway arrows on the pistes, along the geometry.
fn oneway_arrows(p: &Palette, theme: Theme) -> Value {
    // White chevrons with a dark halo: visible on every piste colour, open or closed.
    let halo = match theme {
        Theme::Clair => p.text,
        Theme::Sombre => p.halo,
    };
    layer(
        "pistes-oneway",
        "symbol",
        Some("pistes"),
        json!({"minzoom": 14, "filter": ["all", groomed_json(), eq("oneway", json!(true))],
               "layout": {"symbol-placement": "line", "symbol-spacing": 110, "text-field": "›",
                          "text-font": [BOLD], "text-size": zl(&[(14.0, 20.0), (16.0, 28.0)]),
                          "text-keep-upright": false, "text-rotation-alignment": "map",
                          "text-allow-overlap": true, "text-ignore-placement": true},
               "paint": text_paint("#ffffff", halo, 1.5)}),
    )
}

/// Piste names along the lines merged by itinerary (`piste-labels` of the pack): the
/// OSM ways are too short for most names.
fn piste_labels(p: &Palette, minzoom: f64, size: f64) -> Value {
    layer(
        "pistes-label",
        "symbol",
        Some("piste-labels"),
        json!({"minzoom": minzoom, "filter": ["has", "name"],
               "layout": {"symbol-placement": "line", "text-field": ["get", "name"],
                          "text-font": [SEMIBOLD], "text-size": size, "symbol-spacing": 300,
                          "text-max-angle": 35, "text-padding": 6},
               "paint": text_paint(p.text, p.halo, 2.5)}),
    )
}

/// Avalanche exposure along the pistes (vector segments): a wide striped band under the
/// piste, distinct from the plain colours of difficulty. `min`: lowest class drawn (2
/// exposed, 3 very exposed).
fn exposure_segments(p: &Palette, min: u8, primary: bool) -> Value {
    let w = if primary {
        zl(&[(10.0, 6.0), (12.0, 9.0), (14.0, 14.0), (16.0, 20.0)])
    } else {
        zl(&[(10.0, 4.0), (13.0, 7.0), (16.0, 12.0)])
    };
    contrast(
        layer(
            "avalanche-segments",
            "line",
            Some("avalanche-segments"),
            json!({"minzoom": 11, "filter": [">=", ["coalesce", ["get", "exposure"], 0], min],
                   "layout": {"line-cap": "butt", "line-join": "round"},
                   "paint": {"line-color": ["match", ["get", "exposure"], 3, p.exposure[1], p.exposure[0]],
                             "line-width": w, "line-dasharray": [0.45, 0.35]}}),
        ),
        "background",
    )
}

/// Huts: refuges.info (all huts), OSM and camptocamp huts not already in refuges.info.
/// Small dots up to `icons_from` (a domain view stays light), then the IGN hut icons,
/// always drawn (dark theme: on a light disc, the icons are drawn for a light ground);
/// names from `labels_from`, dropped when they collide.
fn huts(
    p: &Palette,
    theme: Theme,
    dots_from: f64,
    icons_from: f64,
    labels_from: f64,
    guarded_only: bool,
) -> Vec<Value> {
    let info_types: &[&str] = if guarded_only {
        &["refuge gardé", "gîte d'étape"]
    } else {
        &["refuge gardé", "gîte d'étape", "cabane non gardée"]
    };
    let open = json!(["!", one_of("state", &["Détruite", "Inutilisable"])]);
    // (source, filter, icon expression, name property)
    let mut sources = vec![(
        "refuges-info",
        json!(["all", one_of("type_label", info_types), open]),
        json!([
            "match",
            ["get", "type_label"],
            "refuge gardé",
            "Refugegard",
            "gîte d'étape",
            "Refugegard",
            "Refugenongard"
        ]),
        Some("name"),
    )];
    if !guarded_only {
        sources.push((
            "refuges-osm",
            json!([
                "all",
                ["!", ["has", "refuges_info_id"]],
                one_of("kind", &["alpine_hut", "wilderness_hut", "shelter"])
            ]),
            json!([
                "match",
                ["get", "kind"],
                "alpine_hut",
                "Refugegard",
                "wilderness_hut",
                "Refugenongard",
                "Abri"
            ]),
            None,
        ));
        sources.push((
            "refuges-camptocamp",
            json!(["!", ["has", "link"]]),
            json!([
                "match",
                ["get", "waypoint_type"],
                "shelter",
                "Abri",
                "hut",
                "Refugenongard",
                "Refugegard"
            ]),
            Some("title"),
        ));
    }
    let short = |src: &str| src.trim_start_matches("refuges-").to_string();
    let mut v = Vec::new();
    for (src, filter, _, _) in &sources {
        v.push(contrast(
            layer(
                &format!("huts-dot-{}", short(src)),
                "circle",
                Some(src),
                json!({"minzoom": dots_from, "maxzoom": icons_from, "filter": filter,
                       "paint": {"circle-color": p.hut, "circle-radius": zl(&[(10.0, 3.5), (13.0, 5.0)]),
                                 "circle-stroke-color": p.halo, "circle-stroke-width": 1.5}}),
            ),
            "background",
        ));
    }
    if theme == Theme::Sombre {
        for (src, filter, _, _) in &sources {
            v.push(layer(
                &format!("huts-disc-{}", short(src)),
                "circle",
                Some(src),
                json!({"minzoom": icons_from, "filter": filter,
                       "paint": {"circle-color": p.hut_disc, "circle-radius": zl(&[(13.0, 11.0), (16.0, 15.0)])}}),
            ));
        }
    }
    for (src, filter, icon, name) in sources {
        let mut layout = json!({"icon-image": icon, "icon-size": zl(&[(13.0, 0.7), (16.0, 1.0)]),
                                "icon-allow-overlap": true});
        if let Some(name) = name {
            layout["text-field"] = json!(["coalesce", ["get", name], ""]);
            layout["text-font"] = json!([SEMIBOLD]);
            layout["text-size"] = json!(["step", ["zoom"], 0, labels_from, 12.5]);
            layout["text-anchor"] = json!("top");
            layout["text-offset"] = json!([0, 1.2]);
            layout["text-optional"] = json!(true);
            layout["text-max-width"] = json!(8);
        }
        let mut l = layer(
            &format!("huts-{}", short(src)),
            "symbol",
            Some(src),
            json!({"minzoom": icons_from, "filter": filter, "layout": layout}),
        );
        if name.is_some() {
            l["paint"] = text_paint(p.text, p.halo, 2.0);
        }
        v.push(l);
    }
    v
}

/// Water points and delicate passages (refuges.info).
fn hut_extras(p: &Palette) -> Vec<Value> {
    vec![
        layer(
            "water-point",
            "circle",
            Some("refuges-info"),
            json!({"minzoom": 14, "filter": eq("type_label", json!("point d'eau")),
                   "paint": {"circle-color": p.water_point, "circle-radius": 5,
                             "circle-stroke-color": p.halo, "circle-stroke-width": 1.5}}),
        ),
        layer(
            "hazard",
            "circle",
            Some("refuges-info"),
            json!({"minzoom": 12, "filter": eq("type_label", json!("passage délicat")),
                   "paint": {"circle-color": p.hazard, "circle-radius": zl(&[(12.0, 7.0), (16.0, 10.0)]),
                             "circle-stroke-color": p.halo, "circle-stroke-width": 2}}),
        ),
        contrast(
            layer(
                "hazard-mark",
                "symbol",
                Some("refuges-info"),
                json!({"minzoom": 12, "filter": eq("type_label", json!("passage délicat")),
                   "layout": {"text-field": "!", "text-font": [BOLD], "text-size": zl(&[(12.0, 12.0), (16.0, 16.0)]),
                              "text-allow-overlap": true, "text-ignore-placement": true},
                   "paint": {"text-color": "#ffffff"}}),
            ),
            "hazard",
        ),
    ]
}

/// Zones whose rules restrict skiing (field `access` derived from the rules by the pack:
/// access forbidden, marked routes only, to avoid for wildlife), the whole zone, plus
/// the other protected areas that matter in winter (reserves, national park, by kind);
/// zones where bivouac is forbidden (field `bivouac`). Jurisdictions (country,
/// département, canton, commune) carry rules but are not outlined.
fn zones(p: &Palette) -> Vec<Value> {
    let jurisdiction = one_of(
        "kind",
        &[
            ZoneKind::Country,
            ZoneKind::Department,
            ZoneKind::Canton,
            ZoneKind::Commune,
        ]
        .map(ZoneKind::as_str),
    );
    let winter = json!([
        "all",
        ["!", jurisdiction.clone()],
        [
            "any",
            one_of("access", &["forbidden", "routes_only", "avoid_recommended"]),
            one_of(
                "kind",
                &[
                    ZoneKind::ReserveNaturelleNationale,
                    ZoneKind::ReserveNaturelleRegionale,
                    ZoneKind::ReserveBiologique,
                    ZoneKind::ParcNational
                ]
                .map(ZoneKind::as_str)
            )
        ]
    ]);
    let bivouac = json!([
        "all",
        ["!", jurisdiction],
        eq("bivouac", json!("forbidden"))
    ]);
    vec![
        layer(
            "zones-fill",
            "fill",
            Some("zones"),
            json!({"filter": winter.clone(), "paint": {"fill-color": p.zone, "fill-opacity": 0.07}}),
        ),
        contrast(
            layer(
                "zones-line",
                "line",
                Some("zones"),
                json!({"filter": winter.clone(),
                       "paint": {"line-color": p.zone, "line-width": zl(&[(10.0, 1.2), (15.0, 2.5)])}}),
            ),
            "background",
        ),
        layer(
            "zones-bivouac",
            "line",
            Some("zones"),
            json!({"minzoom": 12, "filter": ["all", bivouac, ["!", winter.clone()]],
                   "paint": {"line-color": p.bivouac, "line-width": 1.5, "line-dasharray": [3, 2]}}),
        ),
        layer(
            "zones-label",
            "symbol",
            Some("zones"),
            json!({"minzoom": 13, "filter": winter,
                   "layout": {"symbol-placement": "line", "text-field": ["get", "name"], "text-font": [SEMIBOLD],
                              "text-size": 12, "symbol-spacing": 500, "text-max-angle": 30, "text-offset": [0, 0.8]},
                   "paint": text_paint(p.zone, p.halo, 2.0)}),
        ),
    ]
}

fn no_network(p: &Palette) -> Vec<Value> {
    vec![
        layer(
            "no-network-fill",
            "fill",
            Some("no-network"),
            json!({"paint": {"fill-color": p.no_network, "fill-opacity": 0.07}}),
        ),
        contrast(
            layer(
                "no-network-line",
                "line",
                Some("no-network"),
                json!({"minzoom": 12, "paint": {"line-color": p.no_network, "line-width": 1.2,
                                                "line-dasharray": [2, 2]}}),
            ),
            "background",
        ),
    ]
}

/// Historic avalanche paths (CLPA): outline only, the slope classes show through.
fn avalanche_areas(p: &Palette) -> Vec<Value> {
    vec![contrast(
        layer(
            "avalanche-areas-line",
            "line",
            Some("avalanche-areas"),
            json!({"minzoom": 13, "paint": {"line-color": p.avalanche_area,
                                                "line-width": zl(&[(13.0, 0.7), (16.0, 1.6)])}}),
        ),
        "background",
    )]
}

/// Class polygons of the pack (slope, avalanche exposure): one fill per class, in the
/// colours of the theme; `opacity` over the zooms.
fn class_fills(
    source: &str,
    prefix: &str,
    colours: &[(u8, &str)],
    opacity: Value,
    minzoom: Option<f64>,
    maxzoom: Option<f64>,
) -> Vec<Value> {
    colours
        .iter()
        .map(|(class, colour)| {
            let mut l = layer(
                &format!("{prefix}-{class}"),
                "fill",
                Some(source),
                json!({"filter": eq("class", json!(class)),
                       "paint": {"fill-color": colour, "fill-opacity": opacity.clone(), "fill-antialias": true}}),
            );
            l["source-layer"] = json!(source);
            if let Some(z) = minzoom {
                l["minzoom"] = json!(z);
            }
            if let Some(z) = maxzoom {
                l["maxzoom"] = json!(z);
            }
            l
        })
        .collect()
}

/// Estimated snow classes of a practice (`snow-groomed`, `snow-srn`: 0 insufficient, 1
/// borderline, 2 sufficient), vector polygons in the colours of the theme, hidden until
/// asked for (`metadata.skimap:optional_layers`).
fn snow(p: &Palette, source: &str) -> Vec<Value> {
    let mut v = class_fills(
        source,
        source,
        &[(0, p.snow[0]), (1, p.snow[1]), (2, p.snow[2])],
        json!(p.snow_opacity),
        None,
        None,
    );
    for l in &mut v {
        l["layout"] = json!({"visibility": "none"});
    }
    v
}

/// SRN play areas and itineraries outside the groomed domains (`srn-areas`,
/// `srn-routes`). SRN style: a faint tint and a dashed outline for the areas, the
/// itineraries of SRN or easy ski touring in a solid line of their own hue with their
/// names, snowshoe and other ski touring itineraries attached to the areas dotted. Fond:
/// the SRN itineraries only, thin, dashed, no names (`primary` false).
fn srn_layers(p: &Palette, primary: bool) -> Vec<Value> {
    let skiable = one_of("activity", &["srn", "easy_ski_touring"]);
    if !primary {
        return vec![layer(
            "srn-routes-line",
            "line",
            Some("srn-routes"),
            json!({"minzoom": 11, "filter": skiable,
                   "layout": {"line-cap": "butt", "line-join": "round"},
                   "paint": {"line-color": p.srn, "line-width": zl(&[(11.0, 1.0), (16.0, 2.2)]),
                             "line-dasharray": [3, 2], "line-opacity": 0.8}}),
        )];
    }
    vec![
        layer(
            "srn-areas-fill",
            "fill",
            Some("srn-areas"),
            json!({"paint": {"fill-color": p.srn, "fill-opacity": zl(&[(9.0, 0.10), (13.0, 0.05)])}}),
        ),
        contrast(
            layer(
                "srn-areas-line",
                "line",
                Some("srn-areas"),
                json!({"paint": {"line-color": p.srn, "line-width": zl(&[(9.0, 0.8), (15.0, 1.6)]),
                                 "line-dasharray": [4, 3], "line-opacity": 0.7}}),
            ),
            "background",
        ),
        layer(
            "srn-routes-other",
            "line",
            Some("srn-routes"),
            json!({"minzoom": 12, "filter": ["!", skiable.clone()], "layout": {"line-cap": "round"},
                   "paint": {"line-color": p.srn, "line-width": zl(&[(12.0, 1.0), (16.0, 2.0)]),
                             "line-dasharray": [0.5, 2]}}),
        ),
        layer(
            "srn-routes-casing",
            "line",
            Some("srn-routes"),
            json!({"filter": skiable.clone(), "layout": {"line-cap": "round", "line-join": "round"},
                   "paint": {"line-color": p.halo, "line-width": zl(&[(9.0, 3.0), (13.0, 5.0), (16.0, 8.0)])}}),
        ),
        contrast(
            layer(
                "srn-routes-line",
                "line",
                Some("srn-routes"),
                json!({"filter": skiable, "layout": {"line-cap": "round", "line-join": "round"},
                       "paint": {"line-color": p.srn, "line-width": zl(&[(9.0, 1.6), (13.0, 2.8), (16.0, 4.5)])}}),
            ),
            "background",
        ),
    ]
}

/// Names of the SRN itineraries along their line (SRN style).
fn srn_labels(p: &Palette) -> Value {
    layer(
        "srn-routes-label",
        "symbol",
        Some("srn-routes"),
        json!({"minzoom": 12, "filter": ["all", one_of("activity", &["srn", "easy_ski_touring"]), ["has", "name"]],
               "layout": {"symbol-placement": "line", "text-field": ["get", "name"], "text-font": [SEMIBOLD],
                          "text-size": 12, "symbol-spacing": 400, "text-max-angle": 35, "text-padding": 6,
                          "text-offset": [0, 0.9]},
               "paint": text_paint(p.text, p.halo, 2.5)}),
    )
}

/// Trail-pass validity zones (`fee-zones`, schema of `android/docs/telechargement.md`):
/// the footprints of the domains where each pass is valid. Neutral thin dotted outline;
/// once the app knows the pass of the user (feature state `valid`), its zones get a
/// green tint and outline (« valable avec ton titre »), the zones of the other passes an
/// amber outline (« autre redevance »). Pass name along the outline from z13.
fn fee_zones(p: &Palette) -> Vec<Value> {
    let valid = json!(["==", ["feature-state", "valid"], true]);
    let other = json!(["==", ["feature-state", "valid"], false]);
    vec![
        layer(
            "fee-zones-fill",
            "fill",
            Some("fee-zones"),
            json!({"minzoom": 10,
                   "paint": {"fill-color": p.fee_valid,
                             "fill-opacity": ["case", valid.clone(), 0.08, 0.0]}}),
        ),
        layer(
            "fee-zones-line",
            "line",
            Some("fee-zones"),
            json!({"minzoom": 10,
                   "paint": {"line-color": ["case", valid.clone(), p.fee_valid, other, p.fee_other, p.fee],
                             "line-width": ["case", valid, 2.0, 1.0],
                             "line-dasharray": [1, 2.5]}}),
        ),
        layer(
            "fee-zones-label",
            "symbol",
            Some("fee-zones"),
            json!({"minzoom": 13, "filter": ["has", "name"],
                   "layout": {"symbol-placement": "line", "text-field": ["get", "name"], "text-font": [ITALIC],
                              "text-size": 11, "symbol-spacing": 600, "text-max-angle": 30, "text-offset": [0, 0.8]},
                   "paint": text_paint(p.text_soft, p.halo, 2.0)}),
        ),
    ]
}

/// Points of the stations (`stations-pois`, OSM via `skimap-stations`): rescue posts
/// (pisteurs, first aid, mountain rescue, defibrillators: red disc with a white cross)
/// from z12, sale points of the trail pass from z13.
fn station_points(p: &Palette) -> Vec<Value> {
    let rescue = eq("category", json!("secours"));
    let sale = eq("category", json!("vente"));
    vec![
        contrast(
            layer(
                "sale-point",
                "circle",
                Some("stations-pois"),
                json!({"minzoom": 13, "filter": sale.clone(),
                       "paint": {"circle-color": p.sale, "circle-radius": 6,
                                 "circle-stroke-color": p.halo, "circle-stroke-width": 1.5}}),
            ),
            "background",
        ),
        layer(
            "sale-point-mark",
            "symbol",
            Some("stations-pois"),
            json!({"minzoom": 14, "filter": sale,
                   "layout": {"text-field": "€", "text-font": [BOLD], "text-size": 10,
                              "text-allow-overlap": true, "text-ignore-placement": true},
                   "paint": text_paint("#ffffff", "#004d40", 1.5)}),
        ),
        contrast(
            layer(
                "secours-post",
                "circle",
                Some("stations-pois"),
                json!({"minzoom": 12, "filter": rescue.clone(),
                       "paint": {"circle-color": p.rescue, "circle-radius": 8,
                                 "circle-stroke-color": "#ffffff", "circle-stroke-width": 2}}),
            ),
            "background",
        ),
        layer(
            "secours-post-mark",
            "symbol",
            Some("stations-pois"),
            json!({"minzoom": 12, "filter": rescue,
                   "layout": {"text-field": "+", "text-font": [BOLD], "text-size": 15,
                              "text-allow-overlap": true, "text-ignore-placement": true},
                   "paint": text_paint("#ffffff", "#7f0000", 1.5)}),
        ),
    ]
}

/// Position of the skier (runtime source): accuracy disc and dot.
fn position(p: &Palette) -> Vec<Value> {
    // Pixels per metre at 45.7° N (both sites within 3 %), 512-pixel tiles.
    let k = 1.0 / (78_271.517 * 45.7_f64.to_radians().cos());
    let radius = json!([
        "interpolate",
        ["exponential", 2],
        ["zoom"],
        0,
        ["*", ["coalesce", ["get", "accuracy_m"], 0], k],
        22,
        ["*", ["coalesce", ["get", "accuracy_m"], 0], k * 4_194_304.0]
    ]);
    vec![
        layer(
            "position-accuracy",
            "circle",
            Some("position"),
            json!({"paint": {"circle-color": p.position, "circle-opacity": 0.15, "circle-radius": radius,
                             "circle-stroke-color": p.position, "circle-stroke-width": 1, "circle-stroke-opacity": 0.4}}),
        ),
        contrast(
            layer(
                "position-dot",
                "circle",
                Some("position"),
                json!({"paint": {"circle-color": p.position, "circle-radius": 9,
                                 "circle-stroke-color": "#ffffff", "circle-stroke-width": 3}}),
            ),
            "background",
        ),
    ]
}

// ---------------------------------------------------------------------------------------
// The three styles.

fn fond(theme: Theme) -> Vec<Value> {
    let p = theme.palette();
    let mut v = vec![background(p)];
    v.extend(ground_cover(p, false));
    v.extend(snow(p, "snow-groomed"));
    v.extend(water(p, 13.0));
    v.extend(contours(p, false));
    v.extend(roads(p, false, 11.0));
    v.push(buildings(p));
    v.push(lifts(p));
    v.extend(fee_zones(p));
    v.extend(srn_layers(p, false));
    v.push(exposure_segments(p, 3, true));
    v.extend(pistes(p, true));
    v.extend(status_closed(p, true));
    v.push(oneway_arrows(p, theme));
    v.extend(access_points(p, true));
    v.extend(station_points(p));
    v.extend(huts(p, theme, 11.0, 13.0, 14.0, true));
    v.push(piste_labels(p, 13.0, 14.0));
    v.extend(localities(p, 15.0, 13.0, Some(15.0)));
    v.extend(oronyms(p, false));
    v.extend(position(p));
    v
}

fn srn(theme: Theme) -> Vec<Value> {
    let p = theme.palette();
    let mut v = vec![background(p)];
    v.extend(ground_cover(p, true));
    v.extend(snow(p, "snow-srn"));
    // One reading at a time (X6, #140): the avalanche exposure at the scale of the
    // domain (z11 to 13), the slope classes from z13 (fade between 12.5 and 13.5, never
    // both at full strength); near steep ground the exposure stays on the pistes
    // (striped band) and the known avalanche paths keep their own outline.
    v.extend(class_fills(
        "avalanche-exposure",
        "exposure",
        &[(2, p.exposure_fill[0]), (3, p.exposure_fill[1])],
        zl(&[
            (11.0, 0.0),
            (11.5, p.exposure_opacity),
            (12.5, p.exposure_opacity),
            (13.0, 0.0),
        ]),
        Some(11.0),
        Some(13.0),
    ));
    v.extend(class_fills(
        "slope",
        "slope",
        &[
            (1, p.slope[0]),
            (2, p.slope[1]),
            (3, p.slope[2]),
            (4, p.slope[3]),
        ],
        zl(&[(13.0, 0.0), (13.5, p.slope_opacity)]),
        Some(13.0),
        None,
    ));
    v.extend(srn_layers(p, true).into_iter().take(2));
    v.extend(water(p, 12.0));
    v.push(ign(
        "ign-rock-line",
        "line",
        "ocs_lin",
        json!({"minzoom": 13, "filter": eq("symbo", json!("LIGNE_DESC_ZONE_ROCHEUSE")),
               "paint": {"line-color": p.rock_line, "line-width": 0.8}}),
    ));
    v.extend(contours(p, true));
    v.extend(roads(p, true, 11.0));
    v.push(ign(
        "ign-path",
        "line",
        "routier_chemin",
        json!({"minzoom": 13, "filter": one_of("symbo", &["CHEMIN", "SENTIER"]),
               "paint": {"line-color": p.path, "line-width": zl(&[(13.0, 0.8), (16.0, 1.8)]),
                         "line-dasharray": [3, 2]}}),
    ));
    v.push(buildings(p));
    v.push(lifts(p));
    v.extend(no_network(p));
    v.extend(zones(p));
    v.extend(avalanche_areas(p));
    v.push(exposure_segments(p, 2, false));
    v.extend(pistes(p, false));
    v.extend(status_closed(p, false));
    v.extend(srn_layers(p, true).into_iter().skip(2));
    v.extend(access_points(p, false));
    v.extend(station_points(p));
    v.extend(hut_extras(p));
    v.extend(huts(p, theme, 10.0, 13.0, 13.0, false));
    v.push(contour_labels(p));
    v.push(srn_labels(p));
    v.push(piste_labels(p, 14.0, 12.0));
    v.push(ign(
        "ign-spot-height",
        "symbol",
        "oro_ponc",
        json!({"minzoom": 14, "filter": ["all", one_of("symbo", &["POINT_COTE", "POINT_COTE_TOPO"]), ["has", "texte"]],
               "layout": {"text-field": ["get", "texte"], "text-font": [REGULAR], "text-size": 11, "text-padding": 6},
               "paint": text_paint(p.text_soft, p.halo, 2.0)}),
    ));
    v.push(ign(
        "ign-lieu-dit",
        "symbol",
        "toponyme_ocs_ponc",
        json!({"minzoom": 14, "filter": eq("symbo", json!("LIEU-DIT_NON_HABITE")),
               "layout": {"text-field": ["get", "texte"], "text-font": [ITALIC], "text-size": 11.5,
                          "text-max-width": 8, "text-padding": 6},
               "paint": text_paint(p.text_soft, p.halo, 2.0)}),
    ));
    v.extend(localities(p, 14.0, 12.0, Some(14.0)));
    v.extend(oronyms(p, true));
    v.extend(position(p));
    v
}

fn guidage(theme: Theme) -> Vec<Value> {
    let p = theme.palette();
    let mut v = vec![background(p)];
    v.extend(ground_cover(p, false));
    v.push(ign(
        "ign-water",
        "fill",
        "hydro_surf",
        json!({"filter": one_of("symbo", &["SURFACE_D_EAU", "BASSIN"]), "paint": {"fill-color": p.water}}),
    ));
    v.push(ign(
        "ign-road-major",
        "line",
        "routier_route",
        json!({"filter": one_of("symbo", &ROAD_MAJOR),
               "paint": {"line-color": p.road_minor, "line-width": zl(&[(10.0, 1.0), (16.0, 3.0)])}}),
    ));
    v.push(layer(
        "pistes-context",
        "line",
        Some("pistes"),
        json!({"filter": nordic(), "layout": {"line-cap": "round", "line-join": "round"},
               "paint": {"line-color": p.piste_context, "line-width": zl(&[(10.0, 1.0), (16.0, 3.0)])}}),
    ));
    v.push(layer(
        "route-done",
        "line",
        Some("route"),
        json!({"filter": eq("part", json!("done")), "layout": {"line-cap": "round", "line-join": "round"},
               "paint": {"line-color": p.route_done, "line-width": zl(&[(10.0, 4.0), (16.0, 9.0)])}}),
    ));
    v.push(layer(
        "route-casing",
        "line",
        Some("route"),
        json!({"filter": ["!=", ["get", "part"], "done"], "layout": {"line-cap": "round", "line-join": "round"},
               "paint": {"line-color": p.halo, "line-width": zl(&[(10.0, 9.0), (16.0, 18.0)])}}),
    ));
    v.push(contrast(
        layer(
            "route-line",
            "line",
            Some("route"),
            json!({"filter": ["!=", ["get", "part"], "done"], "layout": {"line-cap": "round", "line-join": "round"},
                   "paint": {"line-color": p.route, "line-width": zl(&[(10.0, 5.0), (16.0, 11.0)])}}),
        ),
        "background",
    ));
    v.extend(localities(p, 17.0, 14.0, None));
    // Yellow disc (contrast with dark ground), dark ring (contrast with snow).
    v.push(contrast(
        layer(
            "decision",
            "circle",
            Some("decision"),
            json!({"paint": {"circle-color": p.decision, "circle-radius": 15,
                             "circle-stroke-color": p.decision_line, "circle-stroke-width": 4}}),
        ),
        "background",
    ));
    v.push(layer(
        "decision-label",
        "symbol",
        Some("decision"),
        json!({"layout": {"text-field": ["get", "label"], "text-font": [BOLD], "text-size": 20,
                          "text-anchor": "left", "text-offset": [1.4, 0], "text-max-width": 12,
                          "text-allow-overlap": true},
               "paint": text_paint(p.text, p.halo, 3.0)}),
    ));
    v.extend(position(p));
    v
}

/// A group of layers the app and the viewer switch together (`metadata.skimap:group` of
/// each layer, `metadata.skimap:groups` of the style).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Group {
    /// Identifier.
    pub id: &'static str,
    /// Title, French (menu).
    pub title: &'static str,
    /// Whether the user may hide it. Safety layers (dangers, rules, route, position) and
    /// the ground of the map cannot be hidden.
    pub toggleable: bool,
}

/// Every group, in menu order.
pub const GROUPS: [Group; 16] = [
    Group {
        id: "fond",
        title: "Fond de carte (relief, forêt, eau, routes)",
        toggleable: false,
    },
    Group {
        id: "pistes",
        title: "Pistes et leur état",
        toggleable: false,
    },
    Group {
        id: "dangers",
        title: "Avalanches et passages délicats",
        toggleable: false,
    },
    Group {
        id: "reglementation",
        title: "Zones réglementées et bivouac",
        toggleable: false,
    },
    Group {
        id: "secours",
        title: "Secours (postes de secours, défibrillateurs)",
        toggleable: false,
    },
    Group {
        id: "itineraire",
        title: "Itinéraire et prochaine décision",
        toggleable: false,
    },
    Group {
        id: "position",
        title: "Position",
        toggleable: false,
    },
    Group {
        id: "pentes",
        title: "Pentes",
        toggleable: true,
    },
    Group {
        id: "refuges",
        title: "Refuges et abris",
        toggleable: true,
    },
    Group {
        id: "points-eau",
        title: "Points d'eau",
        toggleable: true,
    },
    Group {
        id: "acces",
        title: "Parkings et accueils",
        toggleable: true,
    },
    Group {
        id: "noms",
        title: "Noms de lieux, sommets, cotes",
        toggleable: true,
    },
    Group {
        id: "reseau",
        title: "Zones sans réseau",
        toggleable: true,
    },
    Group {
        id: "neige",
        title: "Neige estimée",
        toggleable: true,
    },
    Group {
        id: "srn",
        title: "Itinéraires et domaines SRN (hors stations)",
        toggleable: true,
    },
    Group {
        id: "redevance",
        title: "Zones de redevance",
        toggleable: true,
    },
];

/// Group of a layer, from its identifier.
pub fn group_of(id: &str) -> Option<&'static str> {
    let starts = |p: &[&str]| {
        p.iter()
            .any(|x| id == *x || id.starts_with(&format!("{x}-")))
    };
    Some(
        if starts(&[
            "ign-contour-label",
            "ign-spot-height",
            "ign-place",
            "ign-oro",
            "ign-lieu-dit",
        ]) {
            "noms"
        } else if id == "background" || starts(&["ign"]) {
            "fond"
        } else if starts(&["pistes"]) {
            "pistes"
        } else if starts(&["secours-post"]) {
            "secours"
        } else if starts(&["osm-parking", "osm-centre", "sale-point"]) {
            "acces"
        } else if starts(&["huts"]) {
            "refuges"
        } else if starts(&["water-point"]) {
            "points-eau"
        } else if starts(&[
            "hazard",
            "avalanche-areas",
            "avalanche-segments",
            "exposure",
        ]) {
            "dangers"
        } else if starts(&["slope"]) {
            "pentes"
        } else if starts(&["zones"]) {
            "reglementation"
        } else if starts(&["no-network"]) {
            "reseau"
        } else if starts(&["snow"]) {
            "neige"
        } else if starts(&["srn-areas", "srn-routes"]) {
            "srn"
        } else if starts(&["fee-zones"]) {
            "redevance"
        } else if starts(&["route", "decision"]) {
            "itineraire"
        } else if starts(&["position"]) {
            "position"
        } else {
            return None;
        },
    )
}

/// The style `kind` in `theme`.
pub fn style(kind: Kind, theme: Theme) -> Value {
    let mut layers = match kind {
        Kind::Fond => fond(theme),
        Kind::Srn => srn(theme),
        Kind::Guidage => guidage(theme),
    };
    for l in &mut layers {
        let id = l["id"].as_str().unwrap_or("").to_owned();
        let g = group_of(&id).unwrap_or_else(|| panic!("layer {id} has no group"));
        l["metadata"]["skimap:group"] = json!(g);
    }
    let present: std::collections::BTreeSet<&str> = layers
        .iter()
        .filter_map(|l| l["metadata"]["skimap:group"].as_str())
        .collect();
    let groups: Vec<Value> = GROUPS
        .iter()
        .filter(|g| present.contains(g.id))
        .map(|g| json!({"id": g.id, "title": g.title, "toggleable": g.toggleable}))
        .collect();
    let used: std::collections::BTreeSet<&str> =
        layers.iter().filter_map(|l| l["source"].as_str()).collect();
    let sources: Map<String, Value> = all_sources()
        .into_iter()
        .filter(|(id, _)| used.contains(id.as_str()))
        .collect();
    let runtime: Map<String, Value> = runtime_schema()
        .as_object()
        .expect("runtime schema")
        .iter()
        .filter(|(id, _)| used.contains(id.as_str()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let states: Map<String, Value> = feature_states()
        .as_object()
        .expect("feature states")
        .iter()
        .filter(|(id, _)| used.contains(id.as_str()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let optional: Vec<&str> = OPTIONAL_SOURCES
        .iter()
        .copied()
        .filter(|id| used.contains(id))
        .collect();
    let toggles: Vec<&str> = layers
        .iter()
        .filter(|l| l["layout"]["visibility"] == "none")
        .filter_map(|l| l["id"].as_str())
        .collect();
    json!({
        "version": 8,
        "name": format!("skimap {} ({})", kind.title(), theme.name()),
        "metadata": {
            "skimap:style": kind.name(),
            "skimap:theme": theme.name(),
            "skimap:generator": "crates/styles (cargo run -p skimap-styles --example write_styles)",
            "skimap:relative_urls": true,
            "skimap:runtime_sources": runtime,
            "skimap:optional_layers": toggles,
            "skimap:optional_sources": optional,
            "skimap:feature_states": states,
            "skimap:groups": groups
        },
        "glyphs": GLYPHS,
        "sprite": SPRITE,
        "sources": sources,
        "layers": layers
    })
}

/// Pretty JSON of a style with a final newline, as written in `styles/`.
pub fn style_text(kind: Kind, theme: Theme) -> String {
    let mut s = serde_json::to_string_pretty(&style(kind, theme)).expect("serialisable style");
    s.push('\n');
    s
}
