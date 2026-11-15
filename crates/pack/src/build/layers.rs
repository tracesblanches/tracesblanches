//! Vector (GeoJSON) and data (JSON) layers of the pack, taken from `data/out/<site>/`.
//!
//! GeoJSON is rewritten compact with coordinates rounded to 1e-6° (≈ 0.1 m); properties
//! are kept as written by the producing crate. The zones layer gains two derived
//! properties: `rules` (rules of `rules.json` applying to the zone) and `rules_text`.

use std::borrow::Cow;
use std::collections::BTreeSet;
use std::path::Path;

use serde_json::{Value, json};
use skimap_common::meta::LayerMeta;
use skimap_zones::{Effect, RuleSet, Topic, ZoneLayer};

use crate::build::{copy_file, write_bytes};
use crate::error::{Error, Result};
use crate::manifest::{Layer, LayerKind, Missing, Source};

/// How a layer file is copied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transform {
    /// Byte copy (JSON data).
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
    }
}

/// Vector and data layers, in display order (drawn bottom to top by the viewer).
pub fn catalog() -> Vec<Spec> {
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
                "Champs simples dérivés des règles de la zone et de celles des territoires qui la contiennent (skimap_zones::fields) : `bivouac` (forbidden, forbidden_in_places, tolerated, consent_required, allowed, unknown), `access` (forbidden, forbidden_in_places, routes_only, routes_only_in_places, avoid_recommended, allowed, unknown), leurs périodes (`bivouac_period`, `access_period`, null = toute l'année) et les règles qui décident (`bivouac_rules`, `access_rules`).",
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
        // SRN areas and itineraries outside groomed domains touching the site box
        // (G5, written by `skimap build-all` in `srn/`): the content of an SRN site.
        vector(
            "srn-areas",
            "Domaines de jeu et traversées SRN (hors stations)",
            "srn",
            "srn_areas.geojson",
            "srn.meta.json",
            true,
        ),
        vector(
            "srn-routes",
            "Itinéraires SRN (hors stations)",
            "srn",
            "srn_routes.geojson",
            "srn.meta.json",
            true,
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
        vector(
            "refuges-osm",
            "Refuges et abris (OpenStreetMap)",
            "refuges",
            "osm.geojson",
            "refuges.meta.json",
            true,
        ),
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
        data(
            "refuges-links",
            "Liens entre les trois sources de refuges",
            "refuges",
            "links.json",
            "refuges.meta.json",
        ),
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
        copy_file(&dir.join(name), &root.join(&rel))?;
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
    let meta = match spec.meta {
        Some(m) => read_meta(&dir.join(m))?,
        None => None,
    };
    let own = meta.as_ref().is_some_and(|m| restricted(&m.meta.sources));
    let derived = personal_inputs(out, spec.depends)?;
    let personal = own || !derived.is_empty();
    if personal && !personal_sources {
        return Ok(Outcome::Missing(Missing {
            id: spec.id.to_string(),
            reason: "source à usage personnel seulement : option personal_sources désactivée"
                .to_owned(),
        }));
    }
    let ext = ext_of(&spec.file);
    let (file, format) = match spec.transform {
        Transform::Copy => (format!("layers/{}.{ext}", spec.id), ext.clone()),
        Transform::Gzip => (
            format!("layers/{}.{ext}.gz", spec.id),
            format!("{ext}+gzip"),
        ),
        Transform::GeoJson | Transform::Zones | Transform::Pistes => {
            (format!("layers/{}.geojson", spec.id), "geojson".to_owned())
        }
    };
    let dst = root.join(&file);
    let mut features = None;
    let mut geometry_types = Vec::new();
    let mut joined: Option<crate::build::pistes::PistesReport> = None;
    let bytes = match spec.transform {
        Transform::Copy => copy_file(&src, &dst)?,
        Transform::Gzip => gzip_file(&src, &dst)?,
        Transform::GeoJson | Transform::Zones | Transform::Pistes => {
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
            let (n, types) = compact_geojson(&mut v, &src)?;
            features = Some(n);
            geometry_types = types;
            let out = serde_json::to_vec(&v).map_err(|e| Error::json(&file, e))?;
            write_bytes(&dst, &out)?
        }
    };
    let meta_rel = match spec.meta {
        Some(m) => copy_meta(&dir, m, root)?,
        None => None,
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
    if !derived.is_empty() {
        l.personal_use_only = true;
        l.notes.push(format!(
            "Calculée à partir de couches à usage personnel ({}) : usage personnel.",
            derived.join(", ")
        ));
    }
    Ok(Outcome::Included(Box::new(l)))
}

/// Sources whose terms do not allow a published pack until their owner agrees in
/// writing (docs/licences-donnees.md, « Usage personnel seulement, tant qu'il n'y a pas
/// d'accord écrit »), recognised by name (lower case) even when an older metadata file
/// does not mark them `personal_use_only`.
pub const UNAUTHORISED_SOURCES: [&str; 7] = [
    "biodiv'sports",
    "clpa",
    "enquête permanente sur les avalanches",
    "enj snowmap",
    "nordic france",
    "mairie de cervières",
    "open-meteo",
];

/// Whether a source may not go into a published pack: marked `personal_use_only`, or one
/// of [`UNAUTHORISED_SOURCES`].
pub fn unauthorised(s: &Source) -> bool {
    let name = s.name.to_lowercase();
    s.personal_use_only || UNAUTHORISED_SOURCES.iter().any(|u| name.contains(u))
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
/// `access_rules`.
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
        let t = v
            .pointer("/features/0/properties/rules_text")
            .unwrap()
            .as_str()
            .unwrap();
        assert!(t.contains("Bivouac : avec l'accord du propriétaire (toute l'année)"));
    }
}
