//! Names of a site read from its built layers, with where each one comes from (workstation
//! side, VO3): nordic pistes (`pistes`, OpenSnowMap / OSM), huts and shelters (`refuges`:
//! refuges.info, OSM), protected zones and the end dates of their seasonal periods
//! (`zones`: INPN, Swiss federal hunting reserves, Biodiv'sports), names read on the
//! official piste plan (`plan_pistes`, P22).
//!
//! A name is **publishable** when one of the sources it was read from may go into a
//! published pack; it is **personal** when every source it comes from is personal use
//! only (Biodiv'sports, the piste plans: docs/licences-donnees.md), or when its source
//! cannot be told. The voice layer marks the clips of personal names
//! ([`crate::pack::PersonalClips`], `personal.json`); the pack leaves them out without
//! `personal_sources`.

use std::collections::BTreeSet;
use std::path::Path;

use serde_json::Value;
use skimap_common::layout::DataLayout;
use skimap_common::meta::{LayerMeta, SourceRef};
use skimap_common::site::Site;

use crate::clips::DayMonth;
use crate::error::{Error, Result};
use crate::manifest::PackNames;
use crate::names::{Colour, PisteName, name_clip_id};

/// Sources whose terms do not allow a published pack, recognised by name (lower case)
/// even when a metadata file does not mark them `personal_use_only`: the same list as
/// `skimap_pack::build::layers::UNAUTHORISED_SOURCES` (a test of the pack checks it).
pub const PERSONAL_SOURCES: [&str; 7] = [
    "biodiv'sports",
    "clpa",
    "enquête permanente sur les avalanches",
    "enj snowmap",
    "nordic france",
    "page info damage",
    "open-meteo",
];

/// Whether names read from this source may not go into a published pack: its licence
/// family is personal (`skimap_common::licence::of_source`: marked `personal_use_only`,
/// no reuse licence known, a licence still to confirm, #171), or it is one of
/// [`PERSONAL_SOURCES`].
pub fn personal(s: &SourceRef) -> bool {
    let name = s.name.to_lowercase();
    !skimap_common::licence::of_source(s).public()
        || PERSONAL_SOURCES.iter().any(|p| name.contains(p))
}

/// Why the names of the OSM huts are personal: they only complete refuges.info for one
/// feature type, which no licence allows in a published pack (L3 rule 1,
/// docs/licences/l3-combinaison.md, #171).
pub const OSM_HUTS_PERSONAL: &str = "abris OSM : ils complètent refuges.info (CC BY-SA) pour un même type d'objet, ce qu'aucune licence ne permet dans un pack publié (règle 1 de docs/licences/l3-combinaison.md) : noms à usage personnel";

/// Names of a site split by provenance, with their sources.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SiteNames {
    /// Names read from at least one publishable source (and the site name, the number of
    /// pistes whose status is announced).
    pub public: PackNames,
    /// Names read only from personal-use sources (not in `public`); `site` empty,
    /// `status_pistes` 0.
    pub personal: PackNames,
    /// Sources of the public names.
    pub public_sources: Vec<SourceRef>,
    /// Sources of the personal names (marked `personal_use_only`).
    pub personal_sources: Vec<SourceRef>,
}

impl SiteNames {
    /// Splits names of unknown provenance: a piste, place or date is public when `known`
    /// (read from the layers) has it public, else personal. Sources are split by
    /// [`personal`].
    pub fn classify(names: &PackNames, sources: &[SourceRef], known: &SiteNames) -> Self {
        let pistes: BTreeSet<String> = known.public.pistes.iter().map(|p| p.clip_id()).collect();
        let places: BTreeSet<String> = known
            .public
            .places
            .iter()
            .map(|p| name_clip_id(p))
            .collect();
        let dates: BTreeSet<DayMonth> = known.public.dates.iter().copied().collect();
        let mut out = Self::unclassified(names, sources);
        for n in [&mut out.public, &mut out.personal] {
            n.pistes.clear();
            n.places.clear();
            n.dates.clear();
        }
        for p in &names.pistes {
            if pistes.contains(&p.clip_id()) {
                out.public.pistes.push(p.clone());
            } else {
                out.personal.pistes.push(p.clone());
            }
        }
        for p in &names.places {
            if places.contains(&name_clip_id(p)) {
                out.public.places.push(p.clone());
            } else {
                out.personal.places.push(p.clone());
            }
        }
        for d in &names.dates {
            if dates.contains(d) {
                out.public.dates.push(*d);
            } else {
                out.personal.dates.push(*d);
            }
        }
        out
    }

    /// Names whose provenance cannot be told: all public if no source is personal use
    /// only, else all personal (a caller that does not say where each name comes from
    /// cannot publish any).
    pub fn unclassified(names: &PackNames, sources: &[SourceRef]) -> Self {
        let (personal_sources, public_sources): (Vec<SourceRef>, Vec<SourceRef>) =
            sources.iter().cloned().partition(personal);
        let mut out = Self {
            public: names.clone(),
            personal: PackNames::default(),
            public_sources,
            personal_sources: personal_sources.into_iter().map(mark_personal).collect(),
        };
        if !out.personal_sources.is_empty() {
            out.personal.pistes = std::mem::take(&mut out.public.pistes);
            out.personal.places = std::mem::take(&mut out.public.places);
            out.personal.dates = std::mem::take(&mut out.public.dates);
        }
        out
    }

    /// Every name (public and personal), as one set.
    pub fn all(&self) -> PackNames {
        let mut n = self.public.clone();
        n.pistes.extend(self.personal.pistes.iter().cloned());
        n.places.extend(self.personal.places.iter().cloned());
        n.dates.extend(self.personal.dates.iter().copied());
        n.pistes.sort();
        n.places.sort();
        n.dates.sort();
        n
    }
}

fn mark_personal(mut s: SourceRef) -> SourceRef {
    s.personal_use_only = true;
    s
}

fn read_json(path: &Path, site: &Site, layer: &str) -> Result<Value> {
    let b = std::fs::read(path).map_err(|e| {
        Error::Config(format!(
            "{} absent ({e}) : construire d'abord la couche (`skimap {layer} {}`)",
            path.display(),
            site.id
        ))
    })?;
    serde_json::from_slice(&b).map_err(|source| Error::Json {
        path: path.to_owned(),
        source,
    })
}

fn features(v: &Value) -> impl Iterator<Item = &Value> {
    v["features"].as_array().into_iter().flatten()
}

fn trimmed(v: &Value) -> Option<String> {
    v.as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

/// Colour of a piste from the `pistes` layer: the classified relation colour (`green`,
/// `blue`, `red`, `black`), else the difficulty (`novice`... `expert`).
fn colour(classified: Option<&str>, difficulty: Option<&str>) -> Option<Colour> {
    Colour::from_osm(classified.filter(|c| *c != "other"), difficulty)
}

/// Nordic piste names of the `pistes` layer: names of the nordic relations of the ways in
/// the site box (`in_box`), with the relation colour, and the ways' own names, with their
/// `piste:difficulty`. Outside ways (kept to close the network) are left out.
pub fn piste_names(pistes: &Value, meta: &LayerMeta) -> BTreeSet<PisteName> {
    let nordic_relation = |id: &Value| {
        let k = id.as_i64().map(|i| i.to_string()).unwrap_or_default();
        meta.stats["relations"][k.as_str()]["uses"]
            .as_array()
            .is_some_and(|u| u.iter().any(|x| x == "nordic"))
    };
    let mut out = BTreeSet::new();
    for f in features(pistes) {
        let p = &f["properties"];
        let nordic = p["uses"]
            .as_array()
            .is_some_and(|u| u.iter().any(|x| x == "nordic"));
        if !nordic || p["in_box"] == Value::Bool(false) {
            continue;
        }
        if let Some(n) = trimmed(&p["way_name"]) {
            out.insert(PisteName::new(
                n,
                colour(None, p["way_difficulty"].as_str()),
            ));
        }
        for r in p["relations"].as_array().into_iter().flatten() {
            if let Some(n) = trimmed(&r["name"])
                && nordic_relation(&r["id"])
            {
                out.insert(PisteName::new(
                    n,
                    colour(r["colour"].as_str(), r["difficulty"].as_str()),
                ));
            }
        }
    }
    out
}

/// Piste names read on the official plan (`plan_pistes/plan_pistes.geojson`, P22), with
/// the plan colour: what the pack names a way OSM leaves without a name, with
/// `personal_sources`.
pub fn plan_piste_names(plan_pistes: &Value) -> BTreeSet<PisteName> {
    features(plan_pistes)
        .filter_map(|f| {
            let p = &f["properties"];
            let n = trimmed(&p["name"])?;
            Some(PisteName::new(n, colour(p["colour"].as_str(), None)))
        })
        .collect()
}

/// Hut and shelter names of the `refuges` layer: refuges.info huts, staffed refuges and
/// gîtes (publishable, CC BY-SA); OSM `alpine_hut`, `wilderness_hut` and `shelter` (not
/// bus shelters) that refuges.info does not name (personal: [`OSM_HUTS_PERSONAL`]).
pub fn place_names(refuges_info: &Value, osm: &Value) -> (BTreeSet<String>, BTreeSet<String>) {
    let mut out = BTreeSet::new();
    for f in features(refuges_info) {
        let p = &f["properties"];
        if matches!(
            p["type_label"].as_str(),
            Some("cabane non gardée" | "refuge gardé" | "gîte d'étape")
        ) && let Some(n) = trimmed(&p["name"])
        {
            out.insert(n);
        }
    }
    let known: BTreeSet<String> = out.iter().map(|n| name_clip_id(n)).collect();
    let mut osm_names = BTreeSet::new();
    for f in features(osm) {
        let p = &f["properties"];
        if matches!(
            p["kind"].as_str(),
            Some("alpine_hut" | "wilderness_hut" | "shelter")
        ) && let Some(n) = trimmed(&p["tags"]["name"])
            && !known.contains(&name_clip_id(&n))
        {
            osm_names.insert(n);
        }
    }
    (out, osm_names)
}

/// Last days of the months that end a protection period (months 1..=12, wrapping).
pub fn period_ends(months: &[u8]) -> Vec<DayMonth> {
    const LAST: [u8; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let on = |m: u8| months.contains(&m);
    if months.is_empty() || (1..=12).all(on) {
        return Vec::new();
    }
    (1..=12u8)
        .filter(|&m| on(m) && !on(m % 12 + 1))
        .map(|m| DayMonth {
            day: LAST[usize::from(m - 1)],
            month: m,
        })
        .collect()
}

/// Whether a zone of the `zones` layer is announced as a protected zone: nature
/// protection (INPN, Biodiv'sports, Swiss federal hunting reserves), not heritage
/// servitudes (GPU monuments, listed sites) nor administrative areas.
fn protected(p: &Value) -> bool {
    matches!(p["source"].as_str(), Some("inpn" | "biodiv-sports"))
        || p["kind"].as_str() == Some("district_franc")
}

/// The source of the `zones` metadata a zone was read from (its `source` property).
pub fn zone_source<'a>(key: &str, sources: &'a [SourceRef]) -> Option<&'a SourceRef> {
    sources.iter().find(|s| match key {
        "inpn" => s.name.starts_with("INPN"),
        "biodiv-sports" => {
            s.url.contains("biodiv-sports") || s.name.to_lowercase().contains("biodiv")
        }
        "geo-admin" => s.url.contains("geo.admin.ch"),
        _ => false,
    })
}

/// Protected zone names and the end dates of their seasonal periods.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ZoneNames {
    /// Names of zones from a publishable source.
    pub public: BTreeSet<String>,
    /// Their end dates.
    pub public_dates: BTreeSet<DayMonth>,
    /// Names of zones from a personal-use source, or an unknown one.
    pub personal: BTreeSet<String>,
    /// Their end dates.
    pub personal_dates: BTreeSet<DayMonth>,
}

/// Protected zone names and end dates of the `zones` layer, split by the source of each
/// zone in the layer metadata (a zone whose source is not found is personal).
pub fn zone_names(zones: &Value, sources: &[SourceRef]) -> ZoneNames {
    let mut z = ZoneNames::default();
    for f in features(zones) {
        let p = &f["properties"];
        if !protected(p) {
            continue;
        }
        let public = zone_source(p["source"].as_str().unwrap_or_default(), sources)
            .is_some_and(|s| !personal(s));
        let months: Vec<u8> = p["months"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|m| m.as_u64().and_then(|m| u8::try_from(m).ok()))
            .collect();
        let (names, dates) = if public {
            (&mut z.public, &mut z.public_dates)
        } else {
            (&mut z.personal, &mut z.personal_dates)
        };
        if let Some(n) = trimmed(&p["name"]) {
            names.insert(n);
        }
        dates.extend(period_ends(&months));
    }
    z
}

fn noted(mut s: SourceRef) -> SourceRef {
    let used = s.note.take();
    s.note = Some(match used {
        Some(n) => format!("noms lus pour les clips de la voix ; {n}"),
        None => "noms lus pour les clips de la voix".into(),
    });
    s
}

/// Names of `site` from its `pistes`, `refuges` and `zones` layers (and `plan_pistes`
/// when built), split by provenance, with the sources of the files read (from their
/// `<layer>.meta.json`).
pub fn site_names(layout: &DataLayout, site: &Site) -> Result<SiteNames> {
    let dir = |l: &str| layout.layer_dir(site, l);
    let meta = |l: &str| -> Result<LayerMeta> {
        let p = dir(l)?.join(LayerMeta::file_name(l));
        LayerMeta::read(&p).map_err(|e| {
            Error::Config(format!(
                "{} ({e}) : construire d'abord la couche (`skimap {l} {}`)",
                p.display(),
                site.id
            ))
        })
    };
    let pistes_meta = meta("pistes")?;
    let pistes = piste_names(
        &read_json(&dir("pistes")?.join("pistes.geojson"), site, "pistes")?,
        &pistes_meta,
    );
    let refuges_meta = meta("refuges")?;
    let (places, osm_places) = place_names(
        &read_json(
            &dir("refuges")?.join("refuges_info.geojson"),
            site,
            "refuges",
        )?,
        &read_json(&dir("refuges")?.join("osm.geojson"), site, "refuges")?,
    );
    let zones_meta = meta("zones")?;
    let zones = zone_names(
        &read_json(&dir("zones")?.join("zones.geojson"), site, "zones")?,
        &zones_meta.sources,
    );
    let plan_dir = dir("plan_pistes")?;
    let plan = if plan_dir.join("plan_pistes.geojson").is_file() {
        let m = meta("plan_pistes")?;
        let names = plan_piste_names(&read_json(
            &plan_dir.join("plan_pistes.geojson"),
            site,
            "plan_pistes",
        )?);
        Some((names, m.sources))
    } else {
        None
    };

    let mut public_sources = Vec::new();
    let mut personal_sources = Vec::new();
    let mut add = |s: SourceRef| {
        if personal(&s) {
            personal_sources.push(mark_personal(noted(s)));
        } else {
            public_sources.push(noted(s));
        }
    };
    // The piste layer: one personal source would make every piste name personal.
    let pistes_personal = pistes_meta.sources.iter().any(personal);
    pistes_meta.sources.into_iter().for_each(&mut add);
    // Names come from refuges.info (publishable) and OSM: the OSM huts are read from the
    // local Geofabrik extracts (X5); layers built before X5 name the Overpass endpoint.
    // The OSM hut names are personal (L3 rule 1), so is their source here; without any,
    // the source gives no name said.
    for mut s in refuges_meta.sources {
        if s.url.contains("refuges.info/api/bbox") {
            add(s);
        } else if (s.url.contains("download.geofabrik.de") || s.url.contains("overpass"))
            && !osm_places.is_empty()
        {
            s.note = Some(OSM_HUTS_PERSONAL.to_owned());
            s.personal_use_only = true;
            add(s);
        }
    }
    for key in ["inpn", "biodiv-sports", "geo-admin"] {
        if let Some(s) = zone_source(key, &zones_meta.sources) {
            add(s.clone());
        }
    }
    let status_pistes = pistes.len() as u32;
    let (mut pub_pistes, mut per_pistes) = if pistes_personal {
        (BTreeSet::new(), pistes)
    } else {
        (pistes, BTreeSet::new())
    };
    if let Some((names, sources)) = plan {
        // Names read on the plan: personal when the plan is (its OSM sources only give
        // the geometry).
        match sources.iter().find(|s| personal(s)) {
            Some(s) => {
                add(s.clone());
                per_pistes.extend(names);
            }
            None => {
                if let Some(s) = sources.first() {
                    add(s.clone());
                }
                pub_pistes.extend(names);
            }
        }
    }
    let pub_ids: BTreeSet<String> = pub_pistes.iter().map(PisteName::clip_id).collect();
    per_pistes.retain(|p| !pub_ids.contains(&p.clip_id()));

    let mut pub_places = places;
    pub_places.extend(zones.public);
    let place_ids: BTreeSet<String> = pub_places.iter().map(|p| name_clip_id(p)).collect();
    let per_places: BTreeSet<String> = zones
        .personal
        .into_iter()
        .chain(osm_places)
        .filter(|p| !place_ids.contains(&name_clip_id(p)))
        .collect();
    let per_dates: BTreeSet<DayMonth> = zones
        .personal_dates
        .difference(&zones.public_dates)
        .copied()
        .collect();
    // A date said only for personal zones: no public announcement needs it.
    Ok(SiteNames {
        public: PackNames {
            site: site.name.to_owned(),
            pistes: pub_pistes.into_iter().collect(),
            places: pub_places.into_iter().collect(),
            dates: zones.public_dates.into_iter().collect(),
            status_pistes,
        },
        personal: PackNames {
            site: String::new(),
            pistes: per_pistes.into_iter().collect(),
            places: per_places.into_iter().collect(),
            dates: per_dates.into_iter().collect(),
            status_pistes: 0,
        },
        public_sources,
        personal_sources,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn period_end_dates() {
        let d = |day, month| DayMonth { day, month };
        assert_eq!(period_ends(&[12, 1, 2, 3, 4, 5, 6]), vec![d(30, 6)]);
        assert_eq!(period_ends(&[11, 12]), vec![d(31, 12)]);
        assert_eq!(period_ends(&[1, 2, 5]), vec![d(28, 2), d(31, 5)]);
        assert!(period_ends(&[]).is_empty());
        assert!(period_ends(&(1..=12).collect::<Vec<_>>()).is_empty());
    }

    #[test]
    fn names_from_layers() {
        let mut meta = LayerMeta::new("pistes", "x");
        meta.stats = json!({"relations": {
            "1": {"uses": ["nordic"]},
            "2": {"uses": ["hike"]},
        }});
        let pistes = json!({"features": [
            {"properties": {"uses": ["nordic"], "in_box": true, "way_name": "Route du Col",
                "way_difficulty": "easy", "relations": [
                {"id": 1, "name": " Greffier ", "colour": "black", "difficulty": "advanced"},
                {"id": 2, "name": "Raquettes", "colour": null, "difficulty": null}]}},
            {"properties": {"uses": ["nordic"], "in_box": true, "way_name": null,
                "relations": [{"id": 1, "name": "Le Gy", "colour": "other", "difficulty": "novice"}]}},
            {"properties": {"uses": ["nordic"], "in_box": false, "way_name": "Dehors", "relations": []}},
            {"properties": {"uses": ["hike"], "in_box": true, "way_name": "Sentier", "relations": []}},
        ]});
        let got: Vec<(String, Option<Colour>)> = piste_names(&pistes, &meta)
            .into_iter()
            .map(|p| (p.name, p.colour))
            .collect();
        assert_eq!(
            got,
            vec![
                ("Greffier".into(), Some(Colour::Black)),
                ("Le Gy".into(), Some(Colour::Green)),
                ("Route du Col".into(), Some(Colour::Blue)),
            ]
        );
        let ri = json!({"features": [
            {"properties": {"type_label": "cabane non gardée", "name": "Refuge des Fonts"}},
            {"properties": {"type_label": "point d'eau", "name": "Source"}},
        ]});
        let osm = json!({"features": [
            {"properties": {"kind": "shelter", "tags": {"name": "Abri"}}},
            {"properties": {"kind": "bus_shelter", "tags": {"name": "Arrêt"}}},
        ]});
        // refuges.info names are publishable; OSM huts only complete them (L3 rule 1,
        // docs/licences/l3-combinaison.md): their names are personal (#171).
        let (public, personal) = place_names(&ri, &osm);
        assert_eq!(
            public.into_iter().collect::<Vec<_>>(),
            vec!["Refuge des Fonts".to_owned()]
        );
        assert_eq!(
            personal.into_iter().collect::<Vec<_>>(),
            vec!["Abri".to_owned()]
        );
    }

    /// A source whose licence is still to be checked is personal (#171), whatever its
    /// `personal_use_only` says: the decision of `skimap_common::licence`.
    #[test]
    fn unverified_licence_is_personal() {
        let s = |l: &str| SourceRef::new("INPN", "https://x", l, "PatriNat");
        assert!(personal(&s(
            "« other-open » selon data.gouv.fr, à confirmer (F5, #11)"
        )));
        assert!(personal(&s("non vérifiée (F5, #11)")));
        assert!(!personal(&s("Licence Ouverte 2.0")));
        assert!(!personal(&s("CC BY-SA 2.0")));
    }

    fn zone_sources() -> Vec<SourceRef> {
        let mut bsp = SourceRef::new(
            "Biodiv'sports",
            "https://biodiv-sports.fr/api/v2/sensitivearea/",
            "aucune licence trouvée : usage personnel",
            "Biodiv'sports",
        );
        bsp.personal_use_only = true;
        vec![
            SourceRef::new(
                "INPN espaces protégés (PatriNat)",
                "https://data.geopf.fr/wfs/ows",
                "Licence Ouverte 2.0",
                "PatriNat",
            ),
            bsp,
            SourceRef::new(
                "geo.admin.ch (OFEV districts francs)",
                "https://api3.geo.admin.ch/rest/services/api/MapServer/identify",
                "terms_by",
                "OFEV",
            ),
        ]
    }

    #[test]
    fn zones_split_by_source() {
        let zones = json!({"features": [
            {"properties": {"source": "biodiv-sports", "kind": "biodiv_sports", "name": "Aigle royal", "months": [12, 1, 2, 3, 4]}},
            {"properties": {"source": "biodiv-sports", "kind": "biodiv_sports", "name": "Forêt Du Paradis", "months": [1, 2, 3, 4, 5, 6]}},
            {"properties": {"source": "inpn", "kind": "appb", "name": "Forêt Du Paradis", "months": null}},
            {"properties": {"source": "inpn", "kind": "natura2000_sic", "name": "Clarée", "months": [1, 2, 3, 4, 5, 6]}},
            {"properties": {"source": "gpu", "kind": "monument_historique", "name": "Eglise", "months": null}},
            {"properties": {"source": "geo-admin", "kind": "district_franc", "name": "District franc", "months": null}},
            {"properties": {"source": "inconnue", "kind": "district_franc", "name": "Réserve sans source", "months": null}},
            {"properties": {"source": "geo-api", "kind": "commune", "name": "Cervières", "months": null}},
        ]});
        let z = zone_names(&zones, &zone_sources());
        let v = |s: &BTreeSet<String>| s.iter().cloned().collect::<Vec<_>>();
        assert_eq!(
            v(&z.public),
            ["Clarée", "District franc", "Forêt Du Paradis"]
        );
        assert_eq!(
            v(&z.personal),
            ["Aigle royal", "Forêt Du Paradis", "Réserve sans source"]
        );
        assert_eq!(
            z.public_dates.into_iter().collect::<Vec<_>>(),
            [DayMonth { day: 30, month: 6 }]
        );
        assert_eq!(
            z.personal_dates.into_iter().collect::<Vec<_>>(),
            [
                DayMonth { day: 30, month: 4 },
                DayMonth { day: 30, month: 6 }
            ]
        );
        // Without the marking of older metadata, the name list still recognises it.
        let mut older = zone_sources();
        older[1].personal_use_only = false;
        assert_eq!(zone_names(&zones, &older).personal, z.personal);
    }

    fn write(path: &Path, v: &Value) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, serde_json::to_vec(v).unwrap()).unwrap();
    }

    fn layers(root: &Path, with_plan: bool) -> DataLayout {
        let out = root.join("out/cervieres");
        let mut pm = LayerMeta::new("pistes", "cervieres");
        pm.sources = vec![SourceRef::new(
            "OpenSnowMap planet_pistes.osm.gz",
            "https://www.opensnowmap.org/download/planet_pistes.osm.gz",
            "ODbL 1.0",
            "© OSM",
        )];
        pm.stats = json!({"relations": {"1": {"uses": ["nordic"]}}});
        pm.write(&out.join("pistes")).unwrap();
        write(
            &out.join("pistes/pistes.geojson"),
            &json!({"features": [{"properties": {"uses": ["nordic"], "in_box": true, "way_name": null,
                "relations": [{"id": 1, "name": "Le Gibera", "colour": "green", "difficulty": null}]}}]}),
        );
        let mut rm = LayerMeta::new("refuges", "cervieres");
        rm.sources = vec![
            SourceRef::new(
                "refuges.info /api/bbox",
                "https://www.refuges.info/api/bbox?bbox=1",
                "CC BY-SA 2.0",
                "refuges.info",
            ),
            SourceRef::new(
                "OpenStreetMap, extrait Geofabrik rhone-alpes",
                "https://download.geofabrik.de/europe/france/rhone-alpes-latest.osm.pbf",
                "ODbL 1.0",
                "© OSM",
            ),
            SourceRef::new(
                "Camptocamp /waypoints",
                "https://api.camptocamp.org/waypoints",
                "CC BY-SA 3.0",
                "Camptocamp",
            ),
        ];
        rm.write(&out.join("refuges")).unwrap();
        write(
            &out.join("refuges/refuges_info.geojson"),
            &json!({"features": [{"properties": {"type_label": "refuge gardé", "name": "Refuge des Fonts"}}]}),
        );
        write(
            &out.join("refuges/osm.geojson"),
            &json!({"features": [
                {"properties": {"kind": "alpine_hut", "tags": {"name": "Refuge des Fonts"}}},
                {"properties": {"kind": "shelter", "tags": {"name": "Abri du Laus"}}}]}),
        );
        let mut zm = LayerMeta::new("zones", "cervieres");
        zm.sources = zone_sources();
        zm.write(&out.join("zones")).unwrap();
        write(
            &out.join("zones/zones.geojson"),
            &json!({"features": [
                {"properties": {"source": "biodiv-sports", "kind": "biodiv_sports", "name": "Aigle royal", "months": [12, 1, 2, 3, 4]}},
                {"properties": {"source": "inpn", "kind": "site_classe", "name": "Casse Déserte", "months": null}}]}),
        );
        if with_plan {
            let mut plm = LayerMeta::new("plan_pistes", "cervieres");
            let mut plan = SourceRef::new(
                "Plan des pistes - Cervières (dépliant « L'Izoard, hiver »)",
                "https://example.org/plan.pdf",
                "tous droits réservés",
                "Office de tourisme",
            );
            plan.personal_use_only = true;
            plm.sources = vec![
                plan,
                SourceRef::new(
                    "OpenSnowMap planet_pistes.osm.gz",
                    "https://www.opensnowmap.org",
                    "ODbL 1.0",
                    "© OSM",
                ),
            ];
            plm.write(&out.join("plan_pistes")).unwrap();
            write(
                &out.join("plan_pistes/plan_pistes.geojson"),
                &json!({"features": [
                    {"properties": {"name": "Le Gibera", "colour": "green"}},
                    {"properties": {"name": "La Pinatelle", "colour": "black"}}]}),
            );
        }
        DataLayout::new(root)
    }

    #[test]
    fn site_names_split_publishable_and_personal() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = layers(tmp.path(), true);
        let s = site_names(
            &layout,
            skimap_common::site::site("cervieres").expect("evaluation site"),
        )
        .unwrap();
        let pistes = |n: &PackNames| n.pistes.iter().map(|p| p.clip_id()).collect::<Vec<_>>();
        assert_eq!(pistes(&s.public), ["piste.le-gibera.verte"]);
        // « Le Gibera » is on the plan too, but OSM publishes it: public only.
        assert_eq!(pistes(&s.personal), ["piste.la-pinatelle.noire"]);
        assert_eq!(s.public.places, ["Casse Déserte", "Refuge des Fonts"]);
        // The OSM shelter refuges.info lacks: personal (L3 rule 1, #171); the OSM hut
        // refuges.info names is said from refuges.info.
        assert_eq!(s.personal.places, ["Abri du Laus", "Aigle royal"]);
        assert_eq!(s.personal.dates, [DayMonth { day: 30, month: 4 }]);
        assert!(s.public.dates.is_empty());
        assert_eq!(s.public.site, "Cervières");
        assert_eq!(s.public.status_pistes, 1);
        assert!(s.public_sources.iter().all(|x| !personal(x)));
        assert!(s.personal_sources.iter().all(|x| x.personal_use_only));
        let names: Vec<&str> = s.personal_sources.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names.len(), 3, "{names:?}");
        assert!(names.iter().any(|n| n.starts_with("Biodiv'sports")));
        assert!(names.iter().any(|n| n.starts_with("Plan des pistes")));
        // Camptocamp gives no name; the OSM extract gives personal ones only.
        assert!(
            s.public_sources
                .iter()
                .all(|x| !x.name.starts_with("Camptocamp") && !x.url.contains("geofabrik"))
        );
        assert!(
            s.personal_sources
                .iter()
                .any(|x| x.url.contains("download.geofabrik.de"))
        );
        assert_eq!(s.all().places.len(), 4);

        // The same names given without their provenance: classified against the layers.
        let given = s.all();
        for sources in [&[][..], &s.personal_sources[..]] {
            let c = SiteNames::classify(&given, sources, &s);
            assert_eq!(c.public, s.public);
            assert_eq!(c.personal, s.personal);
        }
        // Unknown provenance, a personal source among those given: nothing is public.
        let u = SiteNames::unclassified(&given, &s.personal_sources);
        assert!(u.public.pistes.is_empty() && u.public.places.is_empty());
        assert_eq!(u.personal.places.len(), 4);
        let u = SiteNames::unclassified(&given, &s.public_sources);
        assert_eq!(u.public, given);

        // Without the plan layer: no plan name.
        let tmp2 = tempfile::tempdir().unwrap();
        let s2 = site_names(
            &layers(tmp2.path(), false),
            skimap_common::site::site("cervieres").expect("evaluation site"),
        )
        .unwrap();
        assert!(s2.personal.pistes.is_empty());
        assert_eq!(s2.personal_sources.len(), 2);
    }
}
