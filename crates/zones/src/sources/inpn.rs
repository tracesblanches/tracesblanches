//! INPN protected areas (PatriNat), WFS of the Géoplateforme.

use skimap_common::download::Downloader;
use skimap_common::geom::BBox;

use super::{RawZone, prop_str, wfs_features};
use crate::error::Result;
use crate::layer::{ZoneKind, ZoneProps};

/// INPN layers fetched, with the kind of their zones.
pub const LAYERS: &[(&str, ZoneKind)] = &[
    ("patrinat_apb:apb", ZoneKind::Appb),
    ("patrinat_rnn:rnn", ZoneKind::ReserveNaturelleNationale),
    ("patrinat_rnr:rnr", ZoneKind::ReserveNaturelleRegionale),
    ("patrinat_pnr:pnr", ZoneKind::ParcNaturelRegional),
    ("patrinat_pn:parc_national", ZoneKind::ParcNational),
    ("patrinat_sc:sc", ZoneKind::SiteClasse),
    ("patrinat_sic:sic", ZoneKind::Natura2000Sic),
    ("patrinat_zps:zps", ZoneKind::Natura2000Zps),
    ("patrinat_rb:rb", ZoneKind::ReserveBiologique),
];

/// Fetches every INPN layer intersecting the box.
pub fn fetch(dl: &Downloader, b: BBox) -> Result<Vec<RawZone>> {
    let mut out = Vec::new();
    for (layer, kind) in LAYERS {
        let feats = wfs_features(dl, layer, b)?;
        tracing::info!(layer, features = feats.len(), "INPN");
        for (p, geometry) in feats {
            let code = prop_str(&p, "id_mnhn")
                .or_else(|| prop_str(&p, "cd_sig"))
                .unwrap_or_else(|| format!("{layer}-unknown"));
            out.push(RawZone {
                props: ZoneProps {
                    id: format!("inpn:{code}"),
                    kind: *kind,
                    name: prop_str(&p, "nom_site").unwrap_or_else(|| code.clone()),
                    source: "inpn".into(),
                    url: prop_str(&p, "url_fiche"),
                    months: None,
                    practices: vec![],
                    designated: prop_str(&p, "date_crea")
                        .map(|d| d.trim_end_matches('Z').to_owned()),
                    area_ha: 0.0,
                    clipped: false,
                    note: None,
                },
                geometry,
            });
        }
    }
    Ok(out)
}
