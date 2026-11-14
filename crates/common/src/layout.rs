//! On-disk layout of the data directory (ignored by git).
//!
//! ```text
//! data/                       root, `SKIMAP_DATA_DIR` or ./data
//!   cache/<source>/           downloader cache: <key>.body, <key>.json, ratelimit.json
//!   out/<site>/<layer>/...    pipeline products per site (GeoJSON, grids, packs)
//! ```

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::site::Site;

/// Paths of the data directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataLayout {
    root: PathBuf,
}

impl DataLayout {
    /// Layout rooted at `root` (nothing is created).
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Root directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Root of the downloader cache.
    pub fn cache_root(&self) -> PathBuf {
        self.root.join("cache")
    }

    /// Cache directory of one source.
    pub fn cache_dir(&self, source: &str) -> Result<PathBuf> {
        Ok(self.cache_root().join(checked_component(source)?))
    }

    /// Root of the pipeline products.
    pub fn out_root(&self) -> PathBuf {
        self.root.join("out")
    }

    /// Product directory of a site.
    pub fn out_dir(&self, site: &Site) -> PathBuf {
        self.out_root().join(site.id)
    }

    /// Product directory of one layer of a site, e.g. `out/cervieres/pistes`.
    pub fn layer_dir(&self, site: &Site, layer: &str) -> Result<PathBuf> {
        Ok(self.out_dir(site).join(checked_component(layer)?))
    }

    /// Creates `dir` and its parents.
    pub fn ensure(dir: &Path) -> Result<()> {
        std::fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))
    }
}

/// Accepts a single safe path component: ASCII letters, digits, `-`, `_`, `.`, not
/// starting with a dot.
pub fn checked_component(name: &str) -> Result<&str> {
    let ok = !name.is_empty()
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'));
    if ok {
        Ok(name)
    } else {
        Err(Error::Config(format!("invalid path component `{name}`")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::site::CERVIERES;

    #[test]
    fn paths() {
        let l = DataLayout::new("data");
        assert_eq!(
            l.cache_dir("overpass").unwrap(),
            Path::new("data/cache/overpass")
        );
        assert_eq!(l.out_dir(&CERVIERES), Path::new("data/out/cervieres"));
        assert_eq!(
            l.layer_dir(&CERVIERES, "pistes").unwrap(),
            Path::new("data/out/cervieres/pistes")
        );
        assert!(l.cache_dir("../etc").is_err());
        assert!(l.cache_dir("a/b").is_err());
        assert!(l.cache_dir("").is_err());
        assert!(l.cache_dir(".hidden").is_err());
    }
}
