//! The pipeline layout of a pack's files (`<root>/out/<site>/<layer>/<file>`), rebuilt
//! with symbolic links, so that the loaders of the pipeline (`skimap_assist::inputs`,
//! `skimap_plan`) read a pack exactly as they read the pipeline outputs: on the phone
//! (morning, evening preparation, tracker) and at the end of a pack build, where the
//! planner ranks the starts of the site on the pack itself (#166, `build::rank`).
//!
//! A pack records where each of its files comes from ([`crate::manifest::Layer::origin`]).
//! Layers the pack carries in another format (snow and avalanche exposure GeoTIFFs, tiled
//! as PMTiles for the map) are not linked: the loaders report them missing, as on a
//! workstation without them.

use std::path::{Path, PathBuf};

use skimap_common::layout::DataLayout;

use crate::error::{Error, Result};
use crate::reader::{Pack, check_relative};

/// The pipeline layout of a pack's files, as symbolic links.
#[derive(Debug, Clone)]
pub struct PackView {
    /// Root of the layout (`<root>/out/<site>/<layer>/`).
    pub layout: DataLayout,
    /// Links made.
    pub links: usize,
}

impl PackView {
    /// Links every file of `pack` (layers and their metadata) under `root`
    /// at the place it comes from in the pipeline (`out/<site>/<origin>`). `root` is
    /// emptied first: a view is rebuilt from the pack it shows.
    pub fn build(pack: &Pack, root: &Path) -> Result<Self> {
        let site = &pack.manifest().site.id;
        let base = root.join("out").join(site);
        if base.exists() {
            std::fs::remove_dir_all(&base).map_err(|e| Error::io(&base, e))?;
        }
        let mut links = 0;
        let mut link = |target: PathBuf, at: PathBuf| -> Result<()> {
            if let Some(dir) = at.parent() {
                std::fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;
            }
            if at.symlink_metadata().is_ok() {
                return Ok(());
            }
            symlink(&target, &at).map_err(|e| Error::io(&at, e))?;
            links += 1;
            Ok(())
        };
        let ext = |p: &str| Path::new(p).extension().map(|e| e.to_ascii_lowercase());
        for l in &pack.manifest().layers {
            check_relative(&l.origin)?;
            // A file converted for the phone (GeoTIFF → PMTiles) is not what the loaders
            // read: left out, reported missing by them.
            if ext(&l.file) != ext(&l.origin) {
                continue;
            }
            let at = base.join(&l.origin);
            link(pack.path(&l.file)?, at.clone())?;
            if let (Some(meta), Some(dir)) = (&l.meta, at.parent()) {
                let src = pack.path(meta)?;
                if let Some(name) = src.file_name() {
                    link(src.clone(), dir.join(name))?;
                }
            }
        }
        Ok(Self {
            layout: DataLayout::new(root),
            links,
        })
    }
}

#[cfg(unix)]
fn symlink(target: &Path, at: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, at)
}

#[cfg(not(unix))]
fn symlink(target: &Path, at: &Path) -> std::io::Result<()> {
    if target.is_dir() {
        Err(std::io::Error::other("directory links need unix"))
    } else {
        skimap_common::output::copy_file(target, at)
            .map(|_| ())
            .map_err(std::io::Error::other)
    }
}
