//! `skimap`: command-line entry point of the pipeline and of the core algorithms.
//!
//! Owned tickets: F1 (#1) skeleton, F4 (#4) Android arm64 build. Owned by the
//! orchestrator: each placeholder is wired to the public function exposed by the crate of
//! the ticket named in its help text.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use skimap_common::config::Config;
use skimap_common::crs::{Crs, project_bbox};
use skimap_common::download::{CachePolicy, Downloader, Request};
use skimap_common::site::{MassifStatus, SITES, Site};

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
    /// List the evaluation sites.
    Sites {
        /// JSON output.
        #[arg(long)]
        json: bool,
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
    /// Fetch every source of a site (P2, P5, P6, P8, P9, P11, P12, P13, P19).
    Fetch(Placeholder),
    /// Build the offline pack of a site (P14, #27).
    BuildPack(Placeholder),
    /// Route between two points, on trails or off-piste (C1 #33, C3 #35).
    Route(Placeholder),
    /// Estimated time of a route (C5, #37).
    Eta(Placeholder),
    /// Zones and rules active at a position and a date (C8, #40).
    ZonesAt(Placeholder),
    /// Export the layers of a site for the viewer (P15, #28).
    Export(Placeholder),
    /// Daily briefing of a site (A1, #88).
    Briefing(Placeholder),
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

fn print_sites(json: bool) -> Result<()> {
    if json {
        let all = SITES.iter().map(site_json).collect::<Result<Vec<_>>>()?;
        println!("{}", serde_json::to_string_pretty(&all)?);
        return Ok(());
    }
    for s in SITES {
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
        };
        println!(
            "{:<18} {} ({}, INSEE {})",
            s.id, s.name, s.department, s.insee
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

fn run(cli: Cli) -> Result<()> {
    let mut cfg = Config::from_env()?;
    if let Some(d) = cli.data_dir {
        cfg.data_dir = d;
    }
    match cli.command {
        Command::Sites { json } => print_sites(json),
        Command::FetchUrl {
            url,
            source,
            cache,
            max_age,
            print,
        } => fetch_url(&cfg, &url, &source, cache, max_age, print),
        Command::Fetch(_) => not_implemented(
            "fetch",
            "P2 #15, P5 #18, P6 #19, P8 #21, P9 #22, P11 #24, P12 #25, P13 #26, P19 #32",
        ),
        Command::BuildPack(_) => not_implemented("build-pack", "P14 #27"),
        Command::Route(_) => not_implemented("route", "C1 #33, C3 #35"),
        Command::Eta(_) => not_implemented("eta", "C5 #37"),
        Command::ZonesAt(_) => not_implemented("zones-at", "C8 #40"),
        Command::Export(_) => not_implemented("export", "P15 #28"),
        Command::Briefing(_) => not_implemented("briefing", "A1 #88"),
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
    fn placeholders_accept_any_arguments() {
        let cli =
            Cli::try_parse_from(["skimap", "route", "cervieres", "--from", "6.7,44.8"]).unwrap();
        assert!(matches!(cli.command, Command::Route(p) if p.args.len() == 3));
        assert!(run(Cli::try_parse_from(["skimap", "eta"]).unwrap()).is_err());
    }

    #[test]
    fn sites_json_has_lambert93_bbox() {
        let v = site_json(&SITES[0]).unwrap();
        assert_eq!(v["id"], "cervieres");
        assert!(v["bbox_lambert93"]["min_x"].as_f64().unwrap() > 980_000.0);
    }
}
