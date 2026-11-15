//! `skimap fixtures`: the versioned reference data set of the two sites (Q1, #101).
//!
//! Extracts from `data/cache` and `data/out` a few megabytes that the tests read without
//! network (`fixtures/` at the root of the repository):
//!
//! - `inputs/osm/`: the OpenSnowMap planet extract cut down to the ways around the two
//!   sites (every way with a node in the region the pistes builder looks at, with all its
//!   nodes, and the relations reaching them), gzip XML in the planet format;
//! - `inputs/<site>/dem_profile_cells_5m.tif`: the 5 m terrain model kept only on the
//!   cells that the elevation profiles of the trail graph read (bilinear neighbourhood of
//!   every station), `NaN` elsewhere;
//! - `inputs/<site>/dem_window_5m.tif`: a 1 km × 1 km window of the terrain model (the one
//!   with the widest spread of slopes), for the slope and aspect operator;
//! - `data/out/<site>/<layer>/`: the redistributable pipeline outputs read by the
//!   phone-side crates (pistes, routing graph, zones without Biodiv'sports, coverage,
//!   huts, snow, avalanche exposure of Chapelle-des-Bois);
//! - `manifest.json`: every file with its size, SHA-256, licence key (`LICENCES.md`) and
//!   origin.
//!
//! Never extracted: the personal-use sources (Nordic France, the Cervières town hall
//! page, ENJ, Open-Meteo), the historic avalanche paths (CLPA republished by the DDT 05,
//! letter 3 pending; INRAE: personal use), Biodiv'sports (no licence found), the links
//! between huts of different licences, the snowtools massif raster.
//!
//! Every extract is checked against the full data before it is written: the pistes layer
//! rebuilt from the cut planet and the trail graph rebuilt from the cut terrain model are
//! byte-identical to `data/out`, and the slope and aspect of the window equal those of
//! the terrain layer on its interior. The output is deterministic: running the command
//! twice on the same data gives the same bytes.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use skimap_common::crs::to_lambert93;
use skimap_common::download::{Downloader, Request};
use skimap_common::geom::BBox;
use skimap_common::grid::Grid;
use skimap_common::layout::DataLayout;
use skimap_common::site::{CHAPELLE_DES_BOIS, SITES, Site};
use skimap_osm::extract::{self, Planet};
use skimap_osm::parse::{MemberKind, OsmData};
use skimap_terrain::geotiff;

/// E-mail and phone patterns, shared with the fixtures tests (one copy in a test build).
#[cfg(not(test))]
#[path = "../../../fixtures/support/privacy.rs"]
mod privacy;
#[cfg(test)]
use crate::test_fixtures::privacy;
use privacy::personal_pattern;

/// Planet extract of the fixtures, relative to the fixtures root.
pub const PLANET_FILE: &str = "inputs/osm/planet_pistes_sites.osm.gz";
/// State file (timestamp of the OSM data) of the planet extract.
pub const PLANET_STATE_FILE: &str = "inputs/osm/planet_pistes-state.txt";
/// Terrain model cells read by the elevation profiles, under `inputs/<site>/`.
pub const DEM_PROFILE_FILE: &str = "dem_profile_cells_5m.tif";
/// 1 km terrain model window, under `inputs/<site>/`.
pub const DEM_WINDOW_FILE: &str = "dem_window_5m.tif";
/// Side of the terrain window, cells (1 km at 5 m).
pub const WINDOW_CELLS: usize = 200;
/// Metres per degree of latitude (same constant as the pistes builder).
const M_PER_DEG_LAT: f64 = 111_195.0;

/// Licence keys of `fixtures/LICENCES.md`.
mod licence {
    pub const ODBL: &str = "odbl-osm";
    pub const LO_IGN: &str = "lo-ign";
    pub const LO_METEOFRANCE: &str = "lo-meteofrance";
    pub const ZONES: &str = "zones-mixed";
    pub const ARCEP: &str = "lo-arcep";
    pub const CC_BY_SA: &str = "cc-by-sa-refuges";
    pub const DERIVED: &str = "lo-skimap";
}

/// One file of the set.
struct Entry {
    path: String,
    licence: &'static str,
    origin: String,
}

/// Generator state: fixtures root, data layout, lines printed at the end.
struct Gen<'a> {
    root: PathBuf,
    layout: &'a DataLayout,
    entries: Vec<Entry>,
    lines: Vec<String>,
}

/// Writes the fixture set into `root` (created if needed; the generated files are
/// replaced, `LICENCES.md`, `support/` and `golden/` are left alone). Returns the report
/// lines.
pub fn generate(layout: &DataLayout, root: &Path) -> Result<Vec<String>> {
    let dl = Downloader::builder(layout.cache_root())
        .offline(true)
        .build()
        .context("offline downloader on data/cache")?;
    for d in ["inputs", "data"] {
        let p = root.join(d);
        if p.exists() {
            std::fs::remove_dir_all(&p).with_context(|| format!("removing {}", p.display()))?;
        }
    }
    let mut g = Gen {
        root: root.to_path_buf(),
        layout,
        entries: Vec::new(),
        lines: Vec::new(),
    };
    g.planet(&dl)?;
    for site in SITES {
        g.dem_profile_cells(site)?;
        g.dem_window(site)?;
        g.layers(site)?;
    }
    g.manifest()?;
    Ok(g.lines)
}

impl Gen<'_> {
    fn path(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    fn add(&mut self, rel: &str, licence: &'static str, origin: impl Into<String>) {
        self.entries.push(Entry {
            path: rel.to_owned(),
            licence,
            origin: origin.into(),
        });
    }

    fn write(
        &mut self,
        rel: &str,
        bytes: &[u8],
        licence: &'static str,
        origin: &str,
    ) -> Result<()> {
        let p = self.path(rel);
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d)?;
        }
        std::fs::write(&p, bytes).with_context(|| format!("writing {}", p.display()))?;
        self.add(rel, licence, origin);
        Ok(())
    }

    /// Copies `data/out/<site>/<layer>/<name>` unchanged.
    fn copy(&mut self, site: &Site, layer: &str, name: &str, licence: &'static str) -> Result<()> {
        let src = self.layout.layer_dir(site, layer)?.join(name);
        let bytes = std::fs::read(&src).with_context(|| format!("reading {}", src.display()))?;
        let rel = format!("data/out/{}/{layer}/{name}", site.id);
        self.write(
            &rel,
            &bytes,
            licence,
            &format!("data/out/{}/{layer}/{name}", site.id),
        )
    }

    /// Writes a modified JSON copy of `data/out/<site>/<layer>/<name>`.
    fn write_json(
        &mut self,
        site: &Site,
        layer: &str,
        name: &str,
        v: &Value,
        licence: &'static str,
        how: &str,
    ) -> Result<()> {
        let mut body = serde_json::to_vec_pretty(v)?;
        body.push(b'\n');
        let rel = format!("data/out/{}/{layer}/{name}", site.id);
        let origin = format!("data/out/{}/{layer}/{name} ({how})", site.id);
        self.write(&rel, &body, licence, &origin)
    }

    fn read_json(&self, site: &Site, layer: &str, name: &str) -> Result<Value> {
        let p = self.layout.layer_dir(site, layer)?.join(name);
        let s = std::fs::read_to_string(&p).with_context(|| format!("reading {}", p.display()))?;
        serde_json::from_str(&s).with_context(|| format!("parsing {}", p.display()))
    }

    // ---- OSM ----

    fn planet(&mut self, dl: &Downloader) -> Result<()> {
        let planet = extract::load_planet(dl).context("planet extract (data/cache/opensnowmap)")?;
        let state = dl
            .fetch(&Request::get(extract::SOURCE, extract::STATE_URL))
            .context("planet state (data/cache/opensnowmap)")?
            .bytes()?;
        let mut keep_ways: BTreeSet<i64> = BTreeSet::new();
        for site in SITES {
            let region = region_box(&planet, &site.bbox);
            let before = keep_ways.len();
            for w in &planet.data.ways {
                if planet
                    .data
                    .way_points(w)
                    .iter()
                    .any(|(_, p)| region.contains(*p))
                {
                    keep_ways.insert(w.id);
                }
            }
            self.lines.push(format!(
                "osm {}: region s,w,n,e {:.4},{:.4},{:.4},{:.4}, {} ways",
                site.id,
                region.south,
                region.west,
                region.north,
                region.east,
                keep_ways.len() - before
            ));
        }
        let xml = planet_xml(&planet.data, &keep_ways)?;
        let mut gz = flate2::GzBuilder::new().write(Vec::new(), flate2::Compression::best());
        gz.write_all(&xml)?;
        let gz = gz.finish()?;
        let origin = format!(
            "data/cache/opensnowmap planet_pistes.osm.gz (sha256 {}), ways around the two sites",
            planet.source.sha256.as_deref().unwrap_or("?")
        );
        self.write(PLANET_FILE, &gz, licence::ODBL, &origin)?;
        self.write(
            PLANET_STATE_FILE,
            &state,
            licence::ODBL,
            "data/cache/opensnowmap planet_pistes-state.txt",
        )?;
        self.lines.push(format!(
            "osm: {} ways, {} bytes of XML, {} bytes gzip",
            keep_ways.len(),
            xml.len(),
            gz.len()
        ));

        // Proof: the pistes layer rebuilt from the cut extract equals data/out.
        let cut = read_planet(&self.path(PLANET_FILE), &self.path(PLANET_STATE_FILE))?;
        let tmp = tempfile::tempdir()?;
        let tl = DataLayout::new(tmp.path());
        for site in SITES {
            extract::build_from(site, &tl, &cut)?;
            for f in [
                "pistes.geojson",
                "graph_edges.geojson",
                "graph_nodes.geojson",
            ] {
                let a = std::fs::read(tl.layer_dir(site, "pistes")?.join(f))?;
                let b = std::fs::read(self.layout.layer_dir(site, "pistes")?.join(f))?;
                ensure!(
                    a == b,
                    "{} {f}: rebuilt from the cut extract differs from data/out",
                    site.id
                );
            }
            self.lines.push(format!(
                "osm {}: pistes.geojson, graph_edges.geojson, graph_nodes.geojson rebuilt from the cut extract: identical to data/out",
                site.id
            ));
        }
        Ok(())
    }

    // ---- terrain ----

    fn dem(&self, site: &Site, name: &str) -> Result<Grid<f32>> {
        let p = self.layout.layer_dir(site, "terrain")?.join(name);
        geotiff::read_f32(&p).with_context(|| format!("reading {}", p.display()))
    }

    fn dem_profile_cells(&mut self, site: &Site) -> Result<()> {
        let dem = self.dem(site, "dem_5m.tif")?;
        let pistes = self.layout.layer_dir(site, "pistes")?;
        let mut graph = skimap_core::graph::PisteGraph::load_osm_layer(site.id, &pistes)?;
        let mut keep = vec![false; dem.width() * dem.height()];
        skimap_core::dem::attach_profiles(&mut graph, skimap_core::dem::PROFILE_STEP_M, "", |p| {
            if let Ok((x, y)) = to_lambert93(p) {
                // `Grid::sample_bilinear` reads the cells (c0, r0) to (c0 + 1, r0 + 1)
                // with c0 = floor(col − 0.5).
                let (c, r) = dem.transform().world_to_pixel(x, y);
                let (c0, r0) = ((c - 0.5).floor() as i64, (r - 0.5).floor() as i64);
                for rr in r0..=r0 + 1 {
                    for cc in c0..=c0 + 1 {
                        if (0..dem.width() as i64).contains(&cc)
                            && (0..dem.height() as i64).contains(&rr)
                        {
                            keep[rr as usize * dem.width() + cc as usize] = true;
                        }
                    }
                }
            }
            skimap_core::dem::elevation(&dem, p)
        });
        let data: Vec<f32> = dem
            .data()
            .iter()
            .zip(&keep)
            .map(|(v, k)| if *k { *v } else { f32::NAN })
            .collect();
        let kept = keep.iter().filter(|k| **k).count();
        let mut cut = Grid::from_vec(dem.width(), dem.height(), *dem.transform(), dem.crs(), data)?;
        if let Some(n) = dem.nodata() {
            cut = cut.with_nodata(*n);
        }
        let rel = format!("inputs/{}/{DEM_PROFILE_FILE}", site.id);
        let p = self.path(&rel);
        std::fs::create_dir_all(p.parent().expect("parent"))?;
        let bytes = geotiff::write_f32(&p, &cut)?;
        self.add(
            &rel,
            licence::LO_IGN,
            format!(
                "data/out/{}/terrain/dem_5m.tif, {kept} cells read by the graph profiles, NaN elsewhere",
                site.id
            ),
        );
        self.lines.push(format!(
            "terrain {}: {kept} of {} cells kept for the profiles, {bytes} bytes",
            site.id,
            keep.len()
        ));

        // Proof: the graph rebuilt with the cut terrain model equals data/out.
        let tmp = tempfile::tempdir()?;
        let tl = DataLayout::new(tmp.path());
        copy_dir(&pistes, &tl.layer_dir(site, "pistes")?)?;
        let terrain = tl.layer_dir(site, "terrain")?;
        std::fs::create_dir_all(&terrain)?;
        std::fs::copy(&p, terrain.join("dem_5m.tif"))?;
        std::fs::copy(
            self.layout
                .layer_dir(site, "terrain")?
                .join("terrain.meta.json"),
            terrain.join("terrain.meta.json"),
        )?;
        skimap_core::routing::build(site, &tl)?;
        let a = std::fs::read(tl.layer_dir(site, "routes")?.join("piste_graph.json"))?;
        let b = std::fs::read(
            self.layout
                .layer_dir(site, "routes")?
                .join("piste_graph.json"),
        )?;
        ensure!(
            a == b,
            "{}: piste_graph.json rebuilt from the cut terrain model differs from data/out",
            site.id
        );
        self.lines.push(format!(
            "terrain {}: piste_graph.json rebuilt from the cut terrain model: identical to data/out",
            site.id
        ));
        Ok(())
    }

    fn dem_window(&mut self, site: &Site) -> Result<()> {
        let dem = self.dem(site, "dem_5m.tif")?;
        let slope = self.dem(site, "slope_5m.tif")?;
        let aspect = self.dem(site, "aspect_5m.tif")?;
        let n = WINDOW_CELLS;
        let mut best: Option<((usize, u64), usize, usize)> = None;
        for r0 in (0..dem.height().saturating_sub(n)).step_by(n) {
            for c0 in (0..dem.width().saturating_sub(n)).step_by(n) {
                let mut bins = [0u64; 18];
                let mut valid = true;
                'w: for r in r0..r0 + n {
                    for c in c0..c0 + n {
                        if !dem.get(c, r).is_some_and(|v| v.is_finite()) {
                            valid = false;
                            break 'w;
                        }
                        if let Some(s) = slope.get(c, r).filter(|s| s.is_finite()) {
                            bins[((*s / 5.0) as usize).min(17)] += 1;
                        }
                    }
                }
                if !valid {
                    continue;
                }
                let total: u64 = bins.iter().sum();
                let spread = bins.iter().filter(|b| **b * 100 >= total).count();
                let steep: u64 = bins[6..].iter().sum();
                let score = (spread, steep);
                if best.is_none_or(|(s, _, _)| score > s) {
                    best = Some((score, c0, r0));
                }
            }
        }
        let ((spread, _), c0, r0) =
            best.ok_or_else(|| anyhow::anyhow!("{}: no fully valid 1 km window", site.id))?;
        let (x0, y0) = dem.transform().pixel_to_world(c0 as f64, r0 as f64);
        let t = skimap_common::grid::GeoTransform::north_up(x0, y0, dem.transform().pixel_width);
        let mut data = Vec::with_capacity(n * n);
        for r in r0..r0 + n {
            for c in c0..c0 + n {
                data.push(*dem.get(c, r).expect("inside"));
            }
        }
        let win = Grid::from_vec(n, n, t, dem.crs(), data)?;
        // Proof: the slope and aspect of the window equal the terrain layer's inside.
        let sa = skimap_terrain::slope::horn(&win);
        for r in 1..n - 1 {
            for c in 1..n - 1 {
                let (s, a) = (sa.slope.get(c, r), sa.aspect.get(c, r));
                let (s0, a0) = (slope.get(c0 + c, r0 + r), aspect.get(c0 + c, r0 + r));
                if s.map(|v| v.to_bits()) != s0.map(|v| v.to_bits())
                    || a.map(|v| v.to_bits()) != a0.map(|v| v.to_bits())
                {
                    bail!("{}: slope of the window differs at ({c}, {r})", site.id);
                }
            }
        }
        let rel = format!("inputs/{}/{DEM_WINDOW_FILE}", site.id);
        let p = self.path(&rel);
        std::fs::create_dir_all(p.parent().expect("parent"))?;
        let bytes = geotiff::write_f32(&p, &win)?;
        self.add(
            &rel,
            licence::LO_IGN,
            format!(
                "data/out/{}/terrain/dem_5m.tif, cells {c0}..{} × {r0}..{} (Lambert-93 x {x0}, y {y0}, top left)",
                site.id,
                c0 + n,
                r0 + n
            ),
        );
        self.lines.push(format!(
            "terrain {}: window at x {x0}, y {y0} ({spread} slope classes of 5° with ≥ 1 %), {bytes} bytes; slope and aspect identical to the terrain layer inside",
            site.id
        ));
        Ok(())
    }

    // ---- layers ----

    fn layers(&mut self, site: &Site) -> Result<()> {
        use licence::*;
        self.copy(site, "pistes", "pistes.geojson", ODBL)?;
        self.copy(site, "pistes", "pistes.meta.json", ODBL)?;
        self.copy(site, "terrain", "terrain.meta.json", LO_IGN)?;
        self.copy(site, "routes", "piste_graph.json", ODBL)?;
        let meta = self.read_json(site, "routes", "routes.meta.json")?;
        let meta = keep_files(meta, &["piste_graph.json"]);
        self.write_json(
            site,
            "routes",
            "routes.meta.json",
            &meta,
            ODBL,
            "files limited to the fixture",
        )?;
        self.zones(site)?;
        for f in [
            "build_report.json",
            "no_network.geojson",
            "analysed_area.geojson",
            "coverage.meta.json",
        ] {
            self.copy(site, "coverage", f, ARCEP)?;
        }
        // Huts: only the fields the tests read (allow-lists below); free texts (access,
        // remarks, owner, comments and their authors, descriptions) and contacts (phone,
        // e-mail, website) are third-party personal data, never copied.
        for (f, licence, keep, keep_tags) in [
            (
                "refuges_info.geojson",
                CC_BY_SA,
                REFUGES_INFO_FIELDS,
                &[][..],
            ),
            ("camptocamp.geojson", CC_BY_SA, CAMPTOCAMP_FIELDS, &[][..]),
            ("osm.geojson", ODBL, OSM_HUT_FIELDS, OSM_HUT_TAGS),
        ] {
            let mut fc = self.read_json(site, "refuges", f)?;
            for feat in fc["features"].as_array_mut().into_iter().flatten() {
                if let Some(p) = feat["properties"].as_object_mut() {
                    p.retain(|k, _| keep.contains(&k.as_str()));
                    if let Some(t) = p.get_mut("tags").and_then(Value::as_object_mut) {
                        t.retain(|k, _| keep_tags.contains(&k.as_str()));
                    }
                }
            }
            self.write_json(
                site,
                "refuges",
                f,
                &fc,
                licence,
                "fields needed by the tests only",
            )?;
        }
        let mut meta = self.read_json(site, "refuges", "refuges.meta.json")?;
        // The documented properties are those kept.
        for f in meta["files"].as_array_mut().into_iter().flatten() {
            let keep = match f["name"].as_str() {
                Some("refuges_info.geojson") => REFUGES_INFO_FIELDS,
                Some("camptocamp.geojson") => CAMPTOCAMP_FIELDS,
                Some("osm.geojson") => OSM_HUT_FIELDS,
                _ => continue,
            };
            if let Some(p) = f["properties"].as_object_mut() {
                p.retain(|k, _| keep.contains(&k.as_str()));
            }
        }
        let meta = keep_files(
            meta,
            &["refuges_info.geojson", "camptocamp.geojson", "osm.geojson"],
        );
        self.write_json(
            site,
            "refuges",
            "refuges.meta.json",
            &meta,
            CC_BY_SA,
            "links.json left out (links between licences, personal use, L3); hut fields allow-listed (no comments, contacts, free texts)",
        )?;
        let snow = ["snow_depth.tif", "class_groomed.tif", "class_srn.tif"];
        for f in snow {
            self.copy(site, "snow", f, LO_METEOFRANCE)?;
        }
        let meta = self.read_json(site, "snow", "snow.meta.json")?;
        let meta = keep_files(meta, &snow);
        self.write_json(
            site,
            "snow",
            "snow.meta.json",
            &meta,
            LO_METEOFRANCE,
            "massif.tif (snowtools, CeCILL-C) and the depth bounds left out",
        )?;
        if site.id == CHAPELLE_DES_BOIS.id {
            self.exposure(site)?;
        }
        Ok(())
    }

    /// Zones without Biodiv'sports (no licence found, docs/licences): features, the meta
    /// statistics and the source are removed.
    fn zones(&mut self, site: &Site) -> Result<()> {
        let is_biodiv = |id: &str| id.starts_with("biodiv:");
        let src = self.layout.layer_dir(site, "zones")?.join("zones.geojson");
        let text = std::fs::read_to_string(&src)?;
        let mut layer = skimap_zones::ZoneLayer::from_geojson_str(&text)?;
        // The zones crate's own writer reproduces the pipeline file exactly.
        ensure!(
            layer.to_geojson_string()? == text,
            "{}: zones.geojson does not round-trip through the zones crate",
            site.id
        );
        let before = layer.zones.len();
        layer
            .zones
            .retain(|z| z.kind() != skimap_zones::ZoneKind::BiodivSports && !is_biodiv(z.id()));
        let kept = layer.zones.len();
        let body = layer.to_geojson_string()?;
        let rel = format!("data/out/{}/zones/zones.geojson", site.id);
        self.write(
            &rel,
            body.as_bytes(),
            licence::ZONES,
            &format!(
                "data/out/{}/zones/zones.geojson without the {} Biodiv'sports areas",
                site.id,
                before - kept
            ),
        )?;
        self.copy(site, "zones", "rules.json", licence::DERIVED)?;
        let mut meta = self.read_json(site, "zones", "zones.meta.json")?;
        if let Some(s) = meta["sources"].as_array_mut() {
            s.retain(|s| s["name"] != "Biodiv'sports");
        }
        if let Some(a) = meta["stats"]["areas_ha"].as_object_mut() {
            a.retain(|k, _| !is_biodiv(k));
        }
        if let Some(rules) = meta["stats"]["rules"].as_array_mut() {
            for r in rules.iter_mut() {
                if let Some(z) = r["zones"].as_array_mut() {
                    z.retain(|z| !z.as_str().is_some_and(is_biodiv));
                }
            }
            rules.retain(|r| r["zones"].as_array().is_some_and(|z| !z.is_empty()));
        }
        if let Some(files) = meta["files"].as_array_mut() {
            for f in files.iter_mut().filter(|f| f["name"] == "zones.geojson") {
                f["count"] = json!(kept);
            }
        }
        let meta = keep_files(meta, &["zones.geojson", "rules.json"]);
        self.write_json(
            site,
            "zones",
            "zones.meta.json",
            &meta,
            licence::ZONES,
            "Biodiv'sports removed",
        )?;
        self.lines.push(format!(
            "zones {}: {kept} of {before} zones kept (Biodiv'sports removed)",
            site.id
        ));
        Ok(())
    }

    fn exposure(&mut self, site: &Site) -> Result<()> {
        let summary = self.read_json(site, "avalanche-exposure", "summary.json")?;
        ensure!(
            summary["clpa_cells_raised"] == 0,
            "{}: the exposure raster carries CLPA cells, not redistributable",
            site.id
        );
        self.copy(
            site,
            "avalanche-exposure",
            "exposure_5m.tif",
            licence::DERIVED,
        )?;
        self.copy(site, "avalanche-exposure", "summary.json", licence::DERIVED)?;
        let meta = self.read_json(site, "avalanche-exposure", "avalanche-exposure.meta.json")?;
        let meta = keep_files(meta, &["exposure_5m.tif", "summary.json"]);
        self.write_json(
            site,
            "avalanche-exposure",
            "avalanche-exposure.meta.json",
            &meta,
            licence::DERIVED,
            "files limited to the fixture",
        )
    }

    // ---- manifest ----

    fn manifest(&mut self) -> Result<()> {
        self.entries.sort_by(|a, b| a.path.cmp(&b.path));
        let mut total = 0u64;
        let mut by_licence: BTreeMap<&str, u64> = BTreeMap::new();
        let mut files = Vec::with_capacity(self.entries.len());
        for e in &self.entries {
            let bytes = std::fs::read(self.root.join(&e.path))?;
            total += bytes.len() as u64;
            *by_licence.entry(e.licence).or_default() += bytes.len() as u64;
            files.push(json!({
                "path": e.path,
                "bytes": bytes.len(),
                "sha256": hex::encode(Sha256::digest(&bytes)),
                "licence": e.licence,
                "origin": e.origin,
            }));
        }
        // No e-mail address or phone number in any text (the OSM extract decompressed).
        for e in &self.entries {
            let bytes = std::fs::read(self.root.join(&e.path))?;
            let text = if e.path.ends_with(".gz") {
                let mut s = String::new();
                std::io::Read::read_to_string(
                    &mut flate2::read::GzDecoder::new(&bytes[..]),
                    &mut s,
                )?;
                s
            } else if e.path.ends_with(".tif") {
                continue;
            } else {
                String::from_utf8(bytes).with_context(|| format!("{}: not UTF-8", e.path))?
            };
            if let Some(m) = personal_pattern(&text) {
                bail!("{}: personal data pattern {m:?}", e.path);
            }
        }
        // Personal-use sources never enter the set.
        for e in self
            .entries
            .iter()
            .filter(|e| e.path.ends_with(".meta.json"))
        {
            let v: Value = serde_json::from_slice(&std::fs::read(self.root.join(&e.path))?)?;
            for s in v["sources"].as_array().into_iter().flatten() {
                ensure!(
                    s["personal_use_only"] != true,
                    "{}: personal-use source {}",
                    e.path,
                    s["name"]
                );
            }
        }
        let m = json!({
            "about": "Reference data set of the two sites (Q1, #101), written by `skimap fixtures`; licences in LICENCES.md",
            "files": files,
            "total_bytes": total,
            "bytes_by_licence": by_licence,
        });
        let mut body = serde_json::to_vec_pretty(&m)?;
        body.push(b'\n');
        std::fs::write(self.root.join("manifest.json"), body)?;
        self.lines.push(format!(
            "manifest: {} files, {total} bytes ({:.2} MB)",
            self.entries.len(),
            total as f64 / 1e6
        ));
        Ok(())
    }
}

/// Region the pistes builder looks at for `bbox` (`extract::site_pistes`: box of the pistes
/// touching the site box, widened by its search radius). Every way with a node in it is
/// kept whole; no extra margin is needed, the rebuilt layer is compared with `data/out`.
fn region_box(planet: &Planet, bbox: &BBox) -> BBox {
    let mut b = BBox::new(f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for p in planet.set.pistes.iter().filter(|p| p.touches(bbox)) {
        for (_, q) in &p.points {
            b.south = b.south.min(q.lat);
            b.north = b.north.max(q.lat);
            b.west = b.west.min(q.lon);
            b.east = b.east.max(q.lon);
        }
    }
    let dlat = extract::REENTRY_MAX_M / M_PER_DEG_LAT;
    let dlon = dlat
        / b.south
            .abs()
            .max(b.north.abs())
            .to_radians()
            .cos()
            .max(0.05);
    BBox::new(b.south - dlat, b.west - dlon, b.north + dlat, b.east + dlon)
}

fn xml_escape(s: &str, out: &mut String) {
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\n' => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            '\t' => out.push_str("&#9;"),
            c => out.push(c),
        }
    }
}

/// Properties kept in `refuges_info.geojson`.
const REFUGES_INFO_FIELDS: &[&str] = &[
    "id",
    "name",
    "type_id",
    "type_label",
    "alt",
    "places",
    "places_mattress",
    "state",
    "stove",
    "blankets",
    "latrines",
    "water_100m",
    "forest_100m",
    "missing_wall",
    "in_france",
    "link",
    "url",
];
/// Properties kept in `camptocamp.geojson`.
const CAMPTOCAMP_FIELDS: &[&str] = &[
    "id",
    "title",
    "waypoint_type",
    "elevation",
    "capacity_staffed",
    "capacity_unstaffed",
    "custodianship",
    "country",
    "quality",
    "in_france",
    "link",
    "url",
    "licence",
    "attribution_url",
    "blanket_unstaffed",
    "gas_unstaffed",
    "heating_unstaffed",
    "mattress_unstaffed",
];
/// Properties kept in the OSM huts layer, and the OSM tags kept in them.
const OSM_HUT_FIELDS: &[&str] = &[
    "osm_type",
    "osm_id",
    "kind",
    "in_france",
    "link",
    "refuges_info_id",
    "tags",
];
const OSM_HUT_TAGS: &[&str] = &[
    "name",
    "amenity",
    "tourism",
    "building",
    "shelter_type",
    "ele",
    "capacity",
    "beds",
    "fireplace",
    "drinking_water",
    "toilets",
    "access",
    "fee",
    "ref:refuges.info",
];

/// OSM tags never copied into the extract: contacts and free texts (third-party personal
/// data). None of them is read by the pistes builder (the rebuilt layer is checked).
fn dropped_osm_tag(k: &str) -> bool {
    const KEYS: &[&str] = &[
        "phone",
        "mobile",
        "fax",
        "email",
        "contact",
        "website",
        "url",
        "addr",
        "description",
        "note",
        "fixme",
        "comment",
        "operator",
        "opening_hours",
    ];
    KEYS.iter()
        .any(|x| k == *x || k.starts_with(&format!("{x}:")) || k.ends_with(&format!(":{x}")))
}

fn tags_xml(tags: &skimap_osm::parse::Tags, out: &mut String) {
    for (k, v) in tags.0.iter().filter(|(k, _)| !dropped_osm_tag(k)) {
        out.push_str("  <tag k=\"");
        xml_escape(k, out);
        out.push_str("\" v=\"");
        xml_escape(v, out);
        out.push_str("\"/>\n");
    }
}

/// OSM XML of the kept ways, their nodes (sorted by id) and the relations reaching them
/// directly or through nested relations (file order, every member listed).
fn planet_xml(data: &OsmData, ways: &BTreeSet<i64>) -> Result<Vec<u8>> {
    use std::fmt::Write as _;
    let mut nodes: BTreeMap<i64, (f64, f64)> = BTreeMap::new();
    for w in data.ways.iter().filter(|w| ways.contains(&w.id)) {
        for &n in &w.nodes {
            if let Some(p) = data.node(n) {
                nodes.insert(n, (p.lat, p.lon));
            }
        }
    }
    let mut rels: HashSet<i64> = HashSet::new();
    loop {
        let before = rels.len();
        for r in &data.relations {
            if r.members.iter().any(|m| match m.kind {
                MemberKind::Way => ways.contains(&m.id),
                MemberKind::Relation => rels.contains(&m.id),
                MemberKind::Node => false,
            }) {
                rels.insert(r.id);
            }
        }
        if rels.len() == before {
            break;
        }
    }
    let mut s = String::with_capacity(1 << 22);
    s.push_str("<?xml version='1.0' encoding='UTF-8'?>\n<osm version=\"0.6\" generator=\"skimap fixtures\">\n");
    for (id, (lat, lon)) in &nodes {
        writeln!(s, " <node id=\"{id}\" lat=\"{lat:.7}\" lon=\"{lon:.7}\"/>")?;
    }
    for w in data.ways.iter().filter(|w| ways.contains(&w.id)) {
        writeln!(s, " <way id=\"{}\">", w.id)?;
        for n in &w.nodes {
            writeln!(s, "  <nd ref=\"{n}\"/>")?;
        }
        tags_xml(&w.tags, &mut s);
        s.push_str(" </way>\n");
    }
    for r in data.relations.iter().filter(|r| rels.contains(&r.id)) {
        writeln!(s, " <relation id=\"{}\">", r.id)?;
        for m in &r.members {
            let kind = match m.kind {
                MemberKind::Node => "node",
                MemberKind::Way => "way",
                MemberKind::Relation => "relation",
            };
            write!(s, "  <member type=\"{kind}\" ref=\"{}\" role=\"", m.id)?;
            xml_escape(&m.role, &mut s);
            s.push_str("\"/>\n");
        }
        tags_xml(&r.tags, &mut s);
        s.push_str(" </relation>\n");
    }
    s.push_str("</osm>\n");
    Ok(s.into_bytes())
}

/// Reads the planet extract of the fixtures as `extract::load_planet` reads the full one.
pub fn read_planet(path: &Path, state: &Path) -> Result<Planet> {
    let data = skimap_osm::parse::read_file(path, &extract::france_aoi())?;
    let set = skimap_osm::pistes::classify(&data);
    let valid_at = std::fs::read_to_string(state)?.trim().to_owned();
    Ok(Planet {
        data,
        set,
        source: skimap_common::meta::SourceRef::new(
            "OpenSnowMap planet_pistes.osm.gz (fixtures)",
            extract::PLANET_URL,
            extract::LICENCE,
            extract::ATTRIBUTION,
        ),
        valid_at: Some(valid_at),
    })
}

/// Keeps in a layer meta only the files listed in `names`, with a note.
fn keep_files(mut meta: Value, names: &[&str]) -> Value {
    if let Some(files) = meta["files"].as_array_mut() {
        files.retain(|f| f["name"].as_str().is_some_and(|n| names.contains(&n)));
    }
    if let Some(notes) = meta["notes"].as_array_mut() {
        notes.push(json!(
            "Jeu de référence des tests (fixtures/, skimap fixtures) : seuls les fichiers listés sont repris."
        ));
    }
    meta
}

fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        if e.file_type()?.is_file() {
            std::fs::copy(e.path(), to.join(e.file_name()))?;
        }
    }
    Ok(())
}
