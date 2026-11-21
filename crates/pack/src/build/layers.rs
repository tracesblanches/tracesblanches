//! Vector (GeoJSON) and data (JSON) layers of the pack, taken from `data/out/<site>/`.
//!
//! GeoJSON is rewritten compact with coordinates rounded to 1e-6° (≈ 0.1 m); properties
//! are kept as written by the producing crate. The zones layer gains two derived
//! properties: `rules` (rules of `rules.json` applying to the zone) and `rules_text`.

use std::borrow::Cow;
use std::collections::BTreeSet;
use std::path::Path;

use serde_json::{Value, json};
use skimap_common::licence::{self, Family, Group};
use skimap_common::meta::LayerMeta;
use skimap_zones::{Effect, RuleSet, Topic, ZoneLayer};

use crate::build::{copy_data, write_bytes};
use crate::error::{Error, Result};
use crate::manifest::{Layer, LayerKind, Missing, Source};

/// How a layer file is copied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transform {
    /// Copy: JSON data rewritten compact ([`crate::build::copy_data`]), other files byte
    /// for byte.
    Copy,
    /// Gzip copy (binary rasters of the core, `*.skr` → `*.skr.gz`).
    Gzip,
    /// GeoJSON, compact, rounded coordinates.
    GeoJson,
    /// GeoJSON zones annotated with the rules of `rules.json` in the same directory.
    Zones,
    /// GeoJSON pistes enriched with the trail state and the piste plan
    /// ([`crate::build::pistes`]).
    Pistes,
    /// Estimated traces of the pistes known but absent from OSM (G1,
    /// [`estimated_pistes`]): the features of `pistes_estimees.geojson` (plan, Geotrek,
    /// IGN sections), all of them in a personal pack, in a published one only those whose
    /// every licence (`licences`) allows it.
    EstimatedPistes,
    /// GeoJSON, only the features of a licence group ([`skimap_common::licence::Group`],
    /// #169): each feature goes to the file of the group of its `licence` (an itinerary)
    /// or of its `licences` (an area made of several itineraries), [`feature_group`].
    Licence(Group),
}

/// A layer looked for in the pipeline outputs.
#[derive(Debug, Clone)]
pub struct Spec {
    /// Pack layer identifier.
    pub id: Cow<'static, str>,
    /// Display title, French.
    pub title: Cow<'static, str>,
    /// Pipeline layer directory (`data/out/<site>/<origin>/`).
    pub origin: &'static str,
    /// File in that directory.
    pub file: Cow<'static, str>,
    /// `<couche>.meta.json` in that directory.
    pub meta: Option<&'static str>,
    /// Kind.
    pub kind: LayerKind,
    /// Shown when the viewer opens.
    pub visible: bool,
    /// Copy mode.
    pub transform: Transform,
    /// Notes, French.
    pub notes: &'static [&'static str],
    /// Pipeline layers the file is derived from: when one of them comes from a
    /// personal-use source, so does this layer (off-piste rasters of the core from the
    /// avalanche exposure and the zones).
    pub depends: &'static [&'static str],
    /// Personal use only whatever its sources say, and why (L3 rule 1: the OSM huts
    /// completing refuges.info, docs/licences/l3-combinaison.md).
    pub personal: Option<&'static str>,
}

/// Result of looking for a layer.
#[derive(Debug)]
pub enum Outcome {
    /// Copied into the pack.
    Included(Box<Layer>),
    /// Not included.
    Missing(Missing),
}

const fn vector(
    id: &'static str,
    title: &'static str,
    origin: &'static str,
    file: &'static str,
    meta: &'static str,
    visible: bool,
) -> Spec {
    Spec {
        id: Cow::Borrowed(id),
        title: Cow::Borrowed(title),
        origin,
        file: Cow::Borrowed(file),
        meta: Some(meta),
        kind: LayerKind::Vector,
        visible,
        transform: Transform::GeoJson,
        notes: &[],
        depends: &[],
        personal: None,
    }
}

const fn data(
    id: &'static str,
    title: &'static str,
    origin: &'static str,
    file: &'static str,
    meta: &'static str,
) -> Spec {
    Spec {
        id: Cow::Borrowed(id),
        title: Cow::Borrowed(title),
        origin,
        file: Cow::Borrowed(file),
        meta: Some(meta),
        kind: LayerKind::Data,
        visible: false,
        transform: Transform::Copy,
        notes: &[],
        depends: &[],
        personal: None,
    }
}

/// Vector and data layers, in display order (drawn bottom to top by the viewer).
pub fn catalog() -> Vec<Spec> {
    let mut v = catalog_without_srn();
    let at = v
        .iter()
        .position(|s| s.id == "coverage-area")
        .map_or(v.len(), |i| i + 1);
    v.splice(at..at, srn_specs());
    v
}

/// Pack layer of the SRN areas (`srn-areas`) or itineraries (`srn-routes`) of a licence
/// group: `srn-routes`, `srn-routes-cc-by-sa`, `srn-routes-personnel` (#169).
pub fn srn_layer(base: &str, group: Group) -> String {
    format!("{base}{}", group.suffix())
}

/// SRN areas and itineraries outside groomed domains touching the site box (G5, written
/// by `skimap build-all` in `srn/`): the content of an SRN site, one layer per licence
/// group of their features (#169): ODbL (OSM, Licence Ouverte), CC BY-SA (Camptocamp)
/// apart with its attribution, personal (no reuse licence shown: Geotrek instances;
/// areas mixing ODbL and CC BY-SA itineraries) only with `personal_sources`.
fn srn_specs() -> Vec<Spec> {
    let mut v = Vec::new();
    for (base, title, file) in [
        (
            "srn-areas",
            "Domaines de jeu et traversées SRN (hors stations)",
            "srn_areas.geojson",
        ),
        (
            "srn-routes",
            "Itinéraires SRN (hors stations)",
            "srn_routes.geojson",
        ),
    ] {
        for group in Group::ALL {
            let title = match group {
                Group::Odbl => title.to_owned(),
                Group::ShareAlike => format!("{title}, CC BY-SA"),
                Group::Personal => format!("{title}, usage personnel"),
            };
            v.push(Spec {
                id: Cow::Owned(srn_layer(base, group)),
                title: Cow::Owned(title),
                file: Cow::Borrowed(file),
                transform: Transform::Licence(group),
                notes: &[
                    "Une couche par famille de licences de ses entités (#169, docs/licences/l3-combinaison.md) : ODbL et Licence Ouverte (`srn-*`), CC BY-SA à part avec son attribution (`srn-*-cc-by-sa`), sans licence de réutilisation connue ou licences incompatibles mêlées (`srn-*-personnel`, seulement avec personal_sources).",
                ],
                ..vector("", "", "srn", "", "srn.meta.json", true)
            });
        }
    }
    v
}

/// Licence group of a feature ([`Transform::Licence`]): of its `licences` (an SRN area:
/// the licences of its itineraries) or of its `licence`; without either, personal.
pub fn feature_group(props: &Value) -> Group {
    if let Some(ls) = props.get("licences").and_then(Value::as_array) {
        return licence::group(
            ls.iter()
                .map(|l| licence::classify(l.as_str().unwrap_or(""))),
        );
    }
    match props.get("licence").and_then(Value::as_str) {
        Some(l) => licence::group([licence::classify(l)]),
        None => Group::Personal,
    }
}

/// Why the OSM huts and the links between the hut sources are personal (L3 rule 1).
pub const L3_HUTS: &str = "abris OSM et liens entre sources de refuges : OSM (ODbL) et refuges.info, Camptocamp (CC BY-SA) se complètent pour un même type d'objet, ce qu'aucune licence ne permet dans un pack publié (règle 1 de docs/licences/l3-combinaison.md)";

fn catalog_without_srn() -> Vec<Spec> {
    vec![
        vector(
            "domaines",
            "Domaines nordiques",
            "domaines",
            "domaines.geojson",
            "domaines.meta.json",
            false,
        ),
        Spec {
            notes: &[
                "Propriétés dérivées par le pack : `rules` (règles de rules.json qui s'appliquent à la zone : id, sujet, effet, période, résumé, conditions, source) et `rules_text` (texte français).",
                "Champs simples dérivés des règles de la zone et de celles des territoires qui la contiennent (skimap_zones::fields) : `bivouac` (forbidden, forbidden_in_places, tolerated, consent_required, allowed, unknown), `access` (forbidden, forbidden_in_places, routes_only, routes_only_in_places, avoid_recommended, allowed, unknown), leurs périodes (`bivouac_period`, `access_period`, null = toute l'année) et les règles qui décident (`bivouac_rules`, `access_rules`). Acte propre de la zone (`local_act` : arrêté de biotope, décret de réserve, réglementation du cœur de parc, arrêtés municipaux…) et son statut (`local_rules` : read, partial, unverified = règles locales non vérifiées ; null sans acte propre) (skimap_zones::local, G4).",
            ],
            transform: Transform::Zones,
            ..vector(
                "zones",
                "Zones protégées et règles",
                "zones",
                "zones.geojson",
                "zones.meta.json",
                true,
            )
        },
        data(
            "zones-rules",
            "Règles de bivouac, feu et accès",
            "zones",
            "rules.json",
            "zones.meta.json",
        ),
        vector(
            "no-network",
            "Zones sans aucun réseau (même le 112)",
            "coverage",
            "no_network.geojson",
            "coverage.meta.json",
            true,
        ),
        vector(
            "coverage-area",
            "Zone où la couverture est connue (France)",
            "coverage",
            "analysed_area.geojson",
            "coverage.meta.json",
            false,
        ),
        vector(
            "avalanche-areas",
            "Avalanches historiques (emprises CLPA)",
            "avalanche-historic",
            "emprises.geojson",
            "avalanche-historic.meta.json",
            true,
        ),
        vector(
            "pistes-graph-edges",
            "Graphe de routage (arêtes)",
            "pistes",
            "graph_edges.geojson",
            "pistes.meta.json",
            false,
        ),
        Spec {
            notes: &[
                "Propriétés ajoutées par le pack, voie OSM par voie OSM : `state` (open, closed, unknown), `state_at` (date de l'état publiée par la source), `state_source`, `state_itineraries` (couche status) ; `colour_source`, `oneway_source`, `name_source` (osm ou plan), `plan_pistes`, `plan_colours`, `geometry_reversed` (plan des pistes P22). Sources à usage personnel : seulement avec personal_sources.",
            ],
            transform: Transform::Pistes,
            ..vector(
                "pistes",
                "Pistes nordiques",
                "pistes",
                "pistes.geojson",
                "pistes.meta.json",
                true,
            )
        },
        // Pistes known but absent from OSM (G1, #117): estimated traces, always uncertain
        // (dotted on the map), from the plan, Geotrek or the IGN sections. Each feature
        // carries its licences: a published pack keeps those whose every licence allows
        // it (none derived from the plan, whose source is personal use only).
        Spec {
            notes: &[
                "Pistes connues absentes d'OSM : tracé estimé (`certainty` = `incertain`, jamais présenté comme sûr). D'après le plan officiel : chemins IGN du couloir de la ligne du plan (`method` = `estimated_paths`) ou ligne du plan reportée (`estimated_drawing`) ; pistes du bulletin de l'exploitant ou du plan : itinéraire Geotrek du même nom et de longueur proche (`geotrek`) ou liaison routée sur les tronçons IGN entre les lieux qu'elle nomme (`estimated_route`). Propriétés : `name`, `colour`, `difficulty` (de la couleur), `kind`, `certainty`, `method`, `score`, `length_m`, `length_km_announced`, `lengths_km_plan`, `departures`, `named_by`, `source_name`, `source_url`, `route_ends`, `plan_url` (adresse du plan officiel, lien seulement), `licences`. Pack publié : seulement les entités dont toutes les licences le permettent.",
            ],
            transform: Transform::EstimatedPistes,
            ..vector(
                ESTIMATED_LAYER,
                "Pistes absentes d'OSM (tracé incertain)",
                "pistes_estimees",
                "pistes_estimees.geojson",
                "pistes_estimees.meta.json",
                true,
            )
        },
        vector(
            "routes",
            "Itinéraires de preuve (routage sur pistes)",
            "routes",
            "routes.geojson",
            "routes.meta.json",
            false,
        ),
        vector(
            "status",
            "État des pistes",
            "status",
            "status.geojson",
            "status.meta.json",
            false,
        ),
        Spec {
            notes: &[
                "Tronçons des pistes par classe d'exposition aux avalanches (« terrain peu exposé (modèle) », « exposé », « très exposé »), jamais « sûr ».",
            ],
            ..vector(
                "avalanche-segments",
                "Pistes par exposition aux avalanches",
                "avalanche-exposure",
                "segments.geojson",
                "avalanche-exposure.meta.json",
                true,
            )
        },
        vector(
            "avalanche-crossings",
            "Tronçons de pistes en zone d'avalanche connue",
            "avalanche-historic",
            "crossings.geojson",
            "avalanche-historic.meta.json",
            true,
        ),
        vector(
            "pistes-graph-nodes",
            "Graphe de routage (nœuds)",
            "pistes",
            "graph_nodes.geojson",
            "pistes.meta.json",
            false,
        ),
        vector(
            "refuges-camptocamp",
            "Refuges et abris (Camptocamp)",
            "refuges",
            "camptocamp.geojson",
            "refuges.meta.json",
            true,
        ),
        // L3 rule 1 (docs/licences/l3-combinaison.md): the OSM huts complete refuges.info
        // (the style draws those it lacks, the day plan keeps one of each pair): OSM and
        // CC BY-SA data for one feature type, which no licence allows in a published pack.
        Spec {
            personal: Some(L3_HUTS),
            ..vector(
                "refuges-osm",
                "Refuges et abris (OpenStreetMap)",
                "refuges",
                "osm.geojson",
                "refuges.meta.json",
                true,
            )
        },
        vector(
            "refuges-info",
            "Refuges et abris (refuges.info)",
            "refuges",
            "refuges_info.geojson",
            "refuges.meta.json",
            true,
        ),
        vector(
            "refuges",
            "Refuges et abris (fiches fusionnées)",
            "refuges",
            "refuges.geojson",
            "refuges.meta.json",
            false,
        ),
        Spec {
            personal: Some(L3_HUTS),
            ..data(
                "refuges-links",
                "Liens entre les trois sources de refuges",
                "refuges",
                "links.json",
                "refuges.meta.json",
            )
        },
        data(
            "avalanche-summary",
            "Avalanches historiques : synthèse",
            "avalanche-historic",
            "summary.json",
            "avalanche-historic.meta.json",
        ),
        data(
            "bera",
            "Bulletin d'estimation du risque d'avalanche",
            "bera",
            "bera.json",
            "bera.meta.json",
        ),
        data(
            "status-summary",
            "État des pistes : dernier instantané",
            "status",
            "status.json",
            "status.meta.json",
        ),
        data(
            "meteo-hourly",
            "Météo heure par heure",
            "meteo-hourly",
            "hourly.json",
            "meteo-hourly.meta.json",
        ),
        data(
            "piste-graph",
            "Graphe de pistes typé (noyau, routage)",
            "routes",
            "piste_graph.json",
            "routes.meta.json",
        ),
        core_raster(
            "offpiste-elevation",
            "Hors piste : altitude 5 m (noyau)",
            "elevation_5m.skr",
        ),
        core_raster(
            "offpiste-slope",
            "Hors piste : pente 5 m (noyau)",
            "slope_5m.skr",
        ),
        core_raster(
            "offpiste-zones",
            "Hors piste : classes de zones 5 m (noyau)",
            "zones_5m.skr",
        ),
        core_raster(
            "offpiste-pistes",
            "Hors piste : pistes et itinéraires 5 m (noyau)",
            "pistes_5m.skr",
        ),
        core_raster(
            "offpiste-exposure",
            "Hors piste : exposition aux avalanches 5 m (noyau, surcouche du routeur)",
            "exposure_5m.skr",
        ),
        Spec {
            transform: Transform::Copy,
            ..core_raster(
                "offpiste-zone-classes",
                "Hors piste : table des classes de zones",
                "zone_classes.tsv",
            )
        },
    ]
}

/// Layers the off-piste rasters of the core (and its proof routes) are computed from,
/// besides relief and pistes: the avalanche exposure (CLPA fused with `personal_sources`)
/// and the zones (Biodiv'sports with `personal_sources`).
pub const OFFPISTE_INPUTS: &[&str] = &["avalanche-exposure", "zones"];

/// A binary raster of the off-piste router (`skimap_core::offpiste`, format SKR1), stored
/// gzipped: the phone inflates it once.
const fn core_raster(id: &'static str, title: &'static str, file: &'static str) -> Spec {
    Spec {
        id: Cow::Borrowed(id),
        title: Cow::Borrowed(title),
        origin: "offpiste",
        file: Cow::Borrowed(file),
        meta: Some("offpiste.meta.json"),
        kind: LayerKind::Data,
        visible: false,
        transform: Transform::Gzip,
        notes: &[
            "Raster SKR1 de skimap-core (en-tête de 72 octets, voir offpiste.meta.json), compressé gzip dans le pack.",
        ],
        depends: OFFPISTE_INPUTS,
        personal: None,
    }
}

/// Layers whose files are found by name: the off-piste proof routes
/// (`routes/offpiste_*.geojson`) and the latest off-piste snow raster
/// (`offpiste/snow_<date>.skr`).
pub fn discovered(out: &Path) -> Result<Vec<Spec>> {
    let mut v = Vec::new();
    for name in discover(out, "routes", "offpiste_", ".geojson")? {
        let stem = name
            .trim_end_matches(".geojson")
            .trim_start_matches("offpiste_")
            .to_owned();
        v.push(Spec {
            id: Cow::Owned(format!("route-offpiste-{}", stem.replace('_', "-"))),
            title: Cow::Owned(format!(
                "Itinéraire hors piste de preuve : {}",
                stem.replace('_', " ")
            )),
            file: Cow::Owned(name),
            depends: OFFPISTE_INPUTS,
            ..vector("", "", "routes", "", "routes.meta.json", false)
        });
    }
    if let Some(name) = discover(out, "offpiste", "snow_", ".skr")?.pop() {
        v.push(Spec {
            file: Cow::Owned(name),
            ..core_raster(
                "offpiste-snow",
                "Hors piste : classes de neige 25 m, dernière date (noyau)",
                "",
            )
        });
    }
    // Assistant (A1, A2, A3, A5): the files of the latest briefing date.
    if let Some(date) = discover(out, "assist", "briefing_", ".json")?
        .pop()
        .map(|n| n["briefing_".len()..n.len() - ".json".len()].to_owned())
    {
        for (id, title, prefix, ext, kind) in [
            (
                "assist-briefing",
                "Assistant : briefing du jour",
                "briefing_",
                "json",
                LayerKind::Data,
            ),
            (
                "assist-wax",
                "Assistant : fartage",
                "wax_",
                "json",
                LayerKind::Data,
            ),
            (
                "assist-snow-quality",
                "Assistant : qualité de neige heure par heure",
                "snow_quality_",
                "json",
                LayerKind::Data,
            ),
            (
                "assist-suggestions",
                "Assistant : boucles suggérées",
                "suggestions_",
                "geojson",
                LayerKind::Vector,
            ),
        ] {
            let file = format!("{prefix}{date}.{ext}");
            if !out.join("assist").join(&file).is_file() {
                continue;
            }
            let base = if kind == LayerKind::Vector {
                vector("", "", "assist", "", "assist.meta.json", true)
            } else {
                data("", "", "assist", "", "assist.meta.json")
            };
            v.push(Spec {
                id: Cow::Borrowed(id),
                title: Cow::Owned(format!("{title} ({date})")),
                file: Cow::Owned(file),
                notes: &[
                    "Sortie de l'assistant (skimap assist) pour le jour indiqué : jamais une garantie de sécurité ; vérifier BERA, neige et règles.",
                ],
                ..base
            });
        }
    }
    Ok(v)
}

/// Metadata of a pipeline layer.
#[derive(Debug, Clone)]
pub struct MetaRead {
    /// Metadata in the shared schema.
    pub meta: LayerMeta,
    /// `false`: the file does not follow `common::meta::LayerMeta` and was read leniently
    /// (`voice.meta.json`, `offpiste.meta.json`, `routes.meta.json` on 15/11/2026).
    pub conforming: bool,
}

fn str_field(v: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|k| v.get(*k).and_then(Value::as_str))
        .map(str::to_owned)
}

/// Lenient reading of a meta file that does not follow the shared schema (older
/// spellings `license`, `valid_for`, `valid_at`, `data_date` accepted).
pub fn lenient_meta(v: &Value) -> LayerMeta {
    let mut m = LayerMeta::new(
        str_field(v, &["layer"]).unwrap_or_default(),
        str_field(v, &["site"]).unwrap_or_default(),
    );
    m.generated_at = str_field(v, &["generated_at"]).unwrap_or_default();
    m.valid_from = str_field(v, &["valid_from", "valid_at"]);
    m.valid_until = str_field(v, &["valid_until"]);
    m.validity = str_field(v, &["validity", "valid_for"]);
    m.crs = str_field(v, &["crs"]);
    if let Some(arr) = v.get("sources").and_then(Value::as_array) {
        m.sources = arr
            .iter()
            .map(|s| {
                let mut r = Source::new(
                    str_field(s, &["name", "title"]).unwrap_or_else(|| "(sans nom)".to_owned()),
                    str_field(s, &["url"]).unwrap_or_default(),
                    str_field(s, &["licence", "license"]).unwrap_or_default(),
                    str_field(s, &["attribution"]).unwrap_or_default(),
                );
                r.personal_use_only = s
                    .get("personal_use_only")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                r.edition = str_field(s, &["edition", "data_date"]);
                r.producer = str_field(s, &["producer"]);
                r
            })
            .collect();
    }
    m
}

/// Reads a `<couche>.meta.json` (`None` if absent): the shared schema, else leniently.
pub fn read_meta(path: &Path) -> Result<Option<MetaRead>> {
    if !path.is_file() {
        return Ok(None);
    }
    if let Ok(meta) = LayerMeta::read(path) {
        return Ok(Some(MetaRead {
            meta,
            conforming: true,
        }));
    }
    let text = std::fs::read_to_string(path).map_err(|e| Error::io(path, e))?;
    let v: Value =
        serde_json::from_str(&text).map_err(|e| Error::json(path.display().to_string(), e))?;
    Ok(Some(MetaRead {
        meta: lenient_meta(&v),
        conforming: false,
    }))
}

/// Copies a meta file into `layers/meta/<origin>/` once; returns its pack path.
pub fn copy_meta(dir: &Path, name: &str, root: &Path) -> Result<Option<String>> {
    if !dir.join(name).is_file() {
        return Ok(None);
    }
    let origin = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let rel = format!("layers/meta/{origin}/{name}");
    if !root.join(&rel).is_file() {
        copy_data(&dir.join(name), &root.join(&rel))?;
    }
    Ok(Some(rel))
}

/// What a layer entry is made of.
#[derive(Debug, Clone)]
pub struct EntryParts<'a> {
    /// Identifier.
    pub id: &'a str,
    /// Title.
    pub title: &'a str,
    /// Kind.
    pub kind: LayerKind,
    /// Format.
    pub format: String,
    /// Pack file.
    pub file: String,
    /// Pipeline origin.
    pub origin: String,
    /// Pack path of the copied meta file.
    pub meta_rel: Option<String>,
    /// Size of the pack file.
    pub bytes: u64,
    /// Shown when the viewer opens.
    pub visible: bool,
    /// Notes.
    pub notes: &'a [&'a str],
}

/// A layer entry from its parts and the metadata of its pipeline layer.
pub fn layer_entry(p: EntryParts<'_>, meta: Option<&MetaRead>) -> Layer {
    let mut notes: Vec<String> = p.notes.iter().map(|s| (*s).to_owned()).collect();
    if meta.is_some_and(|m| !m.conforming) {
        notes.push("Métadonnées hors du schéma common::meta, lues avec tolérance.".to_owned());
    }
    let sources = meta.map(|m| m.meta.sources.clone()).unwrap_or_default();
    Layer {
        id: p.id.to_owned(),
        title: p.title.to_owned(),
        kind: p.kind,
        format: p.format,
        file: p.file,
        meta: p.meta_rel,
        origin: p.origin,
        generated_at: meta
            .map(|m| m.meta.generated_at.clone())
            .filter(|g| !g.is_empty()),
        valid_from: meta.and_then(|m| m.meta.valid_from.clone()),
        valid_until: meta.and_then(|m| m.meta.valid_until.clone()),
        validity: meta.and_then(|m| m.meta.validity.clone()),
        personal_use_only: restricted(&sources),
        sources,
        bytes: p.bytes,
        features: None,
        geometry_types: Vec::new(),
        visible: p.visible,
        raster: None,
        classes: None,
        notes,
    }
}

/// Looks for a layer in `out` (`data/out/<site>/`) and copies it into `root`.
pub fn include(spec: &Spec, out: &Path, root: &Path, personal_sources: bool) -> Result<Outcome> {
    let dir = out.join(spec.origin);
    let src = dir.join(spec.file.as_ref());
    if !src.is_file() {
        return Ok(Outcome::Missing(Missing {
            id: spec.id.to_string(),
            reason: format!(
                "{}/{} absent des sorties du pipeline",
                spec.origin, spec.file
            ),
        }));
    }
    let mut meta = match spec.meta {
        Some(m) => read_meta(&dir.join(m))?,
        None => None,
    };
    // The sources of this file only: those of its licence group (#169).
    // Estimated pistes: the whole file in a personal pack, the features of public
    // licences in a published one.
    let group = if spec.transform == Transform::EstimatedPistes {
        Some(if personal_sources {
            Group::Personal
        } else {
            Group::Odbl
        })
    } else {
        file_group(spec, meta.as_ref())
    };
    // The metadata of the layer names a personal source (an earlier run of the
    // assistant, #151) while this file is of a published group: the pack gets the
    // metadata of the group only.
    let personal_meta = meta.as_ref().is_some_and(|m| restricted(&m.meta.sources));
    if let (Some(m), Some(g)) = (meta.as_mut(), group) {
        m.meta.sources = sources_of_group(&m.meta.sources, g);
    }
    let own = meta.as_ref().is_some_and(|m| restricted(&m.meta.sources))
        || group == Some(Group::Personal);
    let derived = personal_inputs(out, spec.depends)?;
    let personal = own || !derived.is_empty() || spec.personal.is_some();
    if personal && !personal_sources {
        return Ok(Outcome::Missing(Missing {
            id: spec.id.to_string(),
            reason: match spec.personal {
                Some(why) => format!(
                    "usage personnel seulement ({why}) : option personal_sources désactivée"
                ),
                None => "source à usage personnel seulement : option personal_sources désactivée"
                    .to_owned(),
            },
        }));
    }
    let ext = ext_of(&spec.file);
    let (file, format) = match spec.transform {
        Transform::Copy => (format!("layers/{}.{ext}", spec.id), ext.clone()),
        Transform::Gzip => (
            format!("layers/{}.{ext}.gz", spec.id),
            format!("{ext}+gzip"),
        ),
        Transform::GeoJson
        | Transform::Zones
        | Transform::Pistes
        | Transform::EstimatedPistes
        | Transform::Licence(_) => (format!("layers/{}.geojson", spec.id), "geojson".to_owned()),
    };
    let dst = root.join(&file);
    let mut features = None;
    let mut geometry_types = Vec::new();
    let mut joined: Option<crate::build::pistes::PistesReport> = None;
    let bytes = match spec.transform {
        Transform::Copy => copy_data(&src, &dst)?,
        Transform::Gzip => gzip_file(&src, &dst)?,
        Transform::GeoJson
        | Transform::Zones
        | Transform::Pistes
        | Transform::EstimatedPistes
        | Transform::Licence(_) => {
            let text = std::fs::read_to_string(&src).map_err(|e| Error::io(&src, e))?;
            let mut v: Value = serde_json::from_str(&text)
                .map_err(|e| Error::json(src.display().to_string(), e))?;
            if spec.transform == Transform::Zones {
                let rules_path = dir.join("rules.json");
                let rules_text =
                    std::fs::read_to_string(&rules_path).map_err(|e| Error::io(&rules_path, e))?;
                annotate_zones(&mut v, &text, &rules_text, chrono::Utc::now().date_naive())?;
            }
            if spec.transform == Transform::Pistes {
                joined = Some(crate::build::pistes::enrich(&mut v, out, personal_sources)?);
            }
            if spec.transform == Transform::EstimatedPistes {
                let all = v["features"].as_array().map_or(0, Vec::len);
                if let (Some(fs), false) = (
                    v.get_mut("features").and_then(Value::as_array_mut),
                    personal_sources,
                ) {
                    fs.retain(|f| feature_group(&f["properties"]) == Group::Odbl);
                }
                v = estimated_pistes(&v);
                if v["features"].as_array().is_none_or(Vec::is_empty) {
                    return Ok(Outcome::Missing(Missing {
                        id: spec.id.to_string(),
                        reason: if all > 0 {
                            format!(
                                "{all} pistes estimées, toutes d'une source à usage personnel seulement : option personal_sources désactivée"
                            )
                        } else {
                            "aucune piste estimée hors d'OSM sur ce site".to_owned()
                        },
                    }));
                }
            }
            if let Transform::Licence(g) = spec.transform {
                if let Some(fs) = v.get_mut("features").and_then(Value::as_array_mut) {
                    fs.retain(|f| feature_group(&f["properties"]) == g);
                    if !g.fits_osm() {
                        fs.iter_mut()
                            .for_each(|f| osm_stations_by_key(&mut f["properties"]));
                    }
                }
                if v["features"].as_array().is_none_or(Vec::is_empty) {
                    return Ok(Outcome::Missing(Missing {
                        id: spec.id.to_string(),
                        reason: format!(
                            "aucune entité de {} sous une licence du groupe {g:?}",
                            spec.file
                        ),
                    }));
                }
            }
            let (n, types) = compact_geojson(&mut v, &src)?;
            features = Some(n);
            geometry_types = types;
            let out = serde_json::to_vec(&v).map_err(|e| Error::json(&file, e))?;
            write_bytes(&dst, &out)?
        }
    };
    let meta_rel = match (spec.meta, spec.transform, meta.as_ref()) {
        // The metadata of a licence group: its own sources, no figures (they name the
        // itineraries of every source).
        (Some(m), Transform::Licence(g), Some(read)) => {
            Some(write_group_meta(&dir, m, g, &read.meta, root, false)?)
        }
        (Some(m), _, Some(read)) if personal_meta && !personal_sources => match group {
            Some(g) => Some(write_group_meta(&dir, m, g, &read.meta, root, true)?),
            None => copy_meta(&dir, m, root)?,
        },
        (Some(m), _, _) => copy_meta(&dir, m, root)?,
        (None, _, _) => None,
    };
    let mut l = layer_entry(
        EntryParts {
            id: &spec.id,
            title: &spec.title,
            kind: spec.kind,
            format,
            file,
            origin: format!("{}/{}", spec.origin, spec.file),
            meta_rel,
            bytes,
            visible: spec.visible,
            notes: spec.notes,
        },
        meta.as_ref(),
    );
    l.features = features;
    l.geometry_types = geometry_types;
    if let Some(r) = joined {
        for s in r.sources.iter().cloned() {
            if !l.sources.iter().any(|x| x.name == s.name && x.url == s.url) {
                l.sources.push(s);
            }
        }
        l.personal_use_only = restricted(&l.sources);
        l.notes.push(format!(
            "État par voie : {} ; couleur du plan : {} voies sans couleur OSM coloriées, {} avec une couleur absente d'OSM (OSM gardé), {} conformes à OSM ; sens du plan : {} voies (conflits {}) ; noms du plan : {}.",
            r.states
                .iter()
                .map(|(k, n)| format!("{k} {n}"))
                .collect::<Vec<_>>()
                .join(", "),
            r.colour_from_plan,
            r.colour_plan_differs,
            r.colour_plan_agrees,
            r.oneway_from_plan,
            r.oneway_plan_conflicts,
            r.name_from_plan
        ));
    }
    if let Some(why) = spec.personal {
        l.personal_use_only = true;
        l.notes.push(format!("Usage personnel : {why}."));
    }
    if group == Some(Group::Personal) {
        l.personal_use_only = true;
    }
    if !derived.is_empty() {
        l.personal_use_only = true;
        l.notes.push(format!(
            "Calculée à partir de couches à usage personnel ({}) : usage personnel.",
            derived.join(", ")
        ));
    }
    Ok(Outcome::Included(Box::new(l)))
}

/// Licence group of the file of `spec` (#169): the group of a [`Transform::Licence`]
/// layer; else the group of the licence its metadata gives the file (`files[].licence`)
/// when that licence is known (the refuges: one file per source); else none (the sources
/// of the layer are kept as they are).
pub fn file_group(spec: &Spec, meta: Option<&MetaRead>) -> Option<Group> {
    if let Transform::Licence(g) = spec.transform {
        return Some(g);
    }
    let f = meta?
        .meta
        .files
        .iter()
        .find(|f| f.name == spec.file.as_ref())?;
    match licence::classify(f.licence.as_deref()?) {
        Family::Personal => None,
        Family::ShareAlike => Some(Group::ShareAlike),
        Family::Odbl | Family::Attribution => Some(Group::Odbl),
    }
}

/// The sources of a file of licence group `g`: those whose family fits it (ODbL file:
/// ODbL and attribution-only sources; CC BY-SA file: CC BY-SA and attribution-only
/// sources); a personal file keeps them all (it is made of all of them).
pub fn sources_of_group(sources: &[Source], g: Group) -> Vec<Source> {
    sources
        .iter()
        .filter(|s| g == Group::Personal || licence::of_source(s).fits(g))
        .cloned()
        .collect()
}

/// Writes the metadata of a licence group of a pipeline layer
/// (`layers/meta/<origin>/<stem><suffix>.meta.json`): the metadata with the sources of the
/// group only and without the figures and parameters (which name the itineraries of
/// every source). With `own_files`, the files of the layer carry their own licence
/// (the assistant, one per run, #151): only those of the group are listed. Returns its
/// pack path.
fn write_group_meta(
    dir: &Path,
    name: &str,
    g: Group,
    meta: &LayerMeta,
    root: &Path,
    own_files: bool,
) -> Result<String> {
    let origin = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let stem = name.trim_end_matches(".meta.json");
    let rel = format!("layers/meta/{origin}/{stem}{}.meta.json", g.suffix());
    let mut m = meta.clone();
    m.stats = Value::Null;
    m.parameters = Value::Null;
    if own_files {
        m.files.retain(|f| {
            f.licence
                .as_deref()
                .map(licence::classify)
                .is_some_and(|fam| fam.public() && fam.fits(g))
        });
    }
    m.notes.push(format!(
        "Groupe de licences du fichier (#169) : {} ; sources du groupe seulement.",
        g.licence_fr()
    ));
    for f in &mut m.files {
        f.licence = Some(g.licence_fr().to_owned());
        f.count = None;
    }
    let bytes = serde_json::to_vec(&m).map_err(|e| Error::json(&rel, e))?;
    write_bytes(&root.join(&rel), &bytes)?;
    Ok(rel)
}

/// Pack layer of the estimated traces (G1).
pub const ESTIMATED_LAYER: &str = "pistes-estimees";

/// Difficulty of the pistes layer for a plan colour (French scale).
fn difficulty_of_colour(c: &str) -> Option<&'static str> {
    match c {
        "green" => Some("novice"),
        "blue" => Some("easy"),
        "red" => Some("intermediate"),
        "black" => Some("advanced"),
        _ => None,
    }
}

/// The estimated traces of `pistes_estimees.geojson` (certainty `incertain`, or an
/// `estimated_*` method in a file written without certainty), with the properties the
/// styles and the cards read; `source_plan_url` becomes `plan_url`.
pub fn estimated_pistes(v: &Value) -> Value {
    let keep = [
        "name",
        "colour",
        "kind",
        "certainty",
        "method",
        "score",
        "length_m",
        "length_km_announced",
        "lengths_km_plan",
        "departures",
        "named_by",
        "source_name",
        "source_url",
        "route_ends",
        "plan_url",
        "licences",
    ];
    let features: Vec<Value> = v["features"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|f| {
            let p = &f["properties"];
            p["certainty"] == "incertain"
                || p["method"]
                    .as_str()
                    .is_some_and(|m| m.starts_with("estimated_"))
        })
        .map(|f| {
            let p = &f["properties"];
            let mut q = serde_json::Map::new();
            for k in keep {
                if let Some(x) = p.get(k).filter(|x| !x.is_null()) {
                    q.insert(k.to_owned(), x.clone());
                }
            }
            q.insert("certainty".into(), json!("incertain"));
            if let Some(d) = p["colour"].as_str().and_then(difficulty_of_colour) {
                q.insert("difficulty".into(), json!(d));
            }
            if let Some(u) = p["source_plan_url"].as_str() {
                q.insert("plan_url".into(), json!(u));
            }
            json!({"type": "Feature", "geometry": f["geometry"].clone(), "properties": q})
        })
        .collect();
    json!({"type": "FeatureCollection", "features": features})
}

/// Sources refused by name in a published pack ([`licence::UNAUTHORISED_SOURCES`]).
pub use skimap_common::licence::UNAUTHORISED_SOURCES;

/// Whether a source may not go into a published pack: the negation of
/// [`licence::publishable`] (its licence family is personal: marked `personal_use_only`,
/// no reuse licence known, a licence still to confirm, #171; or it is one of
/// [`UNAUTHORISED_SOURCES`]).
pub fn unauthorised(s: &Source) -> bool {
    !licence::publishable(s)
}

/// The OSM stations an SRN area cites (`stations[]`), in a file that may not hold OSM
/// data (CC BY-SA): only their OSM key (`id`), a reference by key that the OSMF
/// « Collective Database » guideline admits; their name and the figures computed from
/// their OSM geometry (`area_share`, `km_inside`) are left out (#171).
pub fn osm_stations_by_key(props: &mut Value) {
    if let Some(st) = props.get_mut("stations").and_then(Value::as_array_mut) {
        for s in st.iter_mut() {
            if let Some(o) = s.as_object_mut() {
                o.retain(|k, _| k == "id");
            }
        }
    }
}

/// Whether any of `sources` may not go into a published pack ([`unauthorised`]).
pub fn restricted(sources: &[Source]) -> bool {
    sources.iter().any(unauthorised)
}

/// Whether the pipeline layer `origin` (`<origin>/<origin>.meta.json`) has a
/// personal-use-only source.
pub fn personal_origin(out: &Path, origin: &str) -> Result<bool> {
    let path = out.join(origin).join(format!("{origin}.meta.json"));
    Ok(read_meta(&path)?.is_some_and(|m| restricted(&m.meta.sources)))
}

/// The layers of `depends` that have a personal-use-only source.
pub fn personal_inputs(out: &Path, depends: &[&str]) -> Result<Vec<String>> {
    let mut v = Vec::new();
    for d in depends {
        if personal_origin(out, d)? {
            v.push((*d).to_owned());
        }
    }
    Ok(v)
}

fn ext_of(file: &str) -> String {
    Path::new(file)
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_else(|| "bin".to_owned())
}

/// Gzip copy (level 6); returns the compressed size.
pub fn gzip_file(src: &Path, dst: &Path) -> Result<u64> {
    let bytes = std::fs::read(src).map_err(|e| Error::io(src, e))?;
    let mut z = flate2::write::GzEncoder::new(
        Vec::with_capacity(bytes.len() / 2),
        flate2::Compression::new(6),
    );
    std::io::Write::write_all(&mut z, &bytes).map_err(|e| Error::io(dst, e))?;
    let out = z.finish().map_err(|e| Error::io(dst, e))?;
    write_bytes(dst, &out)
}

/// Pipeline files `<prefix>*<suffix>` of `out/<origin>/`, sorted.
pub fn discover(out: &Path, origin: &str, prefix: &str, suffix: &str) -> Result<Vec<String>> {
    let dir = out.join(origin);
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut v: Vec<String> = crate::build::read_dir_sorted(&dir)?
        .into_iter()
        .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .filter(|n| n.starts_with(prefix) && n.ends_with(suffix))
        .collect();
    v.sort();
    Ok(v)
}

fn round_coords(v: &mut Value) {
    match v {
        Value::Number(n) => {
            if let Some(f) = n.as_f64() {
                let r = (f * 1e6).round() / 1e6;
                if let Some(m) = serde_json::Number::from_f64(r) {
                    *n = m;
                }
            }
        }
        Value::Array(a) => a.iter_mut().for_each(round_coords),
        _ => {}
    }
}

fn round_geometry(g: &mut Value, types: &mut BTreeSet<String>) {
    if let Some(t) = g.get("type").and_then(Value::as_str) {
        types.insert(t.to_owned());
    }
    if let Some(c) = g.get_mut("coordinates") {
        round_coords(c);
    }
    if let Some(Value::Array(gs)) = g.get_mut("geometries") {
        for g in gs {
            round_geometry(g, types);
        }
    }
}

/// Rounds the coordinates of a FeatureCollection; returns the feature count and the
/// geometry types.
pub fn compact_geojson(v: &mut Value, src: &Path) -> Result<(u64, Vec<String>)> {
    let Some(features) = v.get_mut("features").and_then(Value::as_array_mut) else {
        return Err(Error::GeoJson {
            context: src.display().to_string(),
            message: "not a FeatureCollection".into(),
        });
    };
    let mut types = BTreeSet::new();
    for f in features.iter_mut() {
        if let Some(g) = f.get_mut("geometry")
            && !g.is_null()
        {
            round_geometry(g, &mut types);
        }
    }
    Ok((features.len() as u64, types.into_iter().collect()))
}

/// French label of a rule topic.
pub fn topic_fr(t: Topic) -> &'static str {
    match t {
        Topic::Access => "Accès",
        Topic::LeaveRoutes => "Sortie des itinéraires",
        Topic::Bivouac => "Bivouac",
        Topic::Fire => "Feu",
    }
}

/// French label of a rule effect.
pub fn effect_fr(e: Effect) -> &'static str {
    match e {
        Effect::Forbidden => "interdit",
        Effect::Tolerated => "toléré sous conditions",
        Effect::ConsentRequired => "avec l'accord du propriétaire",
        Effect::AvoidRecommended => "à éviter (recommandation)",
        Effect::Allowed => "autorisé",
    }
}

/// Adds to every zone feature `rules` and `rules_text`, and the simple fields derived
/// from the rules ([`skimap_zones::fields`], rules in force on `date` or later):
/// `bivouac`, `bivouac_period`, `bivouac_rules`, `access`, `access_period`,
/// `access_rules`; and the act of the zone with how far it was read
/// ([`skimap_zones::local`]): `local_act`, `local_rules` (`read`, `partial`,
/// `unverified`, null without act of its own).
pub fn annotate_zones(
    v: &mut Value,
    zones_text: &str,
    rules_text: &str,
    date: chrono::NaiveDate,
) -> Result<()> {
    let layer = ZoneLayer::from_geojson_str(zones_text)?;
    let rules = RuleSet::from_json_str(rules_text)?;
    let Some(features) = v.get_mut("features").and_then(Value::as_array_mut) else {
        return Err(Error::Pack(
            "zones.geojson is not a FeatureCollection".into(),
        ));
    };
    for f in features.iter_mut() {
        let Some(id) = f
            .pointer("/properties/id")
            .and_then(Value::as_str)
            .map(str::to_owned)
        else {
            continue;
        };
        let Some(zone) = layer.get(&id) else { continue };
        let mut list = Vec::new();
        let mut text = Vec::new();
        for r in rules.rules.iter().filter(|r| r.applies_to_zone(zone)) {
            let period = r.period.describe_fr();
            let src = r.primary_source();
            list.push(json!({
                "id": r.id,
                "topic": r.topic,
                "effect": r.effect,
                "period": period,
                "summary": r.summary,
                "conditions": r.conditions,
                "only_where": r.only_where,
                "source_title": src.title,
                "source_url": src.url,
            }));
            let mut line = format!(
                "{} : {} ({period}). {}",
                topic_fr(r.topic),
                effect_fr(r.effect),
                r.summary
            );
            if let Some(c) = &r.conditions {
                line.push_str(&format!(" Conditions : {c}"));
            }
            if let Some(w) = &r.only_where {
                line.push_str(&format!(" Seulement : {w}."));
            }
            text.push(line);
        }
        let fields = skimap_zones::fields::zone_fields(&layer, &rules, zone, date);
        if let Some(props) = f.get_mut("properties").and_then(Value::as_object_mut) {
            props.insert("bivouac".to_owned(), json!(fields.bivouac.as_str()));
            props.insert("bivouac_period".to_owned(), json!(fields.bivouac_period));
            props.insert("bivouac_rules".to_owned(), json!(fields.bivouac_rules));
            props.insert("access".to_owned(), json!(fields.access.as_str()));
            props.insert("access_period".to_owned(), json!(fields.access_period));
            props.insert("access_rules".to_owned(), json!(fields.access_rules));
            let local = skimap_zones::local::local_status(&rules, zone);
            props.insert(
                "local_act".to_owned(),
                json!(local.map(|(a, _)| a.as_str())),
            );
            props.insert(
                "local_rules".to_owned(),
                json!(local.map(|(_, s)| s.as_str())),
            );
            props.insert("rules".to_owned(), Value::Array(list));
            props.insert(
                "rules_text".to_owned(),
                if text.is_empty() {
                    Value::Null
                } else {
                    Value::String(text.join("\n"))
                },
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn sources_both_spellings_and_personal() {
        let m = json!({"sources": [
            {"name": "a", "licence": "LO", "attribution": "A"},
            {"name": "b", "license": "ODbL", "personal_use_only": true}
        ], "valid_until": "2026-12-14", "valid_at": null, "valid_for": "un jour"});
        let lm = lenient_meta(&m);
        let s = &lm.sources;
        assert_eq!(s[0].licence, "LO");
        assert_eq!(s[1].licence, "ODbL");
        assert!(!s[0].personal_use_only && s[1].personal_use_only);
        assert_eq!(lm.valid_until.as_deref(), Some("2026-12-14"));
        assert_eq!(lm.valid_from, None);
        assert_eq!(lm.validity.as_deref(), Some("un jour"));
    }

    /// SRN itineraries and areas of an SRN site (#169): a published pack takes the ODbL
    /// and Licence Ouverte ones in `srn-*`, the CC BY-SA ones apart in `srn-*-cc-by-sa`
    /// with their own sources, nothing without a reuse licence nor any area mixing ODbL
    /// and CC BY-SA; a personal pack adds those in `srn-*-personnel`.
    #[test]
    fn srn_layers_split_by_licence() {
        type Got = BTreeMap<String, (Vec<String>, Vec<String>, bool)>;
        let out = tempfile::tempdir().unwrap();
        let dir = out.path().join("srn");
        std::fs::create_dir_all(&dir).unwrap();
        let line = json!({"type": "LineString", "coordinates": [[6.0, 46.0], [6.01, 46.01]]});
        let poly = json!({"type": "Polygon", "coordinates": [[[6.0, 46.0], [6.01, 46.0], [6.01, 46.01], [6.0, 46.0]]]});
        let gt = "aucune licence affichée par l'instance ; publié sous Licence Ouverte 2.0 : à confirmer";
        let route = |id: &str, l: &str| {
            json!({"type": "Feature", "geometry": line.clone(),
                   "properties": {"id": id, "name": id, "licence": l, "activity": "srn"}})
        };
        let area = |id: &str, ls: &[&str]| {
            json!({"type": "Feature", "geometry": poly.clone(),
                   "properties": {"id": id, "kind": "domaine", "licences": ls}})
        };
        let routes = json!({"type": "FeatureCollection", "features": [
            route("osm", "ODbL 1.0"),
            route("dt", "Licence Ouverte 2.0 (Etalab), via DATAtourisme"),
            route("c2c", "CC BY-SA 3.0"),
            route("gt", gt),
        ]});
        let areas = json!({"type": "FeatureCollection", "features": [
            area("a-osm-dt", &["ODbL 1.0", "Licence Ouverte 2.0"]),
            area("a-c2c", &["CC BY-SA 3.0"]),
            area("a-osm-c2c", &["CC BY-SA 3.0", "ODbL 1.0"]),
            area("a-gt", &[gt, "CC BY-SA 3.0"]),
        ]});
        std::fs::write(dir.join("srn_routes.geojson"), routes.to_string()).unwrap();
        std::fs::write(dir.join("srn_areas.geojson"), areas.to_string()).unwrap();
        let src = |name: &str, l: &str| json!({"name": name, "url": "https://x", "licence": l, "attribution": name});
        let meta = json!({"layer": "srn", "site": "s", "generated_at": "2026-11-17T00:00:00Z",
            "sources": [src("OpenSnowMap", "ODbL 1.0"), src("DATAtourisme", "Licence Ouverte 2.0"),
                        src("Camptocamp", "CC BY-SA 3.0"), src("Geotrek PNR", gt)],
            "stats": {"routes_dropped_groomed_not_osm": [{"name": "gt"}]}});
        std::fs::write(dir.join("srn.meta.json"), meta.to_string()).unwrap();
        let run = |personal: bool| -> Got {
            let root = tempfile::tempdir().unwrap();
            let mut got = BTreeMap::new();
            for spec in catalog().into_iter().filter(|s| s.origin == "srn") {
                let Outcome::Included(l) =
                    include(&spec, out.path(), root.path(), personal).unwrap()
                else {
                    continue;
                };
                let read = |rel: &str| -> Value {
                    serde_json::from_str(&std::fs::read_to_string(root.path().join(rel)).unwrap())
                        .unwrap()
                };
                let ids = read(&l.file)["features"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|f| f["properties"]["id"].as_str().unwrap().to_owned())
                    .collect();
                let meta = read(l.meta.as_deref().unwrap());
                assert!(meta["stats"].is_null(), "{}: figures of every source", l.id);
                let names = l.sources.iter().map(|s| s.name.clone()).collect();
                got.insert(l.id.clone(), (ids, names, l.personal_use_only));
            }
            got
        };
        let s = |v: &[&str]| v.iter().map(|x| (*x).to_owned()).collect::<Vec<_>>();
        let public = run(false);
        assert_eq!(
            public.keys().collect::<Vec<_>>(),
            [
                "srn-areas",
                "srn-areas-cc-by-sa",
                "srn-routes",
                "srn-routes-cc-by-sa"
            ]
        );
        assert_eq!(
            public["srn-routes"],
            (
                s(&["osm", "dt"]),
                s(&["OpenSnowMap", "DATAtourisme"]),
                false
            )
        );
        assert_eq!(
            public["srn-routes-cc-by-sa"],
            (s(&["c2c"]), s(&["DATAtourisme", "Camptocamp"]), false)
        );
        assert_eq!(public["srn-areas"].0, s(&["a-osm-dt"]));
        assert_eq!(public["srn-areas-cc-by-sa"].0, s(&["a-c2c"]));
        let personal = run(true);
        assert_eq!(personal["srn-routes-personnel"].0, s(&["gt"]));
        assert_eq!(personal["srn-areas-personnel"].0, s(&["a-osm-c2c", "a-gt"]));
        assert!(personal["srn-routes-personnel"].2 && !personal["srn-routes"].2);
        assert_eq!(personal["srn-routes"], public["srn-routes"]);
    }

    /// A source whose licence is still to be checked (« non vérifiée ») keeps its layer
    /// out of a published pack, even when it is not marked `personal_use_only` (#171):
    /// the decision is the licence family of `skimap_common::licence`.
    #[test]
    fn unverified_licences_stay_out_of_public_packs() {
        let out = tempfile::tempdir().unwrap();
        let dir = out.path().join("coverage");
        std::fs::create_dir_all(&dir).unwrap();
        let fc = json!({"type": "FeatureCollection", "features": [{"type": "Feature",
            "geometry": {"type": "Point", "coordinates": [6.0, 46.0]}, "properties": {}}]});
        std::fs::write(dir.join("no_network.geojson"), fc.to_string()).unwrap();
        let src = |name: &str, l: &str| json!({"name": name, "url": "https://x", "licence": l, "attribution": name});
        let write_meta = |l: &str| {
            let meta = json!({"layer": "coverage", "site": "s", "generated_at": "2026-11-17T00:00:00Z",
                "sources": [src("ARCEP", "Licence Ouverte (fr-lo)"), src("Contours", l)]});
            std::fs::write(dir.join("coverage.meta.json"), meta.to_string()).unwrap();
        };
        let spec = catalog()
            .into_iter()
            .find(|s| s.id == "no-network")
            .unwrap();
        let root = tempfile::tempdir().unwrap();
        write_meta("non vérifiée (F5, #11)");
        assert!(matches!(
            include(&spec, out.path(), root.path(), false).unwrap(),
            Outcome::Missing(_)
        ));
        let Outcome::Included(l) = include(&spec, out.path(), root.path(), true).unwrap() else {
            panic!("personal pack");
        };
        assert!(l.personal_use_only);
        write_meta("ODbL 1.0 (jeu « Contours administratifs »)");
        let Outcome::Included(l) = include(&spec, out.path(), root.path(), false).unwrap() else {
            panic!("published pack");
        };
        assert!(!l.personal_use_only);
    }

    /// An SRN area of the CC BY-SA file names no OSM station and carries no figure
    /// computed from OSM geometry: only the OSM key of the station (reference by key,
    /// OSMF « Collective Database » guideline); the ODbL file keeps them (#171).
    #[test]
    fn cc_by_sa_areas_cite_osm_stations_by_key_only() {
        let out = tempfile::tempdir().unwrap();
        let dir = out.path().join("srn");
        std::fs::create_dir_all(&dir).unwrap();
        let poly = json!({"type": "Polygon", "coordinates": [[[6.0, 46.0], [6.01, 46.0], [6.01, 46.01], [6.0, 46.0]]]});
        let station =
            json!([{"id": "site-1", "name": "Station OSM", "area_share": 0.5, "km_inside": 3.2}]);
        let area = |id: &str, l: &str| {
            json!({"type": "Feature", "geometry": poly.clone(),
                   "properties": {"id": id, "kind": "domaine", "licences": [l], "stations": station.clone()}})
        };
        let areas = json!({"type": "FeatureCollection", "features": [
            area("a-osm", "ODbL 1.0"), area("a-c2c", "CC BY-SA 3.0")]});
        std::fs::write(dir.join("srn_areas.geojson"), areas.to_string()).unwrap();
        let meta = json!({"layer": "srn", "site": "s", "generated_at": "2026-11-17T00:00:00Z",
            "sources": [{"name": "OSM", "url": "https://x", "licence": "ODbL 1.0", "attribution": "OSM"},
                        {"name": "Camptocamp", "url": "https://x", "licence": "CC BY-SA 3.0", "attribution": "C2C"}]});
        std::fs::write(dir.join("srn.meta.json"), meta.to_string()).unwrap();
        let root = tempfile::tempdir().unwrap();
        let stations = |id: &str| -> Value {
            let spec = catalog().into_iter().find(|s| s.id == id).unwrap();
            let Outcome::Included(l) = include(&spec, out.path(), root.path(), false).unwrap()
            else {
                panic!("{id}");
            };
            let v: Value =
                serde_json::from_str(&std::fs::read_to_string(root.path().join(&l.file)).unwrap())
                    .unwrap();
            v["features"][0]["properties"]["stations"].clone()
        };
        assert_eq!(stations("srn-areas"), station);
        assert_eq!(stations("srn-areas-cc-by-sa"), json!([{"id": "site-1"}]));
    }

    #[test]
    fn keeps_only_the_estimated_traces() {
        let v = json!({"type": "FeatureCollection", "features": [
            {"type": "Feature", "geometry": {"type": "MultiLineString", "coordinates": [[[6.0, 46.0], [6.01, 46.01]]]},
             "properties": {"name": "Sûre", "method": "exact", "certainty": "exact", "colour": "blue"}},
            {"type": "Feature", "geometry": {"type": "MultiLineString", "coordinates": [[[6.0, 46.0], [6.01, 46.01]]]},
             "properties": {"name": "Le Gy", "method": "estimated_paths", "certainty": "incertain", "colour": "green",
                            "score": 0.5, "osm_way_ids": [], "source_plan_url": "https://example.org/plan.pdf"}},
            {"type": "Feature", "geometry": {"type": "MultiLineString", "coordinates": [[[6.0, 46.0], [6.01, 46.01]]]},
             "properties": {"name": "Ancien", "method": "estimated_drawing", "colour": "black"}}
        ]});
        let e = estimated_pistes(&v);
        let fs = e["features"].as_array().unwrap();
        assert_eq!(fs.len(), 2);
        let p = &fs[0]["properties"];
        assert_eq!(p["name"], "Le Gy");
        assert_eq!(p["difficulty"], "novice");
        assert_eq!(p["plan_url"], "https://example.org/plan.pdf");
        assert!(p.get("osm_way_ids").is_none() && p.get("source_plan_url").is_none());
        assert_eq!(fs[1]["properties"]["certainty"], "incertain");
        assert_eq!(fs[1]["properties"]["difficulty"], "advanced");
    }

    /// Writes `pistes_estimees/` with `features` and a personal source.
    fn estimated_out(features: Value) -> tempfile::TempDir {
        let out = tempfile::tempdir().unwrap();
        let dir = out.path().join("pistes_estimees");
        std::fs::create_dir_all(&dir).unwrap();
        let fc = json!({"type": "FeatureCollection", "features": features});
        std::fs::write(dir.join("pistes_estimees.geojson"), fc.to_string()).unwrap();
        let meta = json!({"layer": "pistes_estimees", "site": "s", "generated_at": "2026-11-17T00:00:00Z",
            "sources": [{"name": "plan.pdf", "url": "https://example.org/plan.pdf", "licence": "aucune",
                         "attribution": "Station", "personal_use_only": true},
                        {"name": "BD TOPO", "url": "https://data.geopf.fr", "licence": "Licence Ouverte Etalab 2.0",
                         "attribution": "IGN", "personal_use_only": false}]});
        std::fs::write(dir.join("pistes_estimees.meta.json"), meta.to_string()).unwrap();
        out
    }

    fn estimated_spec() -> Spec {
        catalog()
            .into_iter()
            .find(|s| s.id == ESTIMATED_LAYER)
            .unwrap()
    }

    /// The estimated traces derived from the plan (personal use only): never in a public
    /// pack, in a personal one with only the uncertain features.
    #[test]
    fn estimated_pistes_stay_out_of_public_packs() {
        let root = tempfile::tempdir().unwrap();
        let out = estimated_out(json!([
            {"type": "Feature", "geometry": {"type": "MultiLineString", "coordinates": [[[6.0, 46.0], [6.01, 46.01]]]},
             "properties": {"name": "Sûre", "method": "exact", "certainty": "exact", "colour": "blue"}},
            {"type": "Feature", "geometry": {"type": "MultiLineString", "coordinates": [[[6.0, 46.0], [6.01, 46.01]]]},
             "properties": {"name": "Le Gy", "method": "estimated_drawing", "certainty": "incertain", "colour": "green",
                            "licences": ["aucune licence de réutilisation (usage personnel) : aucune", "Licence Ouverte Etalab 2.0"]}}
        ]));
        let spec = estimated_spec();
        match include(&spec, out.path(), root.path(), false).unwrap() {
            Outcome::Missing(m) => assert!(m.reason.contains("usage personnel"), "{}", m.reason),
            Outcome::Included(_) => panic!("estimated traces in a public pack"),
        }
        match include(&spec, out.path(), root.path(), true).unwrap() {
            Outcome::Included(l) => {
                assert!(l.personal_use_only);
                assert_eq!(l.features, Some(1));
                let v: Value = serde_json::from_str(
                    &std::fs::read_to_string(root.path().join(&l.file)).unwrap(),
                )
                .unwrap();
                assert_eq!(v["features"][0]["properties"]["name"], "Le Gy");
            }
            Outcome::Missing(m) => panic!("{}", m.reason),
        }
    }

    /// #117: the pistes the operator announces and OSM lacks, traced from Geotrek or
    /// routed on the IGN sections, are in the personal pack (dotted, uncertain); a
    /// published pack keeps the one whose every licence allows it.
    #[test]
    fn announced_pistes_absent_from_osm_reach_the_pack() {
        let root = tempfile::tempdir().unwrap();
        let out = estimated_out(json!([
            {"type": "Feature", "geometry": {"type": "MultiLineString", "coordinates": [[[6.0, 46.0], [6.01, 46.01]]]},
             "properties": {"name": "Piéton le Val", "method": "geotrek", "certainty": "incertain", "kind": "pedestrian",
                            "named_by": "nordicfrance", "length_km_announced": 6.0, "source_name": "Le Val en raquettes",
                            "source_url": "https://geotrek.example/api/v2/trek/1/",
                            "licences": ["aucune licence, HTML seulement", "aucune licence affichée par l'instance (non vérifiée)"]}},
            {"type": "Feature", "geometry": {"type": "MultiLineString", "coordinates": [[[6.0, 46.0], [6.02, 46.0]]]},
             "properties": {"name": "Liaison le Bas", "method": "estimated_route", "certainty": "incertain", "kind": "liaison",
                            "named_by": "opendata", "route_ends": ["village", "le bas"],
                            "licences": ["Licence Ouverte Etalab 2.0", "Licence Ouverte Etalab 2.0"]}}
        ]));
        let spec = estimated_spec();
        match include(&spec, out.path(), root.path(), true).unwrap() {
            Outcome::Included(l) => {
                assert_eq!(l.features, Some(2));
                let v: Value = serde_json::from_str(
                    &std::fs::read_to_string(root.path().join(&l.file)).unwrap(),
                )
                .unwrap();
                let p = &v["features"][0]["properties"];
                assert_eq!(p["name"], "Piéton le Val");
                assert_eq!(p["method"], "geotrek");
                assert_eq!(p["certainty"], "incertain");
                assert_eq!(p["source_url"], "https://geotrek.example/api/v2/trek/1/");
                assert_eq!(v["features"][1]["properties"]["route_ends"][1], "le bas");
            }
            Outcome::Missing(m) => panic!("{}", m.reason),
        }
        let public = tempfile::tempdir().unwrap();
        match include(&spec, out.path(), public.path(), false).unwrap() {
            Outcome::Included(l) => {
                assert!(!l.personal_use_only);
                assert_eq!(l.features, Some(1));
                assert!(
                    l.sources.iter().all(|s| !s.personal_use_only),
                    "{:?}",
                    l.sources
                );
                let v: Value = serde_json::from_str(
                    &std::fs::read_to_string(public.path().join(&l.file)).unwrap(),
                )
                .unwrap();
                assert_eq!(v["features"][0]["properties"]["name"], "Liaison le Bas");
            }
            Outcome::Missing(m) => panic!("{}", m.reason),
        }
    }

    #[test]
    fn rounds_coordinates() {
        let mut v = json!({"type": "FeatureCollection", "features": [
            {"type": "Feature", "properties": {"x": 1.234_567_89},
             "geometry": {"type": "LineString", "coordinates": [[6.123_456_789, 44.987_654_321], [6.1, 44.2]]}},
            {"type": "Feature", "properties": {}, "geometry": null}
        ]});
        let (n, types) = compact_geojson(&mut v, Path::new("t")).unwrap();
        assert_eq!(n, 2);
        assert_eq!(types, vec!["LineString".to_owned()]);
        assert_eq!(
            v.pointer("/features/0/geometry/coordinates/0/0").unwrap(),
            6.123_457
        );
        assert_eq!(
            v.pointer("/features/0/geometry/coordinates/0/1").unwrap(),
            44.987_654
        );
        // Properties untouched.
        assert_eq!(v.pointer("/features/0/properties/x").unwrap(), 1.234_567_89);
    }

    #[test]
    fn zones_get_local_rule_status() {
        let feature = |id: &str, kind: &str, name: &str| {
            json!({"type": "Feature",
                "properties": {"id": id, "kind": kind, "name": name, "source": "inpn",
                    "url": null, "months": null, "practices": [], "designated": null,
                    "area_ha": 1.0, "clipped": false, "note": null},
                "geometry": {"type": "Polygon", "coordinates": [[[6.0, 44.0], [6.1, 44.0], [6.1, 44.1], [6.0, 44.0]]]}})
        };
        let zones = json!({"type": "FeatureCollection", "features": [
            feature("inpn:FR3800001", "appb", "APPB jamais lu"),
            feature("inpn:FR3800988", "appb", "APPB lu"),
            feature("inpn:FR3300001", "parc_national", "Cœur"),
            feature("natura", "natura2000_sic", "Natura 2000"),
        ]});
        let text = zones.to_string();
        let mut v = zones.clone();
        let date = chrono::NaiveDate::from_ymd_opt(2026, 11, 15).unwrap();
        annotate_zones(&mut v, &text, skimap_zones::rules::RULES_JSON, date).unwrap();
        let p = |i: usize, k: &str| {
            v.pointer(&format!("/features/{i}/properties/{k}"))
                .unwrap()
                .clone()
        };
        assert_eq!(p(0, "local_rules"), "unverified");
        assert_eq!(p(0, "local_act"), "arrete_prefectoral_biotope");
        assert_eq!(p(1, "local_rules"), "read");
        assert_eq!(p(2, "local_rules"), "unverified");
        assert_eq!(p(2, "local_act"), "reglementation_coeur_parc");
        assert!(p(3, "local_rules").is_null());
    }

    #[test]
    fn zones_get_embedded_rules() {
        let zones = json!({"type": "FeatureCollection", "features": [{
            "type": "Feature",
            "properties": {"id": "country:fr", "kind": "country", "name": "France", "source": "geo-api",
                            "url": null, "months": null, "practices": [], "designated": null,
                            "area_ha": 1.0, "clipped": true, "note": null},
            "geometry": {"type": "Polygon", "coordinates": [[[6.0, 44.0], [7.0, 44.0], [7.0, 45.0], [6.0, 44.0]]]}
        }]});
        let text = zones.to_string();
        let mut v = zones.clone();
        let date = chrono::NaiveDate::from_ymd_opt(2026, 11, 15).unwrap();
        annotate_zones(&mut v, &text, skimap_zones::rules::RULES_JSON, date).unwrap();
        assert_eq!(
            v.pointer("/features/0/properties/bivouac").unwrap(),
            "consent_required"
        );
        assert_eq!(
            v.pointer("/features/0/properties/bivouac_rules/0").unwrap(),
            "fr-r111-32-bivouac-consent"
        );
        assert_eq!(
            v.pointer("/features/0/properties/access").unwrap(),
            "unknown"
        );
        let rules = v
            .pointer("/features/0/properties/rules")
            .unwrap()
            .as_array()
            .unwrap();
        assert!(
            rules
                .iter()
                .any(|r| r["id"] == "fr-r111-32-bivouac-consent")
        );
        // The country has no act of its own (national rules only).
        assert!(
            v.pointer("/features/0/properties/local_rules")
                .unwrap()
                .is_null()
        );
        let t = v
            .pointer("/features/0/properties/rules_text")
            .unwrap()
            .as_str()
            .unwrap();
        assert!(t.contains("Bivouac : avec l'accord du propriétaire (toute l'année)"));
    }
}
