//! Pack builder (pipeline side): gathers the layers of `data/out/<site>/`, builds the
//! basemaps and raster tiles, writes `data/out/<site>/pack/` with its manifest.
//!
//! The pack is written to a temporary directory next to the final one, then swapped in:
//! a reader never sees a half-written pack.

use jiff::civil::Date;
pub mod assets;
pub mod class_tiles;
pub mod extract;
pub mod france;
pub mod layers;
pub mod osm;
pub mod parts;
pub mod pistes;
pub mod plan_ign;
pub mod plan_links;
pub mod rank;
pub mod raster;
pub mod starts;
pub mod stations;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::Serialize;
use serde_json::Value;
use skimap_common::download::Downloader;
use skimap_common::layout::DataLayout;
use skimap_common::site::Site;

use crate::error::{Error, Result};
use crate::manifest::{
    Basemap, FORMAT, FORMAT_VERSION, FileEntry, Layer, MANIFEST_FILE, Manifest, Missing,
    PackValidity, SiteInfo, Source,
};
use crate::reader::sha256_file;
use skimap_common::meta::{FileMeta, LayerMeta};

/// Layer name of the pack in `data/out/<site>/`.
pub const PACK_LAYER: &str = "pack";

/// Default highest zoom of the Plan IGN basemap (P4 measurements, crate README).
pub const PLAN_IGN_MAXZOOM: u8 = 16;
/// Default highest zoom of the OSM basemap (the highest of the Protomaps builds).
pub const OSM_MAXZOOM: u8 = 15;

/// Revision of the code of the pack outside its reusable parts ([`parts`]: layers,
/// rasters, manifest): bump it when a change of that code makes different bytes from the
/// same layers, so that every pack is built again once (its parts taken over while their
/// own code is unchanged). Part of [`code_revision`].
pub const PACK_REVISION: u32 = 2;

/// Fingerprint of the code that makes a pack with `opts` (#174): [`PACK_REVISION`], the
/// crate version and [`parts::PARTS_REVISION`] ([`parts::Key::new`]), and the code each
/// reusable part puts in its key (zooms, styles and assets of the basemaps, description
/// and vectorisation of the class layers: the same helpers as the part keys). The
/// pipeline records it with the pack step: a pack made by another code is built again,
/// even when none of its layers changed, and only the parts whose code changed are made
/// again. The inputs of the parts (layers, Protomaps build, IGN record) are not in it.
pub fn code_revision(opts: &BuildOptions) -> String {
    let mut k = parts::Key::new("code:pack");
    k.add("pack", &PACK_REVISION.to_string());
    osm::add_code(&mut k, (opts.osm_minzoom, opts.osm_maxzoom));
    plan_ign::add_code(&mut k, opts.plan_ign_maxzoom);
    for spec in class_tiles::catalog() {
        class_tiles::add_code(&mut k, &spec, &opts.class_params);
    }
    k.finish()
}

/// Options of a pack build.
#[derive(Debug, Clone)]
pub struct BuildOptions {
    /// Highest zoom of the Plan IGN tiles.
    pub plan_ign_maxzoom: u8,
    /// Lowest zoom of the OSM extract ([`crate::OSM_MINZOOM`]).
    pub osm_minzoom: u8,
    /// Highest zoom of the OSM extract.
    pub osm_maxzoom: u8,
    /// Build the Plan IGN basemap.
    pub plan_ign: bool,
    /// Build the OSM basemap (extract of the Protomaps build by range requests).
    pub osm: bool,
    /// Protomaps daily build (`AAAAMMJJ`); `None`: the latest listed.
    pub protomaps_build: Option<String>,
    /// Include layers whose sources are personal-use only (status, hourly weather).
    /// Off by default: those sources do not allow redistribution.
    pub personal_sources: bool,
    /// Include the IGN slope map tiles (WMTS, RGBA as served) next to our slope raster, for
    /// comparison in the viewer. Off by default: about 20 MB for a mountain site, offline licence
    /// unconfirmed (features.md n° 5).
    pub ign_slopes: bool,
    /// Include the off-piste rasters of the core (`data/out/<site>/offpiste/`, gzipped).
    pub offpiste: bool,
    /// Vectorisation of the slope and avalanche exposure classes.
    pub class_params: class_tiles::Params,
}

impl Default for BuildOptions {
    fn default() -> Self {
        Self {
            plan_ign_maxzoom: PLAN_IGN_MAXZOOM,
            osm_minzoom: crate::OSM_MINZOOM,
            osm_maxzoom: OSM_MAXZOOM,
            plan_ign: true,
            osm: true,
            protomaps_build: None,
            personal_sources: false,
            ign_slopes: false,
            offpiste: true,
            class_params: class_tiles::Params::default(),
        }
    }
}

/// Summary of a build.
#[derive(Debug, Clone, Serialize)]
pub struct BuildReport {
    /// Pack directory.
    pub pack_dir: PathBuf,
    /// Pack version.
    pub pack_version: String,
    /// Total size.
    pub total_bytes: u64,
    /// Size by group (`basemap/plan-ign`, `layers`, `rasters`...).
    pub bytes_by_group: Vec<(String, u64)>,
    /// Layers written.
    pub layers: Vec<String>,
    /// Layers not included, with the reason.
    pub missing: Vec<Missing>,
    /// Seconds per step.
    pub timings: Vec<(String, f64)>,
    /// Parts taken from the previous pack, their inputs unchanged ([`parts`]).
    pub reused: Vec<String>,
}

/// Pack directory of a site: `data/out/<site>/pack/`.
pub fn pack_dir(site: &Site, layout: &DataLayout) -> Result<PathBuf> {
    Ok(layout.layer_dir(site, PACK_LAYER)?)
}

/// Builds the pack of `site` from the pipeline outputs in `layout`.
pub fn build(
    site: &Site,
    layout: &DataLayout,
    dl: &Downloader,
    opts: &BuildOptions,
) -> Result<BuildReport> {
    let now = jiff::Timestamp::now();
    let pack_version = now.strftime("%Y%m%dT%H%M%SZ").to_string();
    let final_dir = pack_dir(site, layout)?;
    let parent = final_dir
        .parent()
        .ok_or_else(|| Error::Pack("pack directory without parent".into()))?;
    let tmp = parent.join(format!(".pack-build-{pack_version}"));
    if tmp.exists() {
        remove_dir(&tmp)?;
    }
    create_dir(&tmp)?;
    let mut parts = parts::Parts::open(&final_dir);
    let result = build_into(site, layout, dl, opts, &tmp, &pack_version, now, &mut parts);
    match result {
        Ok(mut report) => {
            let old = parent.join(format!(".pack-old-{pack_version}"));
            if final_dir.exists() {
                std::fs::rename(&final_dir, &old).map_err(|e| Error::io(&final_dir, e))?;
            }
            std::fs::rename(&tmp, &final_dir).map_err(|e| Error::io(&tmp, e))?;
            if old.exists() {
                remove_dir(&old)?;
            }
            // After the swap: the keys describe the pack in place. Not written, the next
            // build makes every part again.
            if let Err(e) = parts.write(&final_dir, &pack_version) {
                tracing::warn!(site = site.id, error = %e, "pack part keys not written");
            }
            report.pack_dir = final_dir;
            Ok(report)
        }
        Err(e) => {
            let _ = std::fs::remove_dir_all(&tmp);
            Err(e)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn build_into(
    site: &Site,
    layout: &DataLayout,
    dl: &Downloader,
    opts: &BuildOptions,
    root: &Path,
    pack_version: &str,
    now: jiff::Timestamp,
    parts: &mut parts::Parts,
) -> Result<BuildReport> {
    let mut timings = Vec::new();
    let mut missing = Vec::new();
    let out = layout.out_dir(site);

    let mut basemaps: Vec<Basemap> = Vec::new();
    if opts.plan_ign {
        let t = Instant::now();
        let id = format!("basemap:{}", plan_ign::ID);
        let key = plan_ign::part_key(site.bbox, dl, opts.plan_ign_maxzoom)?;
        let b = match parts.reuse::<Basemap>(&id, &key, root) {
            Some(b) => b,
            None => {
                let b = plan_ign::build(site.bbox, dl, root, opts.plan_ign_maxzoom)?;
                let dir = format!("basemap/{}", plan_ign::ID);
                parts.record(&id, key, root, &[&b.tiles, &dir], &b)?;
                b
            }
        };
        basemaps.push(b);
        timings.push(("basemap plan-ign".to_owned(), t.elapsed().as_secs_f64()));
    }
    if opts.osm {
        let t = Instant::now();
        let id = format!("basemap:{}", osm::ID);
        let latest = osm::select_build(dl, opts.protomaps_build.as_deref())?;
        let zooms = (opts.osm_minzoom, opts.osm_maxzoom);
        let key = osm::part_key(site, layout, &latest, zooms)?;
        let b = match parts.reuse::<Basemap>(&id, &key, root) {
            Some(b) => b,
            None => {
                let b = osm::build_from(site, layout, dl, root, &latest, zooms)?;
                // The key after the choice of the build in use (adopted or kept).
                let key = osm::part_key(site, layout, &latest, zooms)?;
                let dir = format!("basemap/{}", osm::ID);
                parts.record(&id, key, root, &[&b.tiles, &dir], &b)?;
                b
            }
        };
        basemaps.push(b);
        timings.push(("basemap osm".to_owned(), t.elapsed().as_secs_f64()));
    }

    let t = Instant::now();
    let mut layers: Vec<Layer> = Vec::new();
    let mut specs = layers::catalog();
    specs.extend(layers::discovered(&out)?);
    for spec in specs {
        if !opts.offpiste && spec.origin == "offpiste" {
            continue;
        }
        match layers::include(&spec, &out, root, opts.personal_sources)? {
            layers::Outcome::Included(l) => layers.push(*l),
            layers::Outcome::Missing(m) => missing.push(m),
        }
    }
    if let Some(l) = piste_labels(root, &layers)? {
        layers.push(l);
    }
    plan_refuges(root, &mut layers)?;
    for o in stations::include(layout, site.bbox, root, opts.personal_sources)? {
        match o {
            layers::Outcome::Included(l) => layers.push(*l),
            layers::Outcome::Missing(m) => missing.push(m),
        }
    }
    // Link to the official plan of each domain, public packs included (G1, #117).
    plan_links::apply(root, &mut layers, &plan_links::PlanLinks::load(layout)?)?;
    timings.push((
        "vector and data layers".to_owned(),
        t.elapsed().as_secs_f64(),
    ));

    let t = Instant::now();
    for spec in raster::catalog() {
        match raster::include(&spec, &out, root, opts.personal_sources)? {
            layers::Outcome::Included(l) => layers.push(*l),
            layers::Outcome::Missing(m) => missing.push(m),
        }
    }
    timings.push(("raster layers".to_owned(), t.elapsed().as_secs_f64()));
    for spec in class_tiles::catalog() {
        let t = Instant::now();
        let id = format!("class:{}", spec.id);
        let key = class_tiles::part_key(&spec, &out, &opts.class_params, opts.personal_sources)?;
        let (outcome, steps) = match parts.reuse::<Layer>(&id, &key, root) {
            Some(l) => {
                class_tiles::copy_meta_of(&spec, &out, root)?;
                (layers::Outcome::Included(Box::new(l)), Vec::new())
            }
            None => {
                let r = class_tiles::include(
                    &spec,
                    &out,
                    root,
                    &opts.class_params,
                    opts.personal_sources,
                )?;
                if let layers::Outcome::Included(l) = &r.0 {
                    parts.record(&id, key, root, &[&l.file], l.as_ref())?;
                }
                r
            }
        };
        match outcome {
            layers::Outcome::Included(l) => layers.push(*l),
            layers::Outcome::Missing(m) => missing.push(m),
        }
        for (step, secs) in steps {
            timings.push((format!("{} : {step}", spec.id), secs));
        }
        timings.push((
            format!("{} (vector classes)", spec.id),
            t.elapsed().as_secs_f64(),
        ));
    }
    if opts.ign_slopes {
        let t = Instant::now();
        // Published only when its source may be (skimap_common::licence, #171).
        if layers::restricted(&[raster::ign_slopes_source()]) && !opts.personal_sources {
            missing.push(Missing {
                id: "ign-slopes".to_owned(),
                reason: "source à usage personnel seulement : option personal_sources désactivée"
                    .to_owned(),
            });
        } else {
            layers.push(raster::ign_slopes(site.bbox, dl, root)?);
        }
        timings.push(("ign slope tiles".to_owned(), t.elapsed().as_secs_f64()));
    }

    // Attributions, in display order, deduplicated.
    let mut seen = BTreeSet::new();
    let mut attributions = Vec::new();
    let all_sources = basemaps
        .iter()
        .flat_map(|b| b.sources.iter())
        .chain(layers.iter().flat_map(|l| l.sources.iter()));
    for s in all_sources {
        let a = &s.attribution;
        if !a.is_empty() && seen.insert(a.clone()) {
            attributions.push(a.clone());
        }
    }

    let valid_for = pack_validity(&layers);
    // pack.meta.json (shared schema), then the file list of the manifest.
    let before = list_files(root)?;
    pack_meta(site, &now, &valid_for, &basemaps, &layers, &before).write(root)?;
    let files = list_files(root)?;
    let total_bytes = files.iter().map(|f| f.bytes).sum();
    let bytes_by_group = group_sizes(&files);
    let mut manifest = Manifest {
        format: FORMAT.to_owned(),
        format_version: FORMAT_VERSION,
        pack_version: pack_version.to_owned(),
        generator: format!("{} {}", crate::CRATE, env!("CARGO_PKG_VERSION")),
        generated_at: now.strftime("%Y-%m-%dT%H:%M:%SZ").to_string(),
        site: site_info(site, layout, opts.personal_sources),
        valid_for,
        personal_sources: opts.personal_sources,
        basemaps,
        layers,
        missing: missing.clone(),
        attributions,
        files,
        total_bytes,
    };
    write_json(&root.join(MANIFEST_FILE), &manifest)?;

    // The starts ranked by the planner on the pack just written (#166), then the
    // manifest again with the record in that order (the file list does not change).
    if let Some(record) = manifest.site.record.as_mut() {
        let t = Instant::now();
        let work = root
            .parent()
            .unwrap_or(root)
            .join(format!(".pack-rank-{pack_version}"));
        manifest.site.start_ranking = rank::rank(
            site,
            record,
            root,
            &work,
            skimap_common::dates::utc_date(now),
        );
        if manifest.site.start_ranking.is_some() {
            write_json(&root.join(MANIFEST_FILE), &manifest)?;
        }
        timings.push(("start ranking".to_owned(), t.elapsed().as_secs_f64()));
    }
    Ok(BuildReport {
        pack_dir: root.to_path_buf(),
        pack_version: pack_version.to_owned(),
        total_bytes,
        bytes_by_group,
        layers: manifest.layers.iter().map(|l| l.id.clone()).collect(),
        missing,
        timings,
        reused: parts.reused().to_vec(),
    })
}

/// The piste name lines ([`pistes::labels`]) derived from the `pistes` layer of the
/// pack, as written (enriched, coordinates rounded): same sources, dates and use
/// restrictions as `pistes`, hidden from the viewer's raw layer list by default.
fn piste_labels(root: &Path, layers: &[Layer]) -> Result<Option<Layer>> {
    let Some(p) = layers.iter().find(|l| l.id == "pistes") else {
        return Ok(None);
    };
    let src = root.join(&p.file);
    let text = std::fs::read_to_string(&src).map_err(|e| Error::io(&src, e))?;
    let v: Value = serde_json::from_str(&text).map_err(|e| Error::json(&p.file, e))?;
    let fc = pistes::labels(&v);
    let n = fc["features"].as_array().map_or(0, Vec::len) as u64;
    let file = format!("layers/{}.geojson", pistes::LABELS_LAYER);
    let bytes = serde_json::to_vec(&fc).map_err(|e| Error::json(&file, e))?;
    let size = write_bytes(&root.join(&file), &bytes)?;
    let mut l = p.clone();
    l.id = pistes::LABELS_LAYER.to_owned();
    l.title = "Noms des pistes (lignes fusionnées par itinéraire)".to_owned();
    l.file = file;
    l.origin = format!("{} (pack)", p.file);
    l.bytes = size;
    l.features = Some(n);
    l.geometry_types = if n > 0 {
        vec!["LineString".to_owned()]
    } else {
        Vec::new()
    };
    l.visible = false;
    l.notes = vec![
        "Une ligne par itinéraire (relation OSM, sinon noms du plan des pistes, sinon nom de la voie), voies jointes bout à bout là où deux voies de l'itinéraire seulement se touchent ; pour placer les noms le long des pistes. Propriétés : `name`, `difficulty`, `length_m`, `ways`.".to_owned(),
    ];
    Ok(Some(l))
}

/// Pack layers of the refuges the day plan reads (refuges.info, OSM).
const PLAN_REFUGE_LAYERS: [&str; 2] = ["refuges-info", "refuges-osm"];

/// Flags in the `refuges-info` and `refuges-osm` layers of the pack, as written, the
/// refuges of the day plan ([`skimap_core::huts::mark`], X21 #155: the same hut in both
/// sources kept once), so that the phone reads the flag instead of comparing the two
/// layers when a pack opens. The layers stay two files.
fn plan_refuges(root: &Path, layers: &mut [Layer]) -> Result<()> {
    let read = |l: &Layer| -> Result<Value> {
        let path = root.join(&l.file);
        let text = std::fs::read_to_string(&path).map_err(|e| Error::io(&path, e))?;
        serde_json::from_str(&text).map_err(|e| Error::json(&l.file, e))
    };
    let find = |id: &str| {
        layers
            .iter()
            .position(|l| l.id == id && l.format == "geojson")
    };
    let at = PLAN_REFUGE_LAYERS.map(find);
    if at.iter().all(Option::is_none) {
        return Ok(());
    }
    let value = |i: Option<usize>| i.map_or(Ok(Value::Null), |i| read(&layers[i]));
    let (mut info, mut osm) = (value(at[0])?, value(at[1])?);
    let counts = skimap_core::huts::mark(&mut info, &mut osm);
    for ((i, v), n) in at.into_iter().zip([info, osm]).zip([counts.0, counts.1]) {
        let Some(i) = i else { continue };
        let l = &mut layers[i];
        let bytes = serde_json::to_vec(&v).map_err(|e| Error::json(&l.file, e))?;
        l.bytes = write_bytes(&root.join(&l.file), &bytes)?;
        l.notes.push(format!(
            "Refuges du plan de la journée : propriété `{}` (vrai sur {n} entités) ; un refuge présent dans les deux sources (moins de {} m) n'est retenu qu'une fois, celui de refuges.info.",
            skimap_core::huts::PLAN_FLAG,
            skimap_core::huts::SAME_PLACE_M
        ));
    }
    Ok(())
}

/// Earliest explicit `valid_until` date among the layers.
fn pack_validity(layers: &[Layer]) -> PackValidity {
    let mut best: Option<(String, String)> = None;
    for l in layers {
        let Some(until) = l.valid_until.as_deref() else {
            continue;
        };
        let Some(date) = leading_date(until) else {
            continue;
        };
        if best.as_ref().is_none_or(|(d, _)| date < *d) {
            best = Some((date, l.id.clone()));
        }
    }
    let note = match &best {
        Some((d, l)) => format!(
            "Pack à reconstruire avant le {d} (couche {l}). Chaque couche garde sa propre date \
             de validité ; l'état des pistes, la neige et le BERA vieillissent en heures ou en jours."
        ),
        None => {
            "Aucune couche ne donne de date de fin de validité ; voir chaque couche.".to_owned()
        }
    };
    let (until, until_layer) = best.map_or((None, None), |(d, l)| (Some(d), Some(l)));
    PackValidity {
        until,
        until_layer,
        note,
    }
}

/// `AAAA-MM-JJ` at the start of a string.
fn leading_date(s: &str) -> Option<String> {
    let d = s.get(..10)?;
    Date::strptime("%Y-%m-%d", d).ok()?;
    Some(d.to_owned())
}

fn site_info(site: &Site, layout: &DataLayout, personal_sources: bool) -> SiteInfo {
    SiteInfo {
        id: site.id.to_owned(),
        name: site.name.to_owned(),
        department: site.department.to_owned(),
        bbox: [
            site.bbox.west,
            site.bbox.south,
            site.bbox.east,
            site.bbox.north,
        ],
        center: [site.commune_center.lon, site.commune_center.lat],
        record: Some(starts::record(site, layout, personal_sources)),
        start_ranking: None,
    }
}

/// Every file under `root` except the manifest, sorted, with size and SHA-256.
pub fn list_files(root: &Path) -> Result<Vec<FileEntry>> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for p in read_dir_sorted(&dir)? {
            if p.is_dir() {
                stack.push(p);
            } else {
                let rel = rel_path(root, &p)?;
                if rel == MANIFEST_FILE {
                    continue;
                }
                let (bytes, sha256) = sha256_file(&p)?;
                out.push(FileEntry {
                    path: rel,
                    bytes,
                    sha256,
                });
            }
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

/// Sizes by group: `basemap/<id>`, first directory otherwise.
pub fn group_sizes(files: &[FileEntry]) -> Vec<(String, u64)> {
    let mut groups: Vec<(String, u64)> = Vec::new();
    for f in files {
        let mut parts = f.path.split('/');
        let first = parts.next().unwrap_or_default();
        let key = if first == "basemap" {
            let second = parts.next().unwrap_or_default();
            format!("basemap/{}", second.trim_end_matches(".pmtiles"))
        } else if f.path.contains('/') {
            first.to_owned()
        } else {
            f.path.clone()
        };
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, n)) => *n += f.bytes,
            None => groups.push((key, f.bytes)),
        }
    }
    groups
}

/// `/`-separated path of `p` relative to `root`.
pub fn rel_path(root: &Path, p: &Path) -> Result<String> {
    let rel = p
        .strip_prefix(root)
        .map_err(|_| Error::Pack(format!("{} is not under {}", p.display(), root.display())))?;
    let parts: Vec<String> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    Ok(parts.join("/"))
}

pub(crate) fn read_dir_sorted(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| Error::io(dir, e))?
        .map(|e| e.map(|e| e.path()).map_err(|e| Error::io(dir, e)))
        .collect::<Result<_>>()?;
    v.sort();
    Ok(v)
}

pub(crate) fn create_dir(dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))
}

fn remove_dir(dir: &Path) -> Result<()> {
    std::fs::remove_dir_all(dir).map_err(|e| Error::io(dir, e))
}

pub(crate) fn copy_file(from: &Path, to: &Path) -> Result<u64> {
    if let Some(p) = to.parent() {
        create_dir(p)?;
    }
    Ok(skimap_common::output::copy_file(from, to)?)
}

/// Copies a pipeline file into the pack: a `.json` file is rewritten as compact JSON (the
/// pipeline writes it indented for reading; the phone only parses it: half the bytes for
/// the weather and the assistant, #160), any other file byte for byte. Returns the size.
pub(crate) fn copy_data(from: &Path, to: &Path) -> Result<u64> {
    if from.extension().and_then(|e| e.to_str()) != Some("json") {
        return copy_file(from, to);
    }
    let text = std::fs::read(from).map_err(|e| Error::io(from, e))?;
    let v: serde_json::Value =
        serde_json::from_slice(&text).map_err(|e| Error::json(from.display().to_string(), e))?;
    let out = serde_json::to_vec(&v).map_err(|e| Error::json(to.display().to_string(), e))?;
    if let Some(p) = to.parent() {
        create_dir(p)?;
    }
    write_bytes(to, &out)
}

/// Atomic write (`common::output`); returns the size.
pub(crate) fn write_bytes(path: &Path, bytes: &[u8]) -> Result<u64> {
    skimap_common::output::write_bytes(path, bytes)?;
    Ok(bytes.len() as u64)
}

/// Atomic pretty JSON write (`common::output`); returns the size.
pub(crate) fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<u64> {
    skimap_common::output::write_json(path, value)?;
    Ok(std::fs::metadata(path)
        .map_err(|e| Error::io(path, e))?
        .len())
}

/// Size of a directory tree.
pub(crate) fn dir_size(dir: &Path) -> Result<u64> {
    let mut n = 0;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for p in read_dir_sorted(&d)? {
            if p.is_dir() {
                stack.push(p);
            } else {
                n += std::fs::metadata(&p).map_err(|e| Error::io(&p, e))?.len();
            }
        }
    }
    Ok(n)
}

/// A source declared by this crate (basemaps, IGN slope tiles).
pub(crate) fn source(name: &str, url: &str, licence: &str, attribution: &str) -> Source {
    Source::new(name, url, licence, attribution)
}

/// `pack.meta.json`: the pack described in the shared layer schema.
fn pack_meta(
    site: &Site,
    now: &jiff::Timestamp,
    validity: &PackValidity,
    basemaps: &[Basemap],
    layers: &[Layer],
    files: &[FileEntry],
) -> LayerMeta {
    let mut m = LayerMeta::new(PACK_LAYER, site.id);
    m.tickets = vec!["P4 (#17)".to_owned(), "P14 (#27)".to_owned()];
    m.generated_at = now.strftime("%Y-%m-%dT%H:%M:%SZ").to_string();
    m.valid_until = validity.until.clone();
    m.validity = Some(validity.note.clone());
    m.crs =
        Some("tuiles EPSG:3857 (XYZ) ; GeoJSON EPSG:4326 ; rasters du noyau EPSG:2154".to_owned());
    let mut seen = BTreeSet::new();
    for s in basemaps
        .iter()
        .flat_map(|b| b.sources.iter())
        .chain(layers.iter().flat_map(|l| l.sources.iter()))
    {
        if seen.insert((s.name.clone(), s.url.clone())) {
            m.sources.push(s.clone());
        }
    }
    let mut f = FileMeta::new(
        MANIFEST_FILE,
        "manifeste du pack (format skimap-pack v1) : couches, fonds, voix, sources, tailles et SHA-256 de chaque fichier",
    );
    f.format = Some("JSON".to_owned());
    m.files.push(f);
    for (group, bytes) in group_sizes(files) {
        let mut f = FileMeta::new(group.clone(), group_description(&group));
        f.bytes = Some(bytes);
        m.files.push(f);
    }
    m.notes = vec![
        "Pack hors ligne d'un site : fonds de carte PMTiles + style MapLibre, couches GeoJSON, rasters en tuiles PNG à palette (l'indice est la classe), données du noyau, clips de voix.".to_owned(),
        "Lecture : skimap_pack::Pack (sans pile HTTP) ; visualiseur : tools/viewer/index.html.".to_owned(),
    ];
    m.stats = serde_json::json!({
        "total_bytes": files.iter().map(|f| f.bytes).sum::<u64>(),
        "bytes_by_group": group_sizes(files),
        "layers": layers.len(),
        "basemaps": basemaps.len(),
    });
    m
}

fn group_description(group: &str) -> &'static str {
    match group {
        "basemap/plan-ign" => {
            "fond Plan IGN v2 : tuiles vectorielles PMTiles, style, sprite, glyphes"
        }
        "basemap/osm" => {
            "fond OpenStreetMap (Protomaps) : tuiles vectorielles PMTiles, style, sprite, glyphes"
        }
        "layers" => {
            "couches GeoJSON, données JSON et rasters du noyau (gzip), métadonnées des couches"
        }
        "rasters" => "rasters en tuiles PNG à palette (PMTiles)",
        _ => "fichier du pack",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #174: the code of a part changes the code fingerprint of the pack (the OSM
    /// basemap from zoom 12 instead of 0, #160), the inputs of the parts do not.
    #[test]
    fn code_revision_follows_the_code_of_every_part() {
        let now = BuildOptions::default();
        let base = code_revision(&now);
        assert_eq!(base, code_revision(&now));
        let from_zero = BuildOptions {
            osm_minzoom: 0,
            ..now.clone()
        };
        assert_ne!(base, code_revision(&from_zero), "OSM zooms");
        let ign = BuildOptions {
            plan_ign_maxzoom: 15,
            ..now.clone()
        };
        assert_ne!(base, code_revision(&ign), "Plan IGN zooms");
        let mut params = now.class_params;
        params.tolerance_px += 0.5;
        let class = BuildOptions {
            class_params: params,
            ..now.clone()
        };
        assert_ne!(base, code_revision(&class), "vectorisation");
        let inputs = BuildOptions {
            personal_sources: true,
            protomaps_build: Some("20261118".into()),
            ..now.clone()
        };
        assert_eq!(base, code_revision(&inputs), "inputs, not code");
        assert_ne!(base, crate::export::code_revision());
    }

    #[test]
    fn leading_dates() {
        assert_eq!(leading_date("2026-12-14").as_deref(), Some("2026-12-14"));
        assert_eq!(
            leading_date("2026-11-13 06:00 UTC").as_deref(),
            Some("2026-11-13")
        );
        assert_eq!(leading_date("relief permanent"), None);
        assert_eq!(leading_date("2026-13-01"), None);
    }

    #[test]
    fn plan_refuges_flag_both_layers_in_the_pack() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("layers")).unwrap();
        let pt = |lon: f64, props: Value| {
            serde_json::json!({"type": "Feature", "properties": props,
                               "geometry": {"type": "Point", "coordinates": [lon, 45.0]}})
        };
        let info = serde_json::json!({"type": "FeatureCollection", "features": [
            pt(6.0, serde_json::json!({"type_label": "refuge gardé"})),
            pt(6.1, serde_json::json!({"type_label": "point d'eau"}))]});
        let osm = serde_json::json!({"type": "FeatureCollection", "features": [
            pt(6.0003, serde_json::json!({"kind": "alpine_hut"})),
            pt(6.01, serde_json::json!({"kind": "shelter"}))]});
        let layer = |id: &str, fc: &Value| -> Layer {
            let file = format!("layers/{id}.geojson");
            std::fs::write(root.path().join(&file), fc.to_string()).unwrap();
            serde_json::from_value(serde_json::json!({
                "id": id, "title": id, "kind": "vector", "format": "geojson", "file": file,
                "origin": "refuges/x.geojson", "sources": [], "personal_use_only": false,
                "bytes": 0, "visible": true}))
            .unwrap()
        };
        let mut layers = vec![layer("refuges-info", &info), layer("refuges-osm", &osm)];
        plan_refuges(root.path(), &mut layers).unwrap();
        let flags = |l: &Layer| {
            let v: Value =
                serde_json::from_slice(&std::fs::read(root.path().join(&l.file)).unwrap()).unwrap();
            assert_eq!(
                l.bytes,
                std::fs::metadata(root.path().join(&l.file)).unwrap().len()
            );
            v["features"]
                .as_array()
                .unwrap()
                .iter()
                .map(|f| f["properties"][skimap_core::huts::PLAN_FLAG].as_bool())
                .collect::<Vec<_>>()
        };
        assert_eq!(flags(&layers[0]), [Some(true), Some(false)]);
        // The OSM hut 24 m from the refuges.info one is the same place.
        assert_eq!(flags(&layers[1]), [Some(false), Some(true)]);
        assert!(layers.iter().all(|l| l.notes.len() == 1));
    }

    #[test]
    fn groups() {
        let f = |p: &str, n| FileEntry {
            path: p.to_owned(),
            bytes: n,
            sha256: String::new(),
        };
        let g = group_sizes(&[
            f("basemap/plan-ign.pmtiles", 10),
            f("basemap/plan-ign/style.json", 1),
            f("basemap/osm.pmtiles", 5),
            f("layers/a.geojson", 2),
            f("layers/b.geojson", 3),
            f("README", 7),
        ]);
        assert_eq!(
            g,
            vec![
                ("basemap/plan-ign".to_owned(), 11),
                ("basemap/osm".to_owned(), 5),
                ("layers".to_owned(), 5),
                ("README".to_owned(), 7)
            ]
        );
    }
}
