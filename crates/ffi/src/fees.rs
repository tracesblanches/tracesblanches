//! Trail-pass validity zones (« zones de redevance », H2 #133), read from the pack layer
//! `fee-zones` when a pack has it.
//!
//! No crate of `main` produces the layer yet (crate `stations` to come). Its schema,
//! documented in `android/docs/telechargement.md`: polygons with `fee_id` (the same in
//! every pack for one pass: ENJ, Nordic Pass), `name`, `issuer`, `kind` (`site`,
//! `groupement`, `departement`, `region`, `national`) and `prices`
//! (`[{label, price_eur, date}]`, indicative and dated). The test pack carries such a
//! layer (`examples/test_pack.rs`). The map draws it once a validated style does.

use serde_json::Value;
use skimap_pack::Pack;

use crate::PackError;

/// Pack layer of the zones.
pub const FEE_ZONES_LAYER: &str = "fee-zones";

/// An indicative price of a pass.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct FeePrice {
    /// What it buys (`Journée adulte`).
    pub label: String,
    /// Price in euros.
    pub price_eur: f64,
    /// Date of the price (`AAAA-MM-JJ`), when known.
    pub date: Option<String>,
}

/// A pass and where it is valid, over every pack of the set.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct FeeZone {
    /// Identifier of the pass, the same in every pack.
    pub fee_id: String,
    /// Name (`Espace nordique jurassien`).
    pub name: String,
    /// Who sells it.
    pub issuer: String,
    /// `site`, `groupement`, `departement`, `region` or `national`.
    pub kind: String,
    /// Sites (pack identifiers) where the zone appears.
    pub sites: Vec<String>,
    /// Indicative prices, dated.
    pub prices: Vec<FeePrice>,
}

fn text(v: &Value) -> String {
    v.as_str().unwrap_or_default().trim().to_owned()
}

/// Zones of the passes in `packs`, one per `fee_id`, in order of appearance.
pub fn fee_zones(packs: &[&Pack]) -> Result<Vec<FeeZone>, PackError> {
    let mut out: Vec<FeeZone> = Vec::new();
    for pack in packs {
        if pack.manifest().layer(FEE_ZONES_LAYER).is_none() {
            continue;
        }
        let site = pack.manifest().site.id.clone();
        let fc = pack.read_geojson(FEE_ZONES_LAYER)?;
        for f in &fc.features {
            let Some(p) = f.properties.as_ref() else {
                continue;
            };
            let id = p.get("fee_id").map(text).unwrap_or_default();
            if id.is_empty() {
                continue;
            }
            if let Some(z) = out.iter_mut().find(|z| z.fee_id == id) {
                if !z.sites.contains(&site) {
                    z.sites.push(site.clone());
                }
                continue;
            }
            let prices = p
                .get("prices")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|pr| {
                    Some(FeePrice {
                        label: text(pr.get("label")?),
                        price_eur: pr.get("price_eur")?.as_f64()?,
                        date: pr.get("date").and_then(Value::as_str).map(str::to_owned),
                    })
                })
                .collect();
            out.push(FeeZone {
                fee_id: id,
                name: p.get("name").map(text).unwrap_or_default(),
                issuer: p.get("issuer").map(text).unwrap_or_default(),
                kind: p.get("kind").map(text).unwrap_or_default(),
                sites: vec![site.clone()],
                prices,
            });
        }
    }
    Ok(out)
}
