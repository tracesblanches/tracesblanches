//! Catalogue of the nordic ski areas of France (census P3: `data/out/france/domaines/`),
//! embedded in the app as a small JSON resource (`examples/catalog.rs` writes it): pick an
//! area on a map of France, by name or by distance, and know whether its pack is on the
//! phone. No network: pack download is K2.

use std::sync::Arc;

use serde::Deserialize;

use crate::PackError;
use crate::info::{Bounds, PackEntry};

/// Version of the catalogue resource read by this library.
pub const CATALOG_FORMAT: u32 = 1;

/// Whether the pack of an area can be shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum PackStatus {
    /// A pack on the phone covers the area.
    Installed,
    /// Built by the pipeline, to download (K2: not yet possible).
    Available,
    /// No pack built for this area yet.
    NotBuilt,
}

/// A nordic ski area of the census.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Domain {
    /// Census identifier.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Commune and département number (`Bois-d'Amont (39)`).
    pub place: String,
    /// Longitude of the area.
    pub lon: f64,
    /// Latitude of the area.
    pub lat: f64,
    /// Kilometres of nordic pistes (OSM).
    pub km_nordic: f64,
    /// Pack status.
    pub status: PackStatus,
    /// Directory of the pack on the phone when installed.
    pub pack_dir: Option<String>,
}

/// An area and its distance to a position.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct DomainDistance {
    /// The area.
    pub domain: Domain,
    /// Distance in kilometres (great circle).
    pub km: f64,
}

#[derive(Debug, Deserialize)]
struct CatalogFile {
    format: u32,
    domains: Vec<RawDomain>,
    /// Outline of metropolitan France: rings of `[lon, lat, lon, lat, ...]`.
    outline: Vec<Vec<f64>>,
    #[serde(default)]
    attribution: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawDomain {
    id: String,
    name: String,
    #[serde(default)]
    alt_names: Vec<String>,
    commune: String,
    dep: String,
    lon: f64,
    lat: f64,
    km: f64,
    /// Built by the pipeline (from `catalog.json` of the pipeline, later).
    #[serde(default)]
    built: bool,
}

/// The catalogue, parsed once.
#[derive(Debug, uniffi::Object)]
pub struct Catalog {
    file: CatalogFile,
    /// Normalised search keys of each area (name, other names, commune).
    keys: Vec<Vec<String>>,
}

/// Lower case without French accents, for searching.
pub fn normalise(s: &str) -> String {
    s.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'à' | 'â' | 'ä' | 'á' => 'a',
            'ç' => 'c',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'î' | 'ï' | 'í' => 'i',
            'ô' | 'ö' | 'ó' => 'o',
            'ù' | 'û' | 'ü' | 'ú' => 'u',
            'ÿ' => 'y',
            '-' | '\'' | '’' => ' ',
            c => c,
        })
        .collect()
}

fn haversine_km(lon1: f64, lat1: f64, lon2: f64, lat2: f64) -> f64 {
    let (p1, p2) = (lat1.to_radians(), lat2.to_radians());
    let dp = p2 - p1;
    let dl = (lon2 - lon1).to_radians();
    let h = (dp / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
    2.0 * 6_371.008_8 * h.sqrt().asin()
}

fn inside(b: &Bounds, lon: f64, lat: f64) -> bool {
    lon >= b.west && lon <= b.east && lat >= b.south && lat <= b.north
}

impl Catalog {
    /// Parses the catalogue resource.
    pub fn parse(json: &str) -> Result<Self, PackError> {
        let file: CatalogFile = serde_json::from_str(json).map_err(|e| PackError::Invalid {
            detail: format!("catalog: {e}"),
        })?;
        if file.format != CATALOG_FORMAT {
            return Err(PackError::Invalid {
                detail: format!("catalog format {}, expected {CATALOG_FORMAT}", file.format),
            });
        }
        let keys = file
            .domains
            .iter()
            .map(|d| {
                std::iter::once(&d.name)
                    .chain(&d.alt_names)
                    .chain(std::iter::once(&d.commune))
                    .map(|s| normalise(s))
                    .collect()
            })
            .collect();
        Ok(Self { file, keys })
    }

    fn domain(&self, i: usize, packs: &[PackEntry]) -> Domain {
        let d = &self.file.domains[i];
        let pack = packs
            .iter()
            .filter(|p| p.error.is_none())
            .find(|p| p.bounds.as_ref().is_some_and(|b| inside(b, d.lon, d.lat)));
        let status = match (pack, d.built) {
            (Some(_), _) => PackStatus::Installed,
            (None, true) => PackStatus::Available,
            (None, false) => PackStatus::NotBuilt,
        };
        Domain {
            id: d.id.clone(),
            name: d.name.clone(),
            place: format!("{} ({})", d.commune, d.dep),
            lon: d.lon,
            lat: d.lat,
            km_nordic: d.km,
            status,
            pack_dir: pack.map(|p| p.dir.clone()),
        }
    }
}

#[uniffi::export]
impl Catalog {
    /// Parses the catalogue resource (JSON text).
    #[uniffi::constructor]
    pub fn from_json(json: String) -> Result<Arc<Self>, PackError> {
        Self::parse(&json).map(Arc::new)
    }

    /// Number of areas.
    pub fn len(&self) -> u32 {
        u32::try_from(self.file.domains.len()).unwrap_or(u32::MAX)
    }

    /// Whether the catalogue has no area.
    pub fn is_empty(&self) -> bool {
        self.file.domains.is_empty()
    }

    /// Every area, installed ones first, then by name; `packs`: the packs on the phone.
    pub fn domains(&self, packs: Vec<PackEntry>) -> Vec<Domain> {
        let mut v: Vec<Domain> = (0..self.file.domains.len())
            .map(|i| self.domain(i, &packs))
            .collect();
        v.sort_by_cached_key(|d| (d.status != PackStatus::Installed, normalise(&d.name)));
        v
    }

    /// Areas whose name, other names or commune contain every word of `query` (accents
    /// and case ignored); names starting with the query first.
    pub fn search(&self, query: String, packs: Vec<PackEntry>, limit: u32) -> Vec<Domain> {
        let q = normalise(query.trim());
        let words: Vec<&str> = q.split_whitespace().collect();
        if words.is_empty() {
            let mut all = self.domains(packs);
            all.truncate(limit as usize);
            return all;
        }
        let mut hits: Vec<(u8, String, usize)> = self
            .keys
            .iter()
            .enumerate()
            .filter(|(_, keys)| {
                let all = keys.join(" ");
                words.iter().all(|w| all.contains(w))
            })
            .map(|(i, keys)| {
                let rank = u8::from(!keys[0].starts_with(&q));
                (rank, keys[0].clone(), i)
            })
            .collect();
        hits.sort();
        hits.into_iter()
            .take(limit as usize)
            .map(|(_, _, i)| self.domain(i, &packs))
            .collect()
    }

    /// The `limit` areas nearest to a position.
    pub fn nearest(
        &self,
        lon: f64,
        lat: f64,
        packs: Vec<PackEntry>,
        limit: u32,
    ) -> Vec<DomainDistance> {
        let mut v: Vec<(f64, usize)> = self
            .file
            .domains
            .iter()
            .enumerate()
            .map(|(i, d)| (haversine_km(lon, lat, d.lon, d.lat), i))
            .collect();
        v.sort_by(|a, b| a.0.total_cmp(&b.0));
        v.into_iter()
            .take(limit as usize)
            .map(|(km, i)| DomainDistance {
                domain: self.domain(i, &packs),
                km,
            })
            .collect()
    }

    /// Outline of metropolitan France (département borders): rings of
    /// `[lon, lat, lon, lat, ...]`, for drawing the map of the picker.
    pub fn outline(&self) -> Vec<Vec<f64>> {
        self.file.outline.clone()
    }

    /// Attributions of the catalogue data.
    pub fn attribution(&self) -> Vec<String> {
        self.file.attribution.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const JSON: &str = r#"{"format": 1, "attribution": ["OSM"],
      "outline": [[6.0, 45.0, 7.0, 45.0, 7.0, 46.0, 6.0, 45.0]],
      "domains": [
        {"id": "a", "name": "Chapelle-des-Bois", "commune": "Chapelle-des-Bois", "dep": "25",
         "lon": 6.11, "lat": 46.6, "km": 120.5},
        {"id": "b", "name": "Les Rousses", "alt_names": ["Station des Rousses"], "commune": "Les Rousses",
         "dep": "39", "lon": 6.06, "lat": 46.48, "km": 200.0, "built": true},
        {"id": "c", "name": "Cervières", "commune": "Cervières", "dep": "05",
         "lon": 6.77, "lat": 44.86, "km": 40.0},
        {"id": "d", "name": "Plateau de Retord", "commune": "Le Grand-Abergement", "dep": "01",
         "lon": 5.7, "lat": 46.0, "km": 80.0}
      ]}"#;

    fn pack(dir: &str, w: f64, s: f64, e: f64, n: f64) -> PackEntry {
        PackEntry {
            dir: dir.into(),
            site_id: dir.into(),
            site_name: dir.into(),
            pack_version: "v".into(),
            error: None,
            bounds: Some(Bounds {
                west: w,
                south: s,
                east: e,
                north: n,
            }),
        }
    }

    #[test]
    fn accents_and_case() {
        assert_eq!(normalise("Cervières"), "cervieres");
        assert_eq!(normalise("Bois-d'Amont"), "bois d amont");
    }

    #[test]
    fn search_status_and_nearest() {
        let c = Catalog::parse(JSON).unwrap();
        assert_eq!(c.len(), 4);
        let packs = vec![pack("/p/cervieres", 6.66, 44.77, 6.86, 44.92)];
        let names = |v: Vec<Domain>| v.into_iter().map(|d| d.name).collect::<Vec<_>>();
        assert_eq!(
            names(c.search("cervieres".into(), packs.clone(), 10)),
            ["Cervières"]
        );
        assert_eq!(
            names(c.search("ROUSSES".into(), packs.clone(), 10)),
            ["Les Rousses"]
        );
        assert_eq!(
            names(c.search("station".into(), packs.clone(), 10)),
            ["Les Rousses"]
        );
        assert_eq!(
            names(c.search("abergement".into(), packs.clone(), 10)),
            ["Plateau de Retord"]
        );
        // Every word must match; name prefixes first.
        assert!(
            c.search("rousses chapelle".into(), packs.clone(), 10)
                .is_empty()
        );
        let ce = c.search("c".into(), packs.clone(), 10);
        assert_eq!(names(ce), ["Cervières", "Chapelle-des-Bois"]);
        // Status: installed (pack bounds), available (built), not built.
        let all = c.domains(packs.clone());
        assert_eq!(all[0].name, "Cervières");
        assert_eq!(all[0].status, PackStatus::Installed);
        assert_eq!(all[0].pack_dir.as_deref(), Some("/p/cervieres"));
        let rousses = all.iter().find(|d| d.id == "b").unwrap();
        assert_eq!(rousses.status, PackStatus::Available);
        let retord = all.iter().find(|d| d.id == "d").unwrap();
        assert_eq!(retord.status, PackStatus::NotBuilt);
        assert_eq!(retord.place, "Le Grand-Abergement (01)");
        // Nearest to Mouthe (6.19, 46.71): Chapelle-des-Bois, then Les Rousses.
        let n = c.nearest(6.19, 46.71, packs, 2);
        assert_eq!(n[0].domain.id, "a");
        assert_eq!(n[1].domain.id, "b");
        assert!((n[0].km - 14.6).abs() < 1.0, "{}", n[0].km);
        assert_eq!(c.outline().len(), 1);
        // An empty query lists everything, installed first.
        assert_eq!(c.search("  ".into(), vec![], 2).len(), 2);
        assert!(Catalog::parse(r#"{"format": 2, "domains": [], "outline": []}"#).is_err());
    }
}
