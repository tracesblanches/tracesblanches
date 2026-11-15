//! Snapshot of what the two packs offer to the styles (`styles/pack-schema.json`), so
//! that the validator runs without the packs (CI). Values are kept only for the
//! properties the styles compare; the rest keeps its key names, zooms and geometries.

use std::collections::BTreeSet;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::expr::walk;
use crate::generate::{Kind, style};
use crate::inventory::Inventory;
use crate::palette::Theme;
use crate::validate::{compared, expressions_of};
use crate::{Error, Result, styles_dir};

/// File name of the snapshot in `styles/`.
pub const FILE: &str = "pack-schema.json";

/// Sites of the proof packs: the two evaluation sites (built with the personal sources)
/// and a public pack of an SRN area (Hauts-Plateaux du Vercors 1/2, with the Corrençon -
/// Col de Rousset traverse): SRN areas and itineraries of every kind (SRN, easy and other
/// ski touring), none of the personal-use layers.
pub const SITES: [&str; 3] = [
    "cervieres",
    "chapelle-des-bois",
    "srn-domaine-geotrek-geo-pnr-vercors-fr-29403-1",
];

/// Inventories of the packs, values pruned to the compared properties.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    /// How to regenerate it.
    pub note: String,
    /// One inventory per site.
    pub packs: Vec<Inventory>,
}

/// `(pack file, source-layer or "", key)` compared by some layer of the styles.
pub fn compared_keys(styles: &[Value]) -> BTreeSet<(String, String, String)> {
    let mut out = BTreeSet::new();
    for s in styles {
        for l in s["layers"].as_array().into_iter().flatten() {
            let Some(src) = l["source"].as_str() else {
                continue;
            };
            let def = &s["sources"][src];
            let path = def["url"]
                .as_str()
                .and_then(|u| u.strip_prefix("pmtiles://"))
                .or_else(|| def["data"].as_str());
            let Some(path) = path else { continue };
            let sl = l["source-layer"].as_str().unwrap_or("").to_string();
            for e in expressions_of(l) {
                walk(e, &mut |a| {
                    for (k, _) in compared(a) {
                        out.insert((path.to_string(), sl.clone(), k));
                    }
                });
            }
        }
    }
    out
}

/// The six generated styles.
pub fn generated_styles() -> Vec<Value> {
    Kind::ALL
        .iter()
        .flat_map(|&k| Theme::ALL.iter().map(move |&t| style(k, t)))
        .collect()
}

/// Drops the values of the keys no style compares.
pub fn prune(inv: &mut Inventory, keep: &BTreeSet<(String, String, String)>) {
    let wanted = |path: &str, layer: &str, key: &str| {
        keep.contains(&(path.to_string(), layer.to_string(), key.to_string()))
    };
    for (path, a) in &mut inv.vector {
        for (name, l) in &mut a.layers {
            for (k, key) in &mut l.keys {
                if !wanted(path, name, k) {
                    key.values = None;
                }
            }
        }
    }
    for (path, l) in &mut inv.geojson {
        for (k, key) in &mut l.keys {
            if !wanted(path, "", k) {
                key.values = None;
            }
        }
    }
}

impl Snapshot {
    /// Reads the packs `<data_out>/<site>/pack` of [`SITES`].
    pub fn from_packs(data_out: &Path) -> Result<Self> {
        let keep = compared_keys(&generated_styles());
        let mut packs = Vec::new();
        for site in SITES {
            let mut inv = Inventory::from_pack(&data_out.join(site).join("pack"))?;
            prune(&mut inv, &keep);
            packs.push(inv);
        }
        Ok(Self {
            note: "Généré par `cargo run --release -p skimap-styles --example write_styles -- \
                   --schema` depuis data/out/<site>/pack ; ne pas éditer."
                .into(),
            packs,
        })
    }

    /// Reads `styles/pack-schema.json`.
    pub fn load() -> Result<Self> {
        let path = styles_dir().join(FILE);
        let text =
            std::fs::read_to_string(&path).map_err(|e| Error::Io(path.display().to_string(), e))?;
        serde_json::from_str(&text)
            .map_err(|e| Error::Json(path.display().to_string(), e.to_string()))
    }

    /// Pretty JSON with a final newline.
    pub fn to_text(&self) -> String {
        let mut s = serde_json::to_string_pretty(self).expect("serialisable snapshot");
        s.push('\n');
        s
    }
}
