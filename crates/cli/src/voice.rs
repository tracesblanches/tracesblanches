//! `skimap voice`: voice layer of a site (VO3) for the names of its built layers: nordic
//! pistes (`pistes`), huts and shelters (`refuges`), protected zones and the end dates of
//! their seasonal periods (`zones`).

use std::collections::BTreeSet;
use std::path::Path;

use anyhow::{Context, Result};
use serde_json::Value;
use skimap_common::layout::DataLayout;
use skimap_common::meta::{LayerMeta, SourceRef};
use skimap_common::site::Site;
use skimap_voice::PackNames;
use skimap_voice::clips::DayMonth;
use skimap_voice::names::{Colour, PisteName};

fn read_json(path: &Path, site: &Site, layer: &str) -> Result<Value> {
    let b = std::fs::read(path).with_context(|| {
        format!(
            "{} absent : construire d'abord la couche (`skimap {layer} {}`)",
            path.display(),
            site.id
        )
    })?;
    serde_json::from_slice(&b).with_context(|| path.display().to_string())
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

/// Hut and shelter names of the `refuges` layer: refuges.info huts, staffed refuges and
/// gîtes; OSM `alpine_hut`, `wilderness_hut` and `shelter` (not bus shelters).
pub fn place_names(refuges_info: &Value, osm: &Value) -> BTreeSet<String> {
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
    for f in features(osm) {
        let p = &f["properties"];
        if matches!(
            p["kind"].as_str(),
            Some("alpine_hut" | "wilderness_hut" | "shelter")
        ) && let Some(n) = trimmed(&p["tags"]["name"])
        {
            out.insert(n);
        }
    }
    out
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

/// Protected zone names and the end dates of their seasonal periods.
pub fn zone_names(zones: &Value) -> (BTreeSet<String>, BTreeSet<DayMonth>) {
    let (mut names, mut dates) = (BTreeSet::new(), BTreeSet::new());
    for f in features(zones) {
        let p = &f["properties"];
        if !protected(p) {
            continue;
        }
        if let Some(n) = trimmed(&p["name"]) {
            names.insert(n);
        }
        let months: Vec<u8> = p["months"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|m| m.as_u64().and_then(|m| u8::try_from(m).ok()))
            .collect();
        dates.extend(period_ends(&months));
    }
    (names, dates)
}

/// Pack names of `site` from its `pistes`, `refuges` and `zones` layers, with the sources
/// of the files read (from their `<layer>.meta.json`).
pub fn pack_names(layout: &DataLayout, site: &Site) -> Result<(PackNames, Vec<SourceRef>)> {
    let dir = |l: &str| layout.layer_dir(site, l);
    let meta = |l: &str| -> Result<LayerMeta> {
        let p = dir(l)?.join(LayerMeta::file_name(l));
        LayerMeta::read(&p).with_context(|| {
            format!(
                "{} : construire d'abord la couche (`skimap {l} {}`)",
                p.display(),
                site.id
            )
        })
    };
    let pistes_meta = meta("pistes")?;
    let pistes = piste_names(
        &read_json(&dir("pistes")?.join("pistes.geojson"), site, "pistes")?,
        &pistes_meta,
    );
    let refuges_meta = meta("refuges")?;
    let mut places = place_names(
        &read_json(
            &dir("refuges")?.join("refuges_info.geojson"),
            site,
            "refuges",
        )?,
        &read_json(&dir("refuges")?.join("osm.geojson"), site, "refuges")?,
    );
    let zones_meta = meta("zones")?;
    let (zones, dates) = zone_names(&read_json(
        &dir("zones")?.join("zones.geojson"),
        site,
        "zones",
    )?);
    places.extend(zones);

    let mut sources = pistes_meta.sources;
    sources.extend(
        refuges_meta
            .sources
            .into_iter()
            .filter(|s| s.url.contains("refuges.info/api/bbox") || s.url.contains("overpass")),
    );
    sources.extend(zones_meta.sources.into_iter().filter(|s| {
        s.name.starts_with("INPN")
            || s.url.contains("biodiv-sports")
            || s.url.contains("geo.admin.ch")
    }));
    for s in &mut sources {
        let used = s.note.take();
        s.note = Some(match used {
            Some(n) => format!("noms lus pour les clips de la voix ; {n}"),
            None => "noms lus pour les clips de la voix".into(),
        });
    }
    let status_pistes = pistes.len() as u32;
    Ok((
        PackNames {
            site: site.name.to_owned(),
            pistes: pistes.into_iter().collect(),
            places: places.into_iter().collect(),
            dates: dates.into_iter().collect(),
            status_pistes,
        },
        sources,
    ))
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
        assert_eq!(
            place_names(&ri, &osm).into_iter().collect::<Vec<_>>(),
            vec!["Abri".to_owned(), "Refuge des Fonts".to_owned()]
        );
        let zones = json!({"features": [
            {"properties": {"source": "biodiv-sports", "kind": "biodiv_sports", "name": "Tétras", "months": [12, 1, 2, 3, 4]}},
            {"properties": {"source": "gpu", "kind": "monument_historique", "name": "Eglise", "months": null}},
            {"properties": {"source": "geo-admin", "kind": "district_franc", "name": "District franc", "months": null}},
            {"properties": {"source": "geo-api", "kind": "commune", "name": "Cervières", "months": null}},
        ]});
        let (names, dates) = zone_names(&zones);
        assert_eq!(
            names.into_iter().collect::<Vec<_>>(),
            vec!["District franc".to_owned(), "Tétras".to_owned()]
        );
        assert_eq!(
            dates.into_iter().collect::<Vec<_>>(),
            vec![DayMonth { day: 30, month: 4 }]
        );
    }

    /// Q2 (#102): voice clips of the names of both sites from the reference layers
    /// (`fixtures/`, without Biodiv'sports): every name clip with its spoken text, and
    /// the number of clips by kind. Exact: fixed layers, deterministic text rules.
    /// Update with `SKIMAP_BLESS=1 cargo test -p skimap-cli voice` and review the diff.
    #[test]
    fn name_clips_of_the_fixtures_golden() {
        use crate::test_fixtures as fixtures;
        use skimap_voice::manifest::{ClipKind, manifest};
        let layout = DataLayout::new(fixtures::data());
        for site in skimap_common::site::SITES {
            let (names, sources) = pack_names(&layout, site).unwrap();
            assert!(sources.iter().all(|s| !s.personal_use_only));
            let m = manifest(&names);
            let mut k = fixtures::golden::Golden::new(&format!("voice/{}", site.id));
            k.count("pistes", names.pistes.len())
                .count("places", names.places.len())
                .count("dates", names.dates.len());
            for kind in [ClipKind::Fixed, ClipKind::Value, ClipKind::Name] {
                k.count(&format!("clips.{kind:?}"), m.count(kind));
            }
            for (id, (kind, text)) in &m.clips {
                if *kind == ClipKind::Name {
                    k.text(id, text.as_str());
                }
            }
            k.check();
        }
    }
}
