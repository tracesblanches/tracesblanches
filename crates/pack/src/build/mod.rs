//! Pack builder (pipeline side): gathers the layers of `data/out/<site>/`, builds the
//! basemaps and raster tiles, writes `data/out/<site>/pack/` with its manifest.
//!
//! The pack is written to a temporary directory next to the final one, then swapped in:
//! a reader never sees a half-written pack.

pub mod assets;
pub mod class_tiles;
pub mod extract;
pub mod layers;
pub mod osm;
pub mod pistes;
pub mod plan_ign;
pub mod raster;

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
    PackValidity, SiteInfo, Source, VoiceSlot,
};
use crate::reader::sha256_file;
use skimap_common::meta::{FileMeta, LayerMeta};

/// Layer name of the pack in `data/out/<site>/`.
pub const PACK_LAYER: &str = "pack";

/// Default highest zoom of the Plan IGN basemap (P4 measurements, crate README).
pub const PLAN_IGN_MAXZOOM: u8 = 16;
/// Default highest zoom of the OSM basemap (the highest of the Protomaps builds).
pub const OSM_MAXZOOM: u8 = 15;

/// Options of a pack build.
#[derive(Debug, Clone)]
pub struct BuildOptions {
    /// Highest zoom of the Plan IGN tiles.
    pub plan_ign_maxzoom: u8,
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
    /// comparison in the viewer. Off by default: 22.6 MB at Cervières, offline licence
    /// unconfirmed (features.md n° 5).
    pub ign_slopes: bool,
    /// Include the voice clips when the voice crate produced them.
    pub voice: bool,
    /// Include the off-piste rasters of the core (`data/out/<site>/offpiste/`, gzipped).
    pub offpiste: bool,
    /// Vectorisation of the slope and avalanche exposure classes.
    pub class_params: class_tiles::Params,
}

impl Default for BuildOptions {
    fn default() -> Self {
        Self {
            plan_ign_maxzoom: PLAN_IGN_MAXZOOM,
            osm_maxzoom: OSM_MAXZOOM,
            plan_ign: true,
            osm: true,
            protomaps_build: None,
            personal_sources: false,
            ign_slopes: false,
            voice: true,
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
    /// Size by group (`basemap/plan-ign`, `layers`, `rasters`, `voice`...).
    pub bytes_by_group: Vec<(String, u64)>,
    /// Layers written.
    pub layers: Vec<String>,
    /// Layers not included, with the reason.
    pub missing: Vec<Missing>,
    /// Seconds per step.
    pub timings: Vec<(String, f64)>,
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
    let now = chrono::Utc::now();
    let pack_version = now.format("%Y%m%dT%H%M%SZ").to_string();
    let final_dir = pack_dir(site, layout)?;
    let parent = final_dir
        .parent()
        .ok_or_else(|| Error::Pack("pack directory without parent".into()))?;
    let tmp = parent.join(format!(".pack-build-{pack_version}"));
    if tmp.exists() {
        remove_dir(&tmp)?;
    }
    create_dir(&tmp)?;
    let result = build_into(site, layout, dl, opts, &tmp, &pack_version, now);
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
            report.pack_dir = final_dir;
            Ok(report)
        }
        Err(e) => {
            let _ = std::fs::remove_dir_all(&tmp);
            Err(e)
        }
    }
}

fn build_into(
    site: &Site,
    layout: &DataLayout,
    dl: &Downloader,
    opts: &BuildOptions,
    root: &Path,
    pack_version: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<BuildReport> {
    let mut timings = Vec::new();
    let mut missing = Vec::new();
    let out = layout.out_dir(site);

    let mut basemaps: Vec<Basemap> = Vec::new();
    if opts.plan_ign {
        let t = Instant::now();
        basemaps.push(plan_ign::build(site.bbox, dl, root, opts.plan_ign_maxzoom)?);
        timings.push(("basemap plan-ign".to_owned(), t.elapsed().as_secs_f64()));
    }
    if opts.osm {
        let t = Instant::now();
        basemaps.push(osm::build(
            site,
            layout,
            dl,
            root,
            opts.protomaps_build.as_deref(),
            opts.osm_maxzoom,
        )?);
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
        let (outcome, steps) =
            class_tiles::include(&spec, &out, root, &opts.class_params, opts.personal_sources)?;
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
        layers.push(raster::ign_slopes(site.bbox, dl, root)?);
        timings.push(("ign slope tiles".to_owned(), t.elapsed().as_secs_f64()));
    }

    let voice = if opts.voice {
        voice_slot(&out, root, opts.personal_sources)?
    } else {
        absent_voice("voix non demandée pour ce pack")
    };

    // Attributions, in display order, deduplicated.
    let mut seen = BTreeSet::new();
    let mut attributions = Vec::new();
    let all_sources = basemaps
        .iter()
        .flat_map(|b| b.sources.iter())
        .chain(layers.iter().flat_map(|l| l.sources.iter()))
        .chain(voice.sources.iter());
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
    let manifest = Manifest {
        format: FORMAT.to_owned(),
        format_version: FORMAT_VERSION,
        pack_version: pack_version.to_owned(),
        generator: format!("{} {}", crate::CRATE, env!("CARGO_PKG_VERSION")),
        generated_at: now.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        site: SiteInfo {
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
        },
        valid_for,
        personal_sources: opts.personal_sources,
        basemaps,
        layers,
        voice,
        missing: missing.clone(),
        attributions,
        files,
        total_bytes,
    };
    write_json(&root.join(MANIFEST_FILE), &manifest)?;
    Ok(BuildReport {
        pack_dir: root.to_path_buf(),
        pack_version: pack_version.to_owned(),
        total_bytes,
        bytes_by_group,
        layers: manifest.layers.iter().map(|l| l.id.clone()).collect(),
        missing,
        timings,
    })
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
    chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()?;
    Some(d.to_owned())
}

fn absent_voice(note: &str) -> VoiceSlot {
    VoiceSlot {
        status: "absent".to_owned(),
        manifest: None,
        clips_dir: None,
        clips: 0,
        bytes: 0,
        sources: Vec::new(),
        note: note.to_owned(),
    }
}

/// File of the voice layer that lists the clips of names read only from personal-use
/// sources (`clips`: ids; `skimap_voice::pack::PERSONAL_FILE`).
pub const VOICE_PERSONAL_FILE: &str = "personal.json";

/// Copies the voice clips (`data/out/<site>/voice/manifest.json` and `clips/`) if present.
///
/// The clips speak names read from the layers (pistes, refuges, zones, piste plans). With
/// `personal_sources` every clip is copied. Without, a layer with its list of personal
/// clips ([`VOICE_PERSONAL_FILE`]) is copied without them (names read only from
/// personal-use sources: Biodiv'sports, piste plans), with a manifest without them and
/// `voice.meta.json` without the personal-use sources; a layer without that list (built
/// before it) is left out whole when one of its sources is personal use.
fn voice_slot(out: &Path, root: &Path, personal_sources: bool) -> Result<VoiceSlot> {
    let src = out.join("voice");
    let meta_path = src.join("voice.meta.json");
    let restricted = layers::read_meta(&meta_path)?
        .as_ref()
        .is_some_and(|m| layers::restricted(&m.meta.sources));
    let manifest = src.join("manifest.json");
    let clips = src.join("clips");
    let marked = src.join(VOICE_PERSONAL_FILE).is_file();
    if restricted && !personal_sources && !marked {
        return Ok(absent_voice(
            "clips écartés : couche sans liste des noms à usage personnel (personal.json), et une source de la voix est à usage personnel (option personal_sources désactivée) ; reconstruire la voix",
        ));
    }
    if !manifest.is_file() || !clips.is_dir() {
        return Ok(absent_voice(
            "emplacement réservé : la crate voice (VO3) n'a pas encore produit de clips pour ce site",
        ));
    }
    let dst = root.join("voice");
    create_dir(&dst.join("clips"))?;
    if !personal_sources && marked {
        return public_voice(&src, &dst);
    }
    let mut bytes = copy_file(&manifest, &dst.join("manifest.json"))?;
    let mut n = 0u64;
    let mut entries: Vec<_> = read_dir_sorted(&clips)?;
    entries.retain(|p| p.is_file());
    for p in entries {
        let name = p
            .file_name()
            .ok_or_else(|| Error::Pack(format!("bad clip path {}", p.display())))?;
        bytes += copy_file(&p, &dst.join("clips").join(name))?;
        n += 1;
    }
    let sources = match layers::read_meta(&meta_path)? {
        Some(m) => {
            bytes += copy_file(&meta_path, &dst.join("voice.meta.json"))?;
            m.meta.sources
        }
        None => Vec::new(),
    };
    Ok(VoiceSlot {
        status: "present".to_owned(),
        manifest: Some("voice/manifest.json".to_owned()),
        clips_dir: Some("voice/clips".to_owned()),
        clips: n,
        bytes,
        sources,
        note: "clips Ogg/Opus de la crate voice ; format du manifeste des clips défini par VO3"
            .to_owned(),
    })
}

fn read_value(path: &Path) -> Result<Value> {
    let b = std::fs::read(path).map_err(|e| Error::io(path, e))?;
    serde_json::from_slice(&b).map_err(|e| Error::json(path.display().to_string(), e))
}

/// The voice of a published pack: the clips not listed in [`VOICE_PERSONAL_FILE`], the
/// manifest without the others, `voice.meta.json` without the personal-use sources and
/// with its counts redone.
fn public_voice(src: &Path, dst: &Path) -> Result<VoiceSlot> {
    let manifest = src.join("manifest.json");
    let personal_path = src.join(VOICE_PERSONAL_FILE);
    let personal: BTreeSet<String> = read_value(&personal_path)?
        .get("clips")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Pack(format!("{}: no clips list", personal_path.display())))?
        .iter()
        .map(|c| {
            c.as_str().map(str::to_owned).ok_or_else(|| {
                Error::Pack(format!("{}: clip id not a string", personal_path.display()))
            })
        })
        .collect::<Result<_>>()?;
    let mut vm = read_value(&manifest)?;
    let all = vm
        .get("clips")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let (kept, dropped): (Vec<Value>, Vec<Value>) = all.into_iter().partition(|c| {
        c.get("id")
            .and_then(Value::as_str)
            .is_none_or(|id| !personal.contains(id))
    });
    let mut bytes = 0;
    let mut clip_bytes = 0;
    for c in &kept {
        let file = c.get("file").and_then(Value::as_str).unwrap_or_default();
        let name = file
            .strip_prefix("clips/")
            .filter(|f| !f.is_empty() && !f.contains('/') && !f.contains(".."))
            .ok_or_else(|| {
                Error::Pack(format!(
                    "clip file {file:?} outside clips/ in {}",
                    manifest.display()
                ))
            })?;
        let b = copy_file(&src.join("clips").join(name), &dst.join("clips").join(name))?;
        bytes += b;
        clip_bytes += b;
    }
    let n = kept.len() as u64;
    let kind_count = |k: &str| kept.iter().filter(|c| c["kind"] == k).count();
    let audio_ms: u64 = kept.iter().filter_map(|c| c["duration_ms"].as_u64()).sum();
    let counts = (kind_count("fixed"), kind_count("value"), kind_count("name"));
    vm["clips"] = Value::Array(kept);
    bytes += write_json(&dst.join("manifest.json"), &vm)?;
    let meta_path = src.join("voice.meta.json");
    let mut sources = Vec::new();
    if meta_path.is_file() {
        let mut mv = read_value(&meta_path)?;
        let keep: Vec<bool> = layers::lenient_meta(&mv)
            .sources
            .iter()
            .map(|s| !layers::unauthorised(s))
            .collect();
        if let Some(arr) = mv.get_mut("sources").and_then(Value::as_array_mut) {
            let mut i = 0;
            arr.retain(|_| {
                let k = keep.get(i).copied().unwrap_or(false);
                i += 1;
                k
            });
        }
        if let Some(files) = mv.get_mut("files").and_then(Value::as_array_mut) {
            // The list of personal clips stays out of the published pack.
            files.retain(|f| f["name"] != VOICE_PERSONAL_FILE);
            for f in files {
                if f.get("count").is_some() {
                    f["count"] = n.into();
                }
                if f["name"] == "clips/<id>.opus" {
                    f["bytes"] = clip_bytes.into();
                }
            }
        }
        if let Some(st) = mv.get_mut("stats").and_then(Value::as_object_mut) {
            st.insert("clips".into(), n.into());
            st.insert("clips_fixed".into(), counts.0.into());
            st.insert("clips_value".into(), counts.1.into());
            st.insert("clips_name".into(), counts.2.into());
            st.insert("clips_personal".into(), 0.into());
            st.insert("bytes_personal".into(), 0.into());
            st.insert("bytes".into(), clip_bytes.into());
            st.insert(
                "audio_s".into(),
                ((audio_ms as f64 / 100.0).round() / 10.0).into(),
            );
            st.remove("names_personal");
            st.insert("public_pack_clips_removed".into(), dropped.len().into());
        }
        if let Some(notes) = mv.get_mut("notes").and_then(Value::as_array_mut) {
            notes.push(
                format!(
                    "Pack publié : {} clips de noms lus seulement dans des sources à usage personnel écartés, avec ces sources.",
                    dropped.len()
                )
                .into(),
            );
        }
        let path = dst.join("voice.meta.json");
        bytes += write_json(&path, &mv)?;
        let m = layers::read_meta(&path)?
            .ok_or_else(|| Error::Pack(format!("{} not written", path.display())))?;
        if layers::restricted(&m.meta.sources) {
            return Err(Error::Pack(format!(
                "{}: a personal-use source is left in the published voice",
                path.display()
            )));
        }
        sources = m.meta.sources;
    }
    Ok(VoiceSlot {
        status: "present".to_owned(),
        manifest: Some("voice/manifest.json".to_owned()),
        clips_dir: Some("voice/clips".to_owned()),
        clips: n,
        bytes,
        sources,
        note: format!(
            "clips Ogg/Opus de la crate voice ; format du manifeste des clips défini par VO3 ; pack publié : {} clips de noms à usage personnel écartés",
            dropped.len()
        ),
    })
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
    std::fs::copy(from, to).map_err(|e| Error::io(from, e))
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
    now: &chrono::DateTime<chrono::Utc>,
    validity: &PackValidity,
    basemaps: &[Basemap],
    layers: &[Layer],
    files: &[FileEntry],
) -> LayerMeta {
    let mut m = LayerMeta::new(PACK_LAYER, site.id);
    m.tickets = vec!["P4 (#17)".to_owned(), "P14 (#27)".to_owned()];
    m.generated_at = now.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
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
        "voice" => "clips de voix et leur manifeste",
        _ => "fichier du pack",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
