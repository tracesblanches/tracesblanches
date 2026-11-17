//! Every layer of a site the assistant uses, loaded once ([`SiteData::load`]).
//!
//! By default each layer is read from its directory `data/out/<site>/<layer>/`; [`Paths`]
//! overrides the files that change with the date (weather, snow map, BERA, status), so
//! that past days can be replayed from archived copies (`data/out/<site>/assist/history/`).
//! A missing layer is not an error: it is listed in [`SiteData::missing`] and every output
//! says what it could not use.

use std::path::{Path, PathBuf};

use chrono::NaiveDate;
use skimap_common::grid::Grid;
use skimap_common::layout::DataLayout;
use skimap_common::meta::SourceRef;
use skimap_common::site::Site;

use crate::error::Result;
use crate::layers::{
    Areas, Bera, Clpa, Domain, Piste, Route, SnowMap, Status, Weather, layer_sources, load_domains,
    load_pistes, load_zones,
};
use crate::text::Lang;

/// Oldest snow map used for a day, days (snowfall since the map is added on top).
pub const MAX_SNOW_AGE_DAYS: i64 = 4;

/// Files to read instead of the current layers.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Paths {
    /// `hourly.json` files, history first; empty: `meteo-hourly/hourly.json`.
    pub meteo: Vec<PathBuf>,
    /// Snow layer directories (one per date); empty: `snow/`.
    pub snow_dirs: Vec<PathBuf>,
    /// `bera.json` files (one per date); empty: `bera/bera.json`.
    pub beras: Vec<PathBuf>,
    /// Status layer directory; `None`: `status/`.
    pub status_dir: Option<PathBuf>,
    /// Planned route (GeoJSON with a LineString).
    pub route: Option<PathBuf>,
}

/// The layers of a site.
#[derive(Debug, Clone)]
pub struct SiteData {
    /// Site.
    pub site: Site,
    /// Hourly weather.
    pub weather: Option<Weather>,
    /// Snow maps, oldest first.
    pub snows: Vec<SnowMap>,
    /// BERA files, one per date.
    pub beras: Vec<Bera>,
    /// Status of the pistes, with the geometry of each itinerary.
    pub status: Option<Status>,
    /// Nordic domains touching the site.
    pub domains: Vec<Domain>,
    /// OSM pistes of the site.
    pub pistes: Vec<Piste>,
    /// Historic avalanche paths.
    pub clpa: Option<Clpa>,
    /// Zones without any network.
    pub no_network: Option<Areas>,
    /// Zones and rules.
    pub zones: Option<(skimap_zones::ZoneLayer, skimap_zones::RuleSet)>,
    /// Planned route.
    pub route: Option<Route>,
    /// Trail graph with elevation profiles (`routes/piste_graph.json`, C1).
    pub graph: Option<skimap_core::graph::PisteGraph>,
    /// Avalanche exposure (`avalanche-exposure/exposure_5m.tif`, P16–P18): 1 terrain peu
    /// exposé (modèle), 2 exposé, 3 très exposé; Lambert-93, 5 m.
    pub exposure: Option<Grid<u8>>,
    /// Layers that could not be read, with the reason.
    pub missing: Vec<String>,
    /// Sources of the layers read (from their `<layer>.meta.json`).
    pub sources: Vec<SourceRef>,
    /// Files read for the date-dependent layers.
    pub files: Vec<String>,
}

/// A layer of the assistant, as named in the missing layers (VO6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerName {
    /// Hourly weather.
    Weather,
    /// Snow map.
    Snow,
    /// Avalanche bulletin.
    Bera,
    /// Trail status.
    Status,
    /// Nordic areas.
    Domains,
    /// Trails.
    Pistes,
    /// Historic avalanche paths.
    Clpa,
    /// Zones without network.
    NoNetwork,
    /// Zones and rules.
    Zones,
    /// Planned route.
    Route,
    /// Trail graph.
    Graph,
    /// Avalanche exposure.
    Exposure,
}

impl LayerName {
    /// Every layer.
    pub const ALL: [LayerName; 12] = [
        LayerName::Weather,
        LayerName::Snow,
        LayerName::Bera,
        LayerName::Status,
        LayerName::Domains,
        LayerName::Pistes,
        LayerName::Clpa,
        LayerName::NoNetwork,
        LayerName::Zones,
        LayerName::Route,
        LayerName::Graph,
        LayerName::Exposure,
    ];
}

fn keep<T>(r: Result<T>, what: LayerName, missing: &mut Vec<String>) -> Option<T> {
    let what = crate::text::of(Lang::Fr).layer_name(what);
    match r {
        Ok(v) => Some(v),
        Err(e) => {
            missing.push(format!("{what} : {e}"));
            None
        }
    }
}

impl SiteData {
    /// Snow map for a day: the latest one of that day or before, at most 4 days old.
    pub fn snow_for(&self, date: NaiveDate) -> Option<&SnowMap> {
        self.snows
            .iter()
            .rev()
            .find(|m| m.date() <= date && (date - m.date()).num_days() <= MAX_SNOW_AGE_DAYS)
    }

    /// BERA of a day (the bulletin date must match).
    pub fn bera_for(&self, date: NaiveDate) -> Option<&Bera> {
        self.beras.iter().find(|b| b.date() == Some(date))
    }

    /// Avalanche exposure class at a position (1 to 3), when the layer covers it.
    pub fn exposure_at(&self, p: skimap_common::geom::LonLat) -> Option<u8> {
        let g = self.exposure.as_ref()?;
        let (x, y) = skimap_common::crs::to_lambert93(p).ok()?;
        let v = *g.value_at(x, y)?;
        (1..=3).contains(&v).then_some(v)
    }

    /// The layers that could not be read ([`SiteData::missing`], French) with their name
    /// in `lang`; the reason (reader error, local path) stays as given.
    pub fn missing_in(&self, lang: Lang) -> Vec<String> {
        let fr = crate::text::of(Lang::Fr);
        let to = crate::text::of(lang);
        self.missing
            .iter()
            .map(|m| {
                for l in LayerName::ALL {
                    let name = fr.layer_name(l);
                    if m == name {
                        return to.layer_name(l).to_owned();
                    }
                    if let Some(rest) = m.strip_prefix(name).and_then(|r| r.strip_prefix(" : ")) {
                        return to.layer_missing(l, rest);
                    }
                }
                m.clone()
            })
            .collect()
    }

    /// Whether the site's massif has no BERA (Jura), from the layer or the site table.
    pub fn no_bera(&self) -> bool {
        !self.site.has_bera
            || self
                .beras
                .iter()
                .any(|b| b.status == crate::layers::BeraStatus::NoBera)
    }

    /// Loads every layer of `site` (missing layers are noted, not fatal).
    pub fn load(layout: &DataLayout, site: &Site, paths: &Paths) -> Result<Self> {
        let dir = |layer: &str| -> Result<PathBuf> { Ok(layout.layer_dir(site, layer)?) };
        let mut missing = Vec::new();
        let mut sources = Vec::new();
        let mut files = Vec::new();
        let mut add_sources = |d: &Path, layer: &str| {
            for s in layer_sources(d, layer) {
                if !sources
                    .iter()
                    .any(|x: &SourceRef| x.url == s.url && x.name == s.name)
                {
                    sources.push(s);
                }
            }
        };

        let meteo_files = if paths.meteo.is_empty() {
            vec![dir("meteo-hourly")?.join("hourly.json")]
        } else {
            paths.meteo.clone()
        };
        add_sources(&dir("meteo-hourly")?, "meteo-hourly");
        for f in &meteo_files {
            if let Some(parent) = f.parent() {
                add_sources(parent, "meteo-hourly");
            }
            files.push(f.display().to_string());
        }
        let weather = keep(
            Weather::load(&meteo_files),
            LayerName::Weather,
            &mut missing,
        );

        let snow_dirs = if paths.snow_dirs.is_empty() {
            vec![dir("snow")?]
        } else {
            paths.snow_dirs.clone()
        };
        let mut snows = Vec::new();
        for d in &snow_dirs {
            add_sources(d, "snow");
            files.push(d.display().to_string());
            snows.extend(keep(SnowMap::load(d), LayerName::Snow, &mut missing));
        }
        snows.sort_by_key(|m| m.valid_at);

        let bera_paths = if paths.beras.is_empty() {
            vec![dir("bera")?.join("bera.json")]
        } else {
            paths.beras.clone()
        };
        let mut beras = Vec::new();
        for p in &bera_paths {
            if let Some(parent) = p.parent() {
                add_sources(parent, "bera");
            }
            files.push(p.display().to_string());
            beras.extend(keep(Bera::load(p), LayerName::Bera, &mut missing));
        }

        let status_dir = paths.status_dir.clone().unwrap_or(dir("status")?);
        add_sources(&status_dir, "status");
        files.push(status_dir.display().to_string());
        let status = keep(Status::load(&status_dir), LayerName::Status, &mut missing);

        let ddir = dir("domaines")?;
        add_sources(&ddir, "domaines");
        let domains =
            keep(load_domains(&ddir), LayerName::Domains, &mut missing).unwrap_or_default();
        let pdir = dir("pistes")?;
        add_sources(&pdir, "pistes");
        let pistes = keep(
            load_pistes(&pdir.join("pistes.geojson")),
            LayerName::Pistes,
            &mut missing,
        )
        .unwrap_or_default();
        let adir = dir("avalanche-historic")?;
        add_sources(&adir, "avalanche-historic");
        let clpa = keep(Clpa::load(&adir), LayerName::Clpa, &mut missing);
        let cdir = dir("coverage")?;
        add_sources(&cdir, "coverage");
        let no_network = keep(
            Areas::load(&cdir.join("no_network.geojson")),
            LayerName::NoNetwork,
            &mut missing,
        );
        let zdir = dir("zones")?;
        add_sources(&zdir, "zones");
        let zones = keep(load_zones(&zdir), LayerName::Zones, &mut missing);
        let route = match &paths.route {
            Some(p) => {
                files.push(p.display().to_string());
                keep(Route::load(p), LayerName::Route, &mut missing)
            }
            None => None,
        };

        let rdir = dir("routes")?;
        add_sources(&rdir, "routes");
        let graph = keep(
            std::fs::read_to_string(rdir.join(skimap_core::routing::GRAPH_FILE))
                .map_err(|source| crate::error::Error::Io {
                    path: rdir.join(skimap_core::routing::GRAPH_FILE),
                    source,
                })
                .and_then(|t| {
                    skimap_core::graph::PisteGraph::from_json(&t)
                        .map_err(|e| crate::error::Error::format(rdir.join("piste_graph.json"), e))
                }),
            LayerName::Graph,
            &mut missing,
        );
        let edir = dir("avalanche-exposure")?;
        add_sources(&edir, "avalanche-exposure");
        let exposure = keep(
            // UInt8 classes, Lambert-93, no data 255 (`avalanche-exposure`).
            skimap_common::geotiff::read_u8(&edir.join("exposure_5m.tif"))
                .map_err(crate::error::Error::from),
            LayerName::Exposure,
            &mut missing,
        );

        Ok(SiteData {
            site: *site,
            weather,
            snows,
            beras,
            status,
            domains,
            pistes,
            clpa,
            no_network,
            zones,
            route,
            graph,
            exposure,
            missing,
            sources,
            files,
        })
    }
}
