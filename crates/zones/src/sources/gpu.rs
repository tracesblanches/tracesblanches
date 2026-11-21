//! Public-utility easements of the Géoportail de l'urbanisme (GPU), layer
//! `wfs_sup:assiette_sup_s`: the only source of sites inscrits (R111-33) and of SPR.
//!
//! Kept: AC1 (historic monuments and their surroundings), AC2 (sites inscrits and
//! classés), AC4 (SPR), AS1 (drinking water catchments), A7 (forêts de protection).
//! AC2 does not say whether a site is inscrit or classé in every region: the build
//! decides with the INPN sites classés (see [`crate::build`]).

use geojson::JsonObject;
use skimap_common::download::Downloader;
use skimap_common::geom::BBox;

use super::{RawZone, prop_str, wfs_features};
use crate::error::Result;
use crate::layer::{ZoneKind, ZoneProps};
use jiff::civil::Date;

/// GPU easement layer.
pub const LAYER: &str = "wfs_sup:assiette_sup_s";

/// Classification of an AC2 easement from its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ac2Class {
    /// Name says "Site Classé".
    Classe,
    /// Name says "Site Inscrit".
    Inscrit,
    /// Name says nothing (e.g. Bourgogne-Franche-Comté).
    Unknown,
}

/// A GPU zone; `ac2` is set for AC2 easements.
#[derive(Debug, Clone)]
pub struct GpuZone {
    /// The zone (AC2 of unknown class carry [`ZoneKind::SiteInscrit`] until resolved).
    pub zone: RawZone,
    /// AC2 classification from the name.
    pub ac2: Option<Ac2Class>,
}

/// Fetches the kept easements intersecting the box.
pub fn fetch(dl: &Downloader, b: BBox) -> Result<Vec<GpuZone>> {
    let feats = wfs_features(dl, LAYER, b)?;
    tracing::info!(features = feats.len(), "GPU easements");
    Ok(feats
        .into_iter()
        .filter_map(|(p, geometry)| {
            parse(&p).map(|(props, ac2)| GpuZone {
                zone: RawZone { props, geometry },
                ac2,
            })
        })
        .collect())
}

/// Maps the properties of an easement; `None` for easement types that are not kept.
pub fn parse(p: &JsonObject) -> Option<(ZoneProps, Option<Ac2Class>)> {
    let suptype = prop_str(p, "suptype")?.to_ascii_lowercase();
    let typeass = prop_str(p, "typeass").unwrap_or_default();
    let raw_name = prop_str(p, "nomsuplitt")
        .or_else(|| prop_str(p, "nomass"))
        .unwrap_or_else(|| suptype.to_ascii_uppercase());
    let (kind, ac2, name) = match suptype.as_str() {
        "ac1" => {
            let kind = if typeass.to_lowercase().contains("abords") {
                ZoneKind::MonumentAbords
            } else {
                ZoneKind::MonumentHistorique
            };
            (kind, None, monument_name(p).unwrap_or(raw_name))
        }
        "ac2" => {
            let (class, name) = classify_ac2(&raw_name);
            let kind = if class == Ac2Class::Classe {
                ZoneKind::SiteClasse
            } else {
                ZoneKind::SiteInscrit
            };
            (kind, Some(class), name)
        }
        "ac4" => (ZoneKind::Spr, None, raw_name),
        "as1" => (ZoneKind::CaptageProtection, None, raw_name),
        "a7" => (ZoneKind::ForetProtection, None, raw_name),
        _ => return None,
    };
    let idass = prop_str(p, "idass")
        .or_else(|| prop_str(p, "gid"))
        .unwrap_or_else(|| format!("{suptype}-{name}"));
    let fichier = prop_str(p, "fichier");
    let url = match (prop_str(p, "gpu_doc_id"), &fichier) {
        (Some(doc), Some(f)) => Some(format!(
            "https://www.geoportail-urbanisme.gouv.fr/api/document/{doc}/files/{f}"
        )),
        _ => None,
    };
    Some((
        ZoneProps {
            id: format!("gpu:{idass}"),
            kind,
            name,
            source: "gpu".into(),
            url,
            months: None,
            practices: vec![],
            designated: fichier.as_deref().and_then(act_date),
            area_ha: 0.0,
            clipped: false,
            note: Some(format!(
                "servitude {} : {typeass}",
                suptype.to_ascii_uppercase()
            )),
        },
        ac2,
    ))
}

/// Splits the "Site Classé - " / "Site Inscrit - " prefix used in some regions.
pub fn classify_ac2(name: &str) -> (Ac2Class, String) {
    for (prefix, class) in [
        ("site classé", Ac2Class::Classe),
        ("site classe", Ac2Class::Classe),
        ("site inscrit", Ac2Class::Inscrit),
    ] {
        let n = prefix.len();
        if name.is_char_boundary(n) && name[..n].to_lowercase() == prefix {
            let rest = name[n..].trim_start_matches([' ', '-', ':']).trim();
            if !rest.is_empty() {
                return (class, rest.to_owned());
            }
        }
    }
    (Ac2Class::Unknown, name.to_owned())
}

/// Monument name from the act file name (`AC1_Eglise-paroissiale-Saint-Laurent-I6EABM_...`).
fn monument_name(p: &JsonObject) -> Option<String> {
    let f = prop_str(p, "fichier")?;
    let body = f.strip_prefix("AC1_")?;
    let end = body.rfind('_')?;
    let body = &body[..end];
    let end = body.rfind('_')?;
    let body = &body[..end];
    // Drop the Mérimée-like code suffix ("-I6EABM").
    let body = match body.rsplit_once('-') {
        Some((head, code))
            if code.len() == 6 && code.chars().all(|c| c.is_ascii_alphanumeric()) =>
        {
            head
        }
        _ => body,
    };
    Some(body.replace('-', " "))
}

/// Date of the act from the file name (`..._19740502_act.pdf` → `1974-05-02`).
pub fn act_date(fichier: &str) -> Option<String> {
    let stem = fichier.strip_suffix(".pdf").unwrap_or(fichier);
    let parts: Vec<&str> = stem.split('_').collect();
    let d = parts
        .iter()
        .rev()
        .find(|s| s.len() == 8 && s.bytes().all(|b| b.is_ascii_digit()))?;
    let (y, m, day) = (&d[0..4], &d[4..6], &d[6..8]);
    if m == "00" || day == "00" {
        return None;
    }
    Date::strptime("%Y-%m-%d", format!("{y}-{m}-{day}"))
        .ok()
        .map(|d| d.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn obj(v: serde_json::Value) -> JsonObject {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn ac2_classification() {
        assert_eq!(
            classify_ac2("Site Classé - Casse Déserte"),
            (Ac2Class::Classe, "Casse Déserte".to_owned())
        );
        assert_eq!(
            classify_ac2("Site Inscrit - Ville vieille de Briançon"),
            (Ac2Class::Inscrit, "Ville vieille de Briançon".to_owned())
        );
        assert_eq!(
            classify_ac2("Site de Chapelle-des-Bois"),
            (Ac2Class::Unknown, "Site de Chapelle-des-Bois".to_owned())
        );
    }

    #[test]
    fn act_dates() {
        assert_eq!(
            act_date("AC2_Site_De_Chapelle-Des-Bois_19740502_act.pdf").as_deref(),
            Some("1974-05-02")
        );
        assert_eq!(act_date("I4_0VAUXL610VERB_00000000_act.pdf"), None);
        assert_eq!(act_date("AC2_x_act.pdf"), None);
    }

    #[test]
    fn parse_easements() {
        // Real properties of the Chapelle-des-Bois site inscrit (GPU, 14/11/2026).
        let p = obj(json!({
            "suptype": "ac2", "gpu_doc_id": "c54cda3aaebe04f913393ae32f813d61",
            "fichier": "AC2_Site_De_Chapelle-Des-Bois_19740502_act.pdf",
            "idass": "AC2-130009012-22545-1-1", "typeass": "Enceinte du site",
            "nomsuplitt": "Site de Chapelle-des-Bois"
        }));
        let (z, ac2) = parse(&p).unwrap();
        assert_eq!(z.id, "gpu:AC2-130009012-22545-1-1");
        assert_eq!(z.kind, ZoneKind::SiteInscrit);
        assert_eq!(ac2, Some(Ac2Class::Unknown));
        assert_eq!(z.designated.as_deref(), Some("1974-05-02"));
        assert_eq!(
            z.url.as_deref(),
            Some(
                "https://www.geoportail-urbanisme.gouv.fr/api/document/c54cda3aaebe04f913393ae32f813d61/files/AC2_Site_De_Chapelle-Des-Bois_19740502_act.pdf"
            )
        );

        let spr = obj(
            json!({"suptype": "ac4", "idass": "AC4-1", "nomsuplitt": "Site Patrimonial Remarquable de Cervières", "typeass": "Périmètre du SPR"}),
        );
        assert_eq!(parse(&spr).unwrap().0.kind, ZoneKind::Spr);

        let abords = obj(
            json!({"suptype": "ac1", "idass": "AC1-2", "nomsuplitt": "", "typeass": "Périmètre des abords",
            "fichier": "AC1_Eglise-paroissiale-Saint-Laurent-I6EABM_19140509_act.pdf"}),
        );
        let (z, _) = parse(&abords).unwrap();
        assert_eq!(z.kind, ZoneKind::MonumentAbords);
        assert_eq!(z.name, "Eglise paroissiale Saint Laurent");
        assert_eq!(z.designated.as_deref(), Some("1914-05-09"));

        let mh = obj(json!({"suptype": "ac1", "idass": "AC1-3", "typeass": "Monument historique"}));
        assert_eq!(parse(&mh).unwrap().0.kind, ZoneKind::MonumentHistorique);

        let risk = obj(json!({"suptype": "pm1", "idass": "PM1-1"}));
        assert!(parse(&risk).is_none());
    }
}
