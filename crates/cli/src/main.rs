//! `skimap`: command-line entry point of the pipeline and of the core algorithms.
//!
//! Owned tickets: F1 (#1) skeleton, F4 (#4) Android arm64 build. Owned by the
//! orchestrator: each subcommand calls the public builder of the crate of its ticket; the
//! remaining placeholders name the ticket that will provide it.

mod assist;
mod batch;
mod build;
mod fixtures;
mod france;
mod history;
mod plan;
mod refresh;
mod relief;
mod route;
/// Test support of the reference data set (Q1, Q2), shared with the other crates.
#[cfg(test)]
#[path = "../../../fixtures/support/mod.rs"]
mod test_fixtures;
mod voice;

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use skimap_common::config::Config;
use skimap_common::crs::{Crs, project_bbox};
use skimap_common::download::{CachePolicy, Downloader, Request};
use skimap_common::layout::DataLayout;
use skimap_common::site::{MassifStatus, Registry, Site};

#[derive(Parser, Debug)]
#[command(
    name = "skimap",
    version,
    about = "Cartes hors ligne de ski de fond et de SRN : pipeline et algorithmes"
)]
struct Cli {
    /// Data directory (default: $SKIMAP_DATA_DIR or ./data).
    #[arg(long, global = true)]
    data_dir: Option<PathBuf>,

    /// More logs (-v info, -vv debug). RUST_LOG takes precedence.
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    verbose: u8,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// List the sites of the registry (every census domain once `skimap domaines` has run,
    /// otherwise the evaluation sites).
    Sites {
        /// JSON output.
        #[arg(long)]
        json: bool,
        /// Identifiers only, one per line, of the sites that are built (no parent of
        /// sections), filtered by the options below (`skimap-job bera`, `status`).
        #[arg(long, conflicts_with = "json")]
        ids: bool,
        /// With `--ids`: only the sites whose main massif has a BERA.
        #[arg(long, requires = "ids")]
        bera: bool,
        /// With `--ids`: only the groomed stations with a readable trail status source
        /// (Nordic France widget or ENJ city).
        #[arg(long, requires = "ids")]
        status: bool,
        /// With `--ids`: only the sites whose pipeline outputs are in the data directory
        /// (`out/<site>/build-all.json`).
        #[arg(long, requires = "ids")]
        built: bool,
    },
    /// Download one URL through the shared cache and rate limiter (P1, #14).
    FetchUrl {
        /// URL to fetch (GET).
        url: String,
        /// Source name (rate limits and cache directory), e.g. `geo-api`.
        #[arg(long)]
        source: String,
        /// Cache policy.
        #[arg(long, value_enum, default_value_t = CacheMode::Normal)]
        cache: CacheMode,
        /// Override of the cache lifetime, in seconds.
        #[arg(long)]
        max_age: Option<u64>,
        /// Print the body on stdout.
        #[arg(long)]
        print: bool,
    },
    /// Nordic trails and routing graph of a site from OpenSnowMap (P2, #15).
    Pistes(SiteArg),
    /// Census of the French cross-country ski areas, `data/out/france/domaines/`, the site
    /// registry `data/out/france/sites/sites.json` (one site per domain, G2) and the
    /// domain layer of the given sites (P3, #16).
    Domaines {
        /// Sites whose domain layer is written (default: every site of the registry).
        sites: Vec<String>,
    },
    /// SRN areas of France outside groomed domains, `data/out/france/srn/` (G5, #131);
    /// needs the census (`skimap domaines`).
    Srn,
    /// Station cards (rescue, sale points, contacts) and access titles of France (H1 #132,
    /// H2 #133), `data/out/france/stations/`: every part now; needs the census. Titles,
    /// prices and validity zones only with `SKIMAP_PERSONAL_SOURCES`.
    Stations,
    /// Trail starts of every census domain (X9, #143), `data/out/france/stations/departs.json`,
    /// from the points of the stations layer and the trail network; then the starts of the
    /// site registry. Run by `skimap stations` and `skimap refresh` too when the station
    /// cards ran.
    Departs,
    /// Scheduled refresh (H1, H2): the due parts of the stations layer, then the hut layer
    /// of each site; idempotent (a second run the same day does nothing); summary in
    /// `data/out/france/refresh/status.json`, non-zero exit when a source is broken.
    Refresh {
        /// Sites whose hut layer is refreshed (default: every site with a `refuges` layer).
        sites: Vec<String>,
    },
    /// Publishable hourly weather (Météo-France AROME 0.025°, X17) of these sites, or of
    /// every site of the registry: one download and decoding of the latest run for all of
    /// them; a site already at that run is not rewritten (idempotent).
    MeteoRefresh {
        /// Sites (default: every buildable site of the registry).
        sites: Vec<String>,
        /// AROME run to use (RFC 3339, e.g. `2026-11-17T06:00:00Z`; default: the latest
        /// complete run): replay of a run still published, measurements.
        #[arg(long)]
        run: Option<String>,
    },
    /// Catalogue of France, `data/out/france/catalog.json` and `catalog.geojson`: every
    /// site with its box, centre, km and the state of its pack (G2).
    Catalog,
    /// IGN LiDAR HD terrain model, slope and aspect of a site (P5, #18).
    Terrain(SiteArg),
    /// Estimated snow cover of a site (P6, #19); needs the terrain tiles.
    Snow {
        /// Site identifier.
        site: String,
        /// Simulation date YYYY-MM-DD (default: latest date of the current year, J-1).
        #[arg(long)]
        date: Option<String>,
    },
    /// Validation of the snow estimate against the Météo-France stations (P7, #20).
    SnowValidate {
        /// Site identifier.
        site: String,
        /// First and last season (start years), e.g. `2020 2025`.
        #[arg(long, num_args = 2, value_names = ["FIRST", "LAST"], default_values_t = [2020, 2025])]
        seasons: Vec<i32>,
    },
    /// Huts and shelters of a site: refuges.info, OSM, Camptocamp (P11, #24).
    Refuges(SiteArg),
    /// Protected areas and their periods, with the rule file (P9 #22, P10 #23).
    Zones(SiteArg),
    /// Areas without any mobile network, even for 112 (P12, #25).
    Coverage {
        /// Site identifier.
        site: String,
        /// ARCEP release `YYYY_TQ` (default: latest known).
        #[arg(long)]
        release: Option<String>,
        /// Directory holding already downloaded ARCEP archives (`.7z`).
        #[arg(long)]
        archives: Option<PathBuf>,
    },
    /// Avalanche bulletin (BERA) of the massif of a site (P8, #21).
    Bera {
        /// Site identifier.
        site: String,
        /// Bulletin date YYYY-MM-DD (default: today, UTC).
        #[arg(long)]
        date: Option<String>,
    },
    /// Hourly weather of a site (P19, #32); personal use only (`SKIMAP_PERSONAL_SOURCES`).
    Meteo(SiteArg),
    /// Archives the weather (historical forecast, personal use only), snow maps and BERA
    /// of past days for the assistant, `assist/history/`.
    History {
        /// Site identifier.
        site: String,
        /// First date, YYYY-MM-DD (the weather starts three days before).
        first: String,
        /// Last date, YYYY-MM-DD.
        last: String,
        /// Skip the snow maps.
        #[arg(long)]
        no_snow: bool,
        /// Skip the BERA.
        #[arg(long)]
        no_bera: bool,
    },
    /// One poll of the trail status sources of a site (P13, #26); personal use only.
    StatusPoll(SiteArg),
    /// Historic avalanche footprints (CLPA) and the piste sections crossing them (P20, #99);
    /// needs the `pistes` layer of the site.
    AvalancheHistoric {
        /// Site identifier.
        site: String,
        /// Read the pistes from a saved Overpass extract instead (report 07).
        #[arg(long)]
        dump: Option<PathBuf>,
    },
    /// Zones and rules active at a position and a date (C8, #40); needs the `zones` layer.
    ZonesAt {
        /// Site identifier.
        site: String,
        /// Latitude, WGS84 degrees.
        #[arg(allow_hyphen_values = true)]
        lat: f64,
        /// Longitude, WGS84 degrees.
        #[arg(allow_hyphen_values = true)]
        lon: f64,
        /// `YYYY-MM-DD` (noon), `YYYY-MM-DDTHH:MM` in French legal time, or RFC 3339.
        date: String,
    },
    /// Avalanche exposure of a site (P16 #29, P17 #30, P18 #31); needs the `terrain` and
    /// `pistes` layers, downloads the IGN LiDAR HD canopy height once.
    AvalancheExposure(SiteArg),
    /// Trail graph with elevation profiles, `routes/piste_graph.json` (C1, #33); needs the
    /// `pistes` and `terrain` layers.
    Routing(SiteArg),
    /// Off-piste rasters of a site, `offpiste/` (C3, #35); needs `terrain`, `zones`,
    /// `pistes` and a snow layer.
    OffpistePrepare {
        /// Site identifier.
        site: String,
        /// Snow layer directories (default: the site's `snow` layer); repeatable. The layer
        /// keeps at most 7 dates: these, then the most recent ones of the previous run.
        #[arg(long = "snow-dir")]
        snow_dirs: Vec<PathBuf>,
    },
    /// Route on the trails between two points: length, climb, estimated time (C1 #33,
    /// C5 #37), turnaround before nightfall with `--start` (C7 #39); writes a GeoJSON.
    Route {
        /// Site identifier.
        site: String,
        /// Start, `lat,lon` (WGS84 degrees).
        #[arg(allow_hyphen_values = true)]
        from: String,
        /// End, `lat,lon`.
        #[arg(allow_hyphen_values = true)]
        to: String,
        /// Technique: classic or skating.
        #[arg(long, default_value = "classic")]
        style: String,
        /// Date of the outing YYYY-MM-DD (default: today), for `--start HH:MM`.
        #[arg(long)]
        date: Option<String>,
        /// Start time: `HH:MM` (French legal time) or date and time; gives the turnaround.
        #[arg(long)]
        start: Option<String>,
        /// Largest distance from a point to the trails, metres.
        #[arg(long, default_value_t = 500.0)]
        max_snap: f64,
        /// GeoJSON written.
        #[arg(long, default_value = "route.geojson")]
        out: PathBuf,
    },
    /// Voice clips of the pack of a site (VO3, #85) with the names of its built layers
    /// (`pistes`, `refuges`, `zones`); Pocket TTS on this workstation. One layer per
    /// language (VO6): `voice/` (fr), `voice-en/` (en).
    Voice {
        /// Site identifier.
        site: String,
        /// Language of the clips: `fr` or `en`.
        #[arg(long, default_value = "fr")]
        lang: String,
    },
    /// Assistant of a site (A1 to A9): briefing, wax, snow quality, synthesis, suggested
    /// loops, equipment, alerts, best time; `data/out/<site>/assist/`.
    Assist {
        /// Site identifier.
        site: String,
        /// First day YYYY-MM-DD (default: today).
        #[arg(long)]
        date: Option<String>,
        /// Days after the first one.
        #[arg(long, default_value_t = 3)]
        days: u32,
        /// Home point `lon,lat`, for the distances of the briefing.
        #[arg(long, allow_hyphen_values = true)]
        home: Option<String>,
        /// Planned route (GeoJSON with a LineString).
        #[arg(long)]
        route: Option<PathBuf>,
        /// Three suggestion requests of the site (A5) and their equipment, alerts, best
        /// time.
        #[arg(long)]
        suggest: bool,
        /// Outings on the core proof routes (A6).
        #[arg(long)]
        outings: bool,
        /// Past days archived by `skimap history`: `past` or `with-forecast`.
        #[arg(long)]
        history: Option<String>,
    },
    /// Day plans of a site (J1, J2): the preset requests of the site on a day, first loop
    /// of each; `data/out/<site>/plan/`, plan text printed.
    Plan {
        /// Site identifier.
        site: String,
        /// Day YYYY-MM-DD (default: the archived day of the site).
        #[arg(long)]
        date: Option<String>,
        /// Only this request of the site, numbered from 1 (default: all).
        #[arg(long)]
        request: Option<usize>,
    },
    /// Every available builder of a site, in dependency order, timed; with `--all` or
    /// `--domains`, many sites in parallel (resumable, log per site, summary, catalogue).
    BuildAll(BuildAllArgs),
    /// Fetch every source of a site (P2, P5, P6, P8, P9, P11, P12, P13, P19).
    Fetch(Placeholder),
    /// Build the offline pack of a site from the pipeline outputs (P14, #27).
    BuildPack(SiteArg),
    /// Voice packs of a site only (VO6): one per language whose voice layer is built,
    /// `data/out/<site>/voice-pack/<lang>/`, published apart from the base pack.
    VoicePack(SiteArg),
    /// Serve a directory over HTTP with range requests, for the pack viewer (P15, #28).
    Serve {
        /// Root directory (the repository root for `tools/viewer/`).
        #[arg(default_value = ".")]
        root: PathBuf,
        /// Address.
        #[arg(long, default_value = "127.0.0.1:8765")]
        addr: String,
    },
    /// Estimated time of a route (C5, #37).
    Eta(Placeholder),
    /// Export the pack of a site for Locus Map and OsmAnd (P21, #100).
    Export(SiteArg),
    /// Daily briefing of a site (A1, #88).
    Briefing(Placeholder),
    /// Reference data set of the tests from `data/cache` and `data/out` (Q1, #101):
    /// redistributable sources only, checked against the full data, with its manifest.
    Fixtures {
        /// Fixtures root (the repository's `fixtures/`).
        #[arg(long, default_value = "fixtures")]
        out: PathBuf,
    },
}

/// A site identifier.
#[derive(clap::Args, Debug)]
struct SiteArg {
    /// Site identifier (`skimap sites`).
    site: String,
}

/// Arguments of `build-all`.
#[derive(clap::Args, Debug)]
struct BuildAllArgs {
    /// Site identifier (one site, steps printed).
    #[arg(conflicts_with_all = ["all", "domains"], required_unless_present_any = ["all", "domains"])]
    site: Option<String>,
    /// Every site of the registry.
    #[arg(long)]
    all: bool,
    /// These sites, comma separated.
    #[arg(long, value_delimiter = ',')]
    domains: Vec<String>,
    /// Sites built in parallel (network sources are shared: one request in flight per
    /// source for all of them).
    #[arg(long, default_value_t = 4)]
    jobs: usize,
    /// Rebuild the sites that are up to date, and every part of the stations layer.
    #[arg(long)]
    force: bool,
    /// A successful build older than this is redone, hours.
    #[arg(long, default_value_t = 24.0)]
    max_age_hours: f64,
    /// Start no site after this many minutes (running ones finish; run again to resume).
    #[arg(long)]
    stop_after_min: Option<f64>,
    /// Also check the outputs of the sites that look up to date (fingerprints recorded
    /// in `build-all.json`): an altered, removed or half-written output is rebuilt. In
    /// every case only the failed, older than `--max-age-hours`, altered steps and those
    /// depending on them are run again.
    #[arg(long)]
    stale: bool,
}

/// Arguments of a subcommand not implemented yet.
#[derive(clap::Args, Debug)]
struct Placeholder {
    /// Arguments, defined by the ticket that implements the subcommand.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    args: Vec<String>,
}

#[derive(ValueEnum, Clone, Copy, Debug)]
enum CacheMode {
    Normal,
    Revalidate,
    Reload,
    Offline,
}

impl From<CacheMode> for CachePolicy {
    fn from(m: CacheMode) -> Self {
        match m {
            CacheMode::Normal => CachePolicy::Normal,
            CacheMode::Revalidate => CachePolicy::Revalidate,
            CacheMode::Reload => CachePolicy::Reload,
            CacheMode::Offline => CachePolicy::Offline,
        }
    }
}

fn init_tracing(verbose: u8) {
    let default = match verbose {
        0 => "warn",
        1 => "info",
        _ => "debug",
    };
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(default));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();
}

fn site_json(s: &Site) -> Result<serde_json::Value> {
    let mut v = serde_json::to_value(s)?;
    let (x0, y0, x1, y1) = project_bbox(s.bbox, Crs::Lambert93, 32)?;
    v["bbox_lambert93"] = serde_json::json!({ "min_x": x0, "min_y": y0, "max_x": x1, "max_y": y1 });
    Ok(v)
}

/// Selection of `skimap sites --ids`.
struct SiteFilter {
    /// Main massif with a BERA (the Jura notice does not change: `build-all` writes it).
    bera: bool,
    /// Groomed station with a readable trail status source.
    status: bool,
    /// Pipeline outputs in this data directory (`build-all` record).
    built: Option<DataLayout>,
}

impl SiteFilter {
    /// Buildable sites of the registry that pass the filter, in registry order.
    fn select(&self, registry: &Registry) -> Vec<&'static Site> {
        registry
            .buildable()
            .into_iter()
            .filter(|s| !self.bera || s.has_bera)
            .filter(|s| !self.status || (s.kind.has_groomed_trails() && !s.status.is_empty()))
            .filter(|s| {
                self.built
                    .as_ref()
                    .is_none_or(|l| l.out_dir(s).join(france::BUILD_RECORD).is_file())
            })
            .collect()
    }
}

fn print_sites(registry: &Registry, json: bool) -> Result<()> {
    if json {
        let all = registry
            .all()
            .iter()
            .map(|s| site_json(s))
            .collect::<Result<Vec<_>>>()?;
        println!("{}", serde_json::to_string_pretty(&all)?);
        return Ok(());
    }
    println!(
        "{} sites ({}), {} construits par build-all (les autres en tronçons)",
        registry.all().len(),
        registry.source().map_or(
            "sites d'évaluation : `skimap domaines` pour toute la France".to_owned(),
            |p| p.display().to_string()
        ),
        registry.buildable().len()
    );
    for (s, r) in registry.all().iter().zip(registry.records()) {
        let (w, h) = s.bbox.size_m();
        let (x0, y0, x1, y1) = project_bbox(s.bbox, Crs::Lambert93, 32)?;
        let massifs = s
            .massifs
            .iter()
            .map(|m| format!("{} {}", m.code, m.name))
            .collect::<Vec<_>>()
            .join(" / ");
        let status = match s.massif_status {
            MassifStatus::Verified => "vérifié sur les polygones snowtools",
            MassifStatus::Assumed => "supposé",
            MassifStatus::ToConfirm => "à confirmer",
            MassifStatus::Outside => "hors des massifs snowtools",
        };
        println!(
            "{:<34} {} ({}, {}, INSEE {}){}",
            s.id,
            s.name,
            s.kind.label_fr(),
            s.department,
            s.insee,
            r.km_nordic
                .map_or(String::new(), |k| format!(", {k} km nordiques"))
        );
        println!(
            "  emprise WGS84 s,w,n,e : {}  ({:.1} x {:.1} km)",
            s.bbox.overpass(),
            w / 1000.0,
            h / 1000.0
        );
        println!("  emprise Lambert-93    : x {x0:.0}..{x1:.0}, y {y0:.0}..{y1:.0}");
        println!(
            "  massif Météo-France   : {massifs} ({status}) ; BERA : {}",
            if s.has_bera { "oui" } else { "non" }
        );
        if !r.sections.is_empty() {
            println!("  tronçons              : {}", r.sections.join(", "));
        }
    }
    Ok(())
}

fn fetch_url(
    cfg: &Config,
    url: &str,
    source: &str,
    cache: CacheMode,
    max_age: Option<u64>,
    print: bool,
) -> Result<()> {
    let dl = Downloader::from_config(cfg)?;
    let mut req = Request::get(source, url).cache(cache.into());
    if let Some(s) = max_age {
        req = req.max_age(Duration::from_secs(s));
    }
    let f = dl.fetch(&req).with_context(|| format!("fetching {url}"))?;
    if print {
        use std::io::Write;
        std::io::stdout().write_all(&f.bytes()?)?;
    } else {
        println!(
            "{:?} {} octets -> {} (ETag {:?}, sha256 {})",
            f.origin,
            f.meta.size,
            f.path.display(),
            f.meta.etag,
            f.meta.sha256
        );
    }
    Ok(())
}

fn not_implemented(name: &str, tickets: &str) -> Result<()> {
    anyhow::bail!("`{name}` n'est pas encore implémenté (ticket(s) {tickets})")
}

fn build_all(cfg: &Config, registry: &Registry, a: BuildAllArgs) -> Result<()> {
    let c = build::Ctx::new(cfg)?;
    if let Some(id) = &a.site {
        return batch::build_single(
            &c,
            registry,
            registry.get(id)?,
            a.force,
            Duration::from_secs_f64(a.max_age_hours.max(0.0) * 3600.0),
        );
    }
    let sites: Vec<&'static Site> = if a.all {
        registry.buildable()
    } else {
        a.domains
            .iter()
            .map(|s| Ok(registry.get(s.trim())?))
            .collect::<Result<_>>()?
    };
    let opts = batch::BatchOptions {
        jobs: a.jobs.max(1),
        force: a.force,
        max_age: Duration::from_secs_f64(a.max_age_hours.max(0.0) * 3600.0),
        stop_after: a
            .stop_after_min
            .map(|m| Duration::from_secs_f64(m.max(0.0) * 60.0)),
        verify: a.stale,
    };
    let summary = batch::build_many(&c, registry, &sites, &opts)?;
    for l in batch::describe(&summary) {
        println!("{l}");
    }
    let cat = france::write_catalog(&c.layout, registry)?;
    println!(
        "catalogue : {} ({} sites, {:?})",
        cat.path.display(),
        cat.sites,
        cat.by_state
    );
    if summary.failed > 0 {
        anyhow::bail!("build-all : {} sites en échec", summary.failed);
    }
    if summary.stations.state == build::StepState::Failed {
        anyhow::bail!("build-all : stations en échec : {}", summary.stations.line);
    }
    Ok(())
}

/// Trail starts of every domain (X9), then the starts of the registry.
fn departs(c: &build::Ctx) -> Result<()> {
    let t = std::time::Instant::now();
    let out = skimap_stations::departs::build(&c.layout, &c.dl)?;
    let n = france::refresh_registry_starts(&c.layout)?;
    println!(
        "départs -> {} ({:.1} s) : {} départs, domaines par méthode du départ principal {:?} ; {n} sites du registre avec départs",
        out.path.display(),
        t.elapsed().as_secs_f64(),
        out.starts,
        out.by_method
    );
    Ok(())
}

fn print_json<T: serde::Serialize>(v: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(v)?);
    Ok(())
}

fn run(cli: Cli) -> Result<()> {
    let mut cfg = Config::from_env()?;
    if let Some(d) = cli.data_dir {
        cfg.data_dir = d;
    }
    let ctx = || build::Ctx::new(&cfg);
    let registry = Registry::load(&cfg.layout())?;
    let site = |id: &str| -> Result<&'static Site> { Ok(registry.get(id)?) };
    match cli.command {
        Command::Sites {
            json,
            ids,
            bera,
            status,
            built,
        } => {
            if !ids {
                return print_sites(&registry, json);
            }
            let filter = SiteFilter {
                bera,
                status,
                built: built.then(|| cfg.layout()),
            };
            for s in filter.select(&registry) {
                println!("{}", s.id);
            }
            Ok(())
        }
        Command::FetchUrl {
            url,
            source,
            cache,
            max_age,
            print,
        } => fetch_url(&cfg, &url, &source, cache, max_age, print),
        Command::Pistes(a) => {
            let s = site(&a.site)?;
            print_json(&ctx()?.pistes(s, None)?.stats)
        }
        Command::Domaines { sites } => {
            let c = ctx()?;
            let planet = skimap_osm::extract::load_planet(&c.dl)?;
            let census = c.census(&planet)?;
            let srn = c.srn(&planet)?;
            println!("{} zones SRN hors stations", srn.areas.len());
            let summary = france::build_registry(&c)?;
            println!("{}", serde_json::to_string_pretty(&summary)?);
            let registry = Registry::load(&c.layout)?;
            let sites: Vec<&Site> = if sites.is_empty() {
                registry.buildable()
            } else {
                sites
                    .iter()
                    .map(|s| Ok(registry.get(s)?))
                    .collect::<Result<_>>()?
            };
            for s in sites {
                c.domaines_site(s, &planet, &census)?;
            }
            print_json(&census.stats)
        }
        Command::Srn => {
            let c = ctx()?;
            let out = skimap_srn::build(&c.layout, &c.dl)?;
            println!("{} zones SRN hors stations", out.areas.len());
            print_json(&out.stats)
        }
        Command::Stations => {
            let c = ctx()?;
            let t = std::time::Instant::now();
            let out = c.stations(true)?;
            println!(
                "stations -> {} ({:.1} s, personal_sources = {})",
                out.dir.display(),
                t.elapsed().as_secs_f64(),
                cfg.personal_sources
            );
            print_json(&out.stats)
        }
        Command::Departs => departs(&ctx()?),
        Command::Refresh { sites } => {
            let c = ctx()?;
            let sites: Vec<&Site> = if sites.is_empty() {
                refresh::default_sites(&c.layout, &registry.buildable())
            } else {
                sites.iter().map(|s| site(s)).collect::<Result<_>>()?
            };
            let summary = refresh::refresh(&c, &sites, chrono::Utc::now())?;
            for l in refresh::describe(&summary) {
                println!("{l}");
            }
            if !summary.ok {
                anyhow::bail!(
                    "refresh : {} sources ou parties en échec (valeurs précédentes gardées)",
                    summary.failures.len()
                );
            }
            Ok(())
        }
        Command::MeteoRefresh { sites, run } => {
            let c = ctx()?;
            let sites: Vec<&Site> = if sites.is_empty() {
                registry.buildable()
            } else {
                sites.iter().map(|s| site(s)).collect::<Result<_>>()?
            };
            let run = run.as_deref().map(build::parse_instant).transpose()?;
            let r = batch::meteo_step_at(&c, &sites, run);
            if r.state == build::StepState::Failed {
                anyhow::bail!("meteo-refresh : {}", r.line);
            }
            Ok(())
        }
        Command::Catalog => {
            let c = france::write_catalog(&cfg.layout(), &registry)?;
            print_json(&c)
        }
        Command::Terrain(a) => {
            let s = site(&a.site)?;
            print_json(&ctx()?.terrain(s)?)
        }
        Command::Snow { site: s, date } => {
            let (s, date) = (
                site(&s)?,
                date.as_deref().map(build::parse_date).transpose()?,
            );
            print_json(&ctx()?.snow(s, date)?)
        }
        Command::SnowValidate { site: s, seasons } => {
            let s = site(&s)?;
            let v = skimap_snow::validate::validate(
                s,
                &cfg.layout(),
                &Downloader::from_config(&cfg)?,
                seasons[0],
                seasons[1],
            )?;
            print_json(&v)
        }
        Command::Refuges(a) => {
            let s = site(&a.site)?;
            print_json(&ctx()?.refuges(s)?)
        }
        Command::Zones(a) => {
            let s = site(&a.site)?;
            print_json(&ctx()?.zones(s)?)
        }
        Command::Coverage {
            site: s,
            release,
            archives,
        } => {
            let s = site(&s)?;
            let mut opts = skimap_coverage::CoverageOptions {
                archive_dir: archives,
                ..Default::default()
            };
            if let Some(r) = release {
                opts.release = build::parse_release(&r)?;
            }
            print_json(&ctx()?.coverage(s, &opts)?)
        }
        Command::Bera { site: s, date } => {
            let (s, date) = (
                site(&s)?,
                date.as_deref().map(build::parse_date).transpose()?,
            );
            print_json(&ctx()?.bera(s, date)?)
        }
        Command::Meteo(a) => {
            let s = site(&a.site)?;
            print_json(&ctx()?.meteo(s)?)
        }
        Command::History {
            site: s,
            first,
            last,
            no_snow,
            no_bera,
        } => {
            let s = site(&s)?;
            let (first, last) = (build::parse_date(&first)?, build::parse_date(&last)?);
            for l in history::history(&ctx()?, s, first, last, !no_snow, !no_bera)? {
                println!("{l}");
            }
            Ok(())
        }
        Command::StatusPoll(a) => {
            let s = site(&a.site)?;
            print_json(&ctx()?.status_poll(s)?)
        }
        Command::AvalancheHistoric { site: s, dump } => {
            let s = site(&s)?;
            print_json(&ctx()?.avalanche_historic(s, dump.as_deref())?)
        }
        Command::ZonesAt {
            site: s,
            lat,
            lon,
            date,
        } => {
            let (s, at) = (site(&s)?, build::parse_instant(&date)?);
            let e = build::zones_at(&cfg.layout(), s, lat, lon, at)?;
            print!("{}", e.describe_fr());
            Ok(())
        }
        Command::AvalancheExposure(a) => {
            let s = site(&a.site)?;
            print_json(&ctx()?.avalanche_exposure(s)?)
        }
        Command::Routing(a) => {
            let s = site(&a.site)?;
            print_json(&ctx()?.routing(s)?)
        }
        Command::OffpistePrepare { site: s, snow_dirs } => {
            let s = site(&s)?;
            print_json(&ctx()?.offpiste_prepare(s, &snow_dirs)?.stats)
        }
        Command::Route {
            site: s,
            from,
            to,
            style,
            date,
            start,
            max_snap,
            out,
        } => {
            let s = site(&s)?;
            let date = date.as_deref().map(build::parse_date).transpose()?;
            let req = route::RouteRequest {
                from: route::parse_lat_lon(&from)?,
                to: route::parse_lat_lon(&to)?,
                style: route::parse_style(&style)?,
                start: start
                    .as_deref()
                    .map(|t| route::parse_start(t, date))
                    .transpose()?,
                max_snap_m: max_snap,
                out,
            };
            let g = route::load_graph(&cfg.layout(), s)?;
            for line in route::route(&g, s, &req)? {
                println!("{line}");
            }
            Ok(())
        }
        Command::Voice { site: id, lang } => {
            let s = site(&id)?;
            let lang = skimap_voice::Lang::from_code(&lang)
                .ok_or_else(|| anyhow::anyhow!("langue {lang:?} : fr ou en"))?;
            let (b, names) = ctx()?.voice(s, lang)?;
            for (what, n) in [
                ("publiables", &names.public),
                ("usage personnel", &names.personal),
            ] {
                println!(
                    "noms {what} : {} pistes, {} lieux, {} dates",
                    n.pistes.len(),
                    n.places.len(),
                    n.dates.len()
                );
            }
            print_json(&b)
        }
        Command::Assist {
            site: s,
            date,
            days,
            home,
            route,
            suggest,
            outings,
            history,
        } => {
            let s = site(&s)?;
            let args = assist::AssistArgs {
                date: date.as_deref().map(build::parse_date).transpose()?,
                days,
                home: home.as_deref().map(assist::parse_lon_lat).transpose()?,
                route,
                suggest,
                outings,
                history,
            };
            let (_, lines) = assist::assist(&cfg.layout(), s, &args)?;
            for l in lines {
                println!("{l}");
            }
            Ok(())
        }
        Command::Plan {
            site: s,
            date,
            request,
        } => {
            let s = site(&s)?;
            let args = plan::PlanArgs {
                date: date.as_deref().map(build::parse_date).transpose()?,
                request,
            };
            for l in plan::plan(&cfg.layout(), s, &args)?.lines {
                println!("{l}");
            }
            Ok(())
        }
        Command::BuildAll(a) => build_all(&cfg, &registry, a),
        Command::Fetch(_) => not_implemented(
            "fetch",
            "P2 #15, P5 #18, P6 #19, P8 #21, P9 #22, P11 #24, P12 #25, P13 #26, P19 #32",
        ),
        Command::BuildPack(a) => {
            let s = site(&a.site)?;
            let c = ctx()?;
            let r = c.build_pack(s)?;
            let problems = skimap_pack::Pack::open(&r.pack_dir)?.verify()?;
            print_json(&r)?;
            if !problems.is_empty() {
                anyhow::bail!("relecture du pack : {}", problems.join(" ; "));
            }
            // VO6: the voice packs apart, one per built language.
            for v in c.build_voice_packs(s)? {
                print_json(&v)?;
            }
            Ok(())
        }
        Command::VoicePack(a) => {
            let s = site(&a.site)?;
            for v in ctx()?.build_voice_packs(s)? {
                print_json(&v)?;
            }
            Ok(())
        }
        Command::Serve { root, addr } => {
            println!(
                "{} sur http://{addr}/ — visualiseur : http://{addr}/tools/viewer/index.html",
                root.display()
            );
            Ok(skimap_pack::serve::serve(&root, addr.as_str())?)
        }
        Command::Eta(_) => not_implemented("eta", "C5 #37"),
        Command::Export(a) => {
            let s = site(&a.site)?;
            print_json(&ctx()?.export(s)?)
        }
        Command::Briefing(_) => not_implemented("briefing", "A1 #88"),
        Command::Fixtures { out } => {
            for l in fixtures::generate(&cfg.layout(), &out)? {
                println!("{l}");
            }
            Ok(())
        }
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    init_tracing(cli.verbose);
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("erreur : {e:#}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn site_filter_selects_from_the_registry() {
        let registry = Registry::evaluation();
        let ids = |f: &SiteFilter| f.select(&registry).iter().map(|s| s.id).collect::<Vec<_>>();
        let all = SiteFilter {
            bera: false,
            status: false,
            built: None,
        };
        assert_eq!(ids(&all), ["cervieres", "chapelle-des-bois"]);
        let bera = SiteFilter { bera: true, ..all };
        assert_eq!(ids(&bera), ["cervieres"]);
        // Nordic France widget for the first, ENJ city for the second.
        let status = SiteFilter {
            status: true,
            bera: false,
            built: None,
        };
        assert_eq!(ids(&status), ["cervieres", "chapelle-des-bois"]);
        // Built: only the sites with a build record in the data directory.
        let dir = std::env::temp_dir().join(format!("skimap-site-filter-{}", std::process::id()));
        let layout = DataLayout::new(&dir);
        let out = layout.out_dir(registry.get("chapelle-des-bois").unwrap());
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(out.join(france::BUILD_RECORD), "{}").unwrap();
        let built = SiteFilter {
            bera: false,
            status: true,
            built: Some(layout),
        };
        assert_eq!(ids(&built), ["chapelle-des-bois"]);
        std::fs::remove_dir_all(&dir).unwrap();
        let cli = Cli::try_parse_from(["skimap", "sites", "--ids", "--bera", "--built"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Sites {
                ids: true,
                bera: true,
                built: true,
                status: false,
                json: false
            }
        ));
        assert!(Cli::try_parse_from(["skimap", "sites", "--bera"]).is_err());
    }

    #[test]
    fn placeholders_accept_any_arguments() {
        let cli =
            Cli::try_parse_from(["skimap", "eta", "cervieres", "--from", "6.7,44.8"]).unwrap();
        assert!(matches!(cli.command, Command::Eta(p) if p.args.len() == 3));
        assert!(run(Cli::try_parse_from(["skimap", "eta"]).unwrap()).is_err());
    }

    #[test]
    fn route_and_core_subcommands_parse() {
        let cli = Cli::try_parse_from([
            "skimap",
            "route",
            "cervieres",
            "44.85894,6.72501",
            "44.84465,6.8157",
            "--style",
            "skating",
            "--date",
            "2027-01-15",
            "--start",
            "13:00",
        ])
        .unwrap();
        assert!(matches!(
            cli.command,
            Command::Route { ref style, start: Some(ref t), max_snap, .. }
                if style == "skating" && t == "13:00" && max_snap == 500.0
        ));
        let cli = Cli::try_parse_from([
            "skimap",
            "offpiste-prepare",
            "cervieres",
            "--snow-dir",
            "a",
            "--snow-dir",
            "b",
        ])
        .unwrap();
        assert!(
            matches!(cli.command, Command::OffpistePrepare { ref snow_dirs, .. } if snow_dirs.len() == 2)
        );
        for cmd in [
            "routing",
            "voice",
            "avalanche-exposure",
            "build-pack",
            "export",
            "assist",
        ] {
            Cli::try_parse_from(["skimap", cmd, "chapelle-des-bois"]).unwrap();
        }
        // Without the graph, `route` says how to build it.
        let dir = tempfile::tempdir().unwrap();
        let cli = Cli::try_parse_from([
            "skimap",
            "--data-dir",
            dir.path().to_str().unwrap(),
            "route",
            "cervieres",
            "44.85,6.72",
            "44.84,6.81",
        ])
        .unwrap();
        let e = format!("{:#}", run(cli).unwrap_err());
        assert!(e.contains("skimap routing cervieres"), "{e}");
    }

    #[test]
    fn builder_subcommands_parse() {
        let cli = Cli::try_parse_from([
            "skimap",
            "zones-at",
            "cervieres",
            "44.85",
            "6.77",
            "2027-01-10",
        ])
        .unwrap();
        assert!(
            matches!(cli.command, Command::ZonesAt { lat, lon, .. } if lat == 44.85 && lon == 6.77)
        );
        let cli = Cli::try_parse_from(["skimap", "snow-validate", "cervieres"]).unwrap();
        assert!(
            matches!(cli.command, Command::SnowValidate { seasons, .. } if seasons == [2020, 2025])
        );
        let cli = Cli::try_parse_from(["skimap", "coverage", "cervieres", "--release", "2026_T2"])
            .unwrap();
        assert!(
            matches!(cli.command, Command::Coverage { release: Some(r), .. } if r == "2026_T2")
        );
        for cmd in [
            "pistes",
            "terrain",
            "refuges",
            "zones",
            "bera",
            "meteo",
            "status-poll",
            "avalanche-historic",
            "build-all",
        ] {
            Cli::try_parse_from(["skimap", cmd, "cervieres"]).unwrap();
        }
        Cli::try_parse_from(["skimap", "domaines"]).unwrap();
        Cli::try_parse_from(["skimap", "stations"]).unwrap();
        let cli =
            Cli::try_parse_from(["skimap", "refresh", "cervieres", "chapelle-des-bois"]).unwrap();
        assert!(matches!(cli.command, Command::Refresh { ref sites } if sites.len() == 2));
        let cli = Cli::try_parse_from(["skimap", "refresh"]).unwrap();
        assert!(matches!(cli.command, Command::Refresh { ref sites } if sites.is_empty()));
    }

    #[test]
    fn sites_json_has_lambert93_bbox() {
        let v = site_json(&skimap_common::site::SITES[0]).unwrap();
        assert_eq!(v["id"], "cervieres");
        assert!(v["bbox_lambert93"]["min_x"].as_f64().unwrap() > 980_000.0);
    }

    #[test]
    fn unknown_site_is_an_error() {
        let cli = Cli::try_parse_from(["skimap", "pistes", "mouthe"]).unwrap();
        let e = run(cli).unwrap_err().to_string();
        assert!(e.contains("mouthe"), "{e}");
    }
}
