//! `skimap-server`: `serve`, `publish`, `send-test-email`.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use skimap_server::alerts::Alerts;
use skimap_server::alerts::mail::{Mailer, NoMailer, SmtpMailer};
use skimap_server::app::App;
use skimap_server::clock::{Clock, SystemClock};
use skimap_server::config::Config;
use skimap_server::store::{Retention, Store};
use skimap_server::strava::Strava;

#[derive(Parser, Debug)]
#[command(
    name = "skimap-server",
    version,
    about = "Serveur de skimap : packs, mises à jour, alerte de non-retour, OAuth Strava"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Serves HTTP (settings from the environment, see the crate README).
    Serve {
        /// Listen address (overrides SKIMAP_SERVER_ADDR).
        #[arg(long)]
        addr: Option<String>,
    },
    /// Publishes pipeline packs (data/out/<site>/pack/, and the voice packs
    /// data/out/<site>/voice-pack/<lang>/) into the store; prints a JSON report per pack.
    /// With `--all-in data/out`, every site pack and voice pack found there (every domain
    /// of France); the catalog is written once at the end.
    Publish {
        /// Pack directories.
        #[arg(required_unless_present = "all_in")]
        packs: Vec<PathBuf>,
        /// Publish every `<dir>/<site>/pack/` holding a `manifest.json`.
        #[arg(long)]
        all_in: Option<PathBuf>,
        /// Store directory (overrides SKIMAP_STORE).
        #[arg(long)]
        store: Option<PathBuf>,
    },
    /// Publishes the France basemap (X20) built by the pipeline
    /// (`data/out/france-basemap/`, `france.json`); prints a JSON report.
    PublishFrance {
        /// Directory of `france.json`.
        dir: PathBuf,
        /// Store directory (overrides SKIMAP_STORE).
        #[arg(long)]
        store: Option<PathBuf>,
    },
    /// Sends one test e-mail through the configured SMTP relay.
    SendTestEmail {
        /// Recipient.
        to: String,
    },
}

fn mailer(cfg: &Config) -> Result<Arc<dyn Mailer>> {
    Ok(match &cfg.smtp {
        Some(s) => Arc::new(SmtpMailer::new(s.clone())?),
        None => Arc::new(NoMailer),
    })
}

fn serve(cfg: Config, addr: Option<String>) -> Result<()> {
    let contact = cfg
        .contact_email
        .clone()
        .filter(|c| skimap_server::util::valid_email(c))
        .context("SKIMAP_CONTACT_EMAIL must be the contact address of the privacy page")?;
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let store = Store::open(&cfg.store)?;
    // Without a relay no alert could ever leave: the service is off (`/v1/trips` answers
    // 503) rather than accepting plans whose alerts would wait in the outbox forever.
    let alerts = match &cfg.smtp {
        Some(_) => {
            let mut a = Alerts::open(&cfg.db, mailer(&cfg)?, Arc::clone(&clock), &cfg.public_url)?;
            a.daily_cap = cfg.alert_daily_cap;
            let a = Arc::new(a);
            tracing::info!(mailer = %a.mailer(), "alerts");
            Some(a)
        }
        None => {
            tracing::warn!(
                "alerts disabled: no SMTP relay (SMTP_HOST unset), /v1/trips answers 503"
            );
            None
        }
    };
    let strava = match &cfg.strava {
        Some(s) => Some((Strava::new(s.clone())?, s.clone())),
        None => None,
    };
    tracing::info!(configured = strava.is_some(), "strava");
    let mut app = App::new(
        store,
        alerts.clone(),
        strava,
        &cfg.public_url,
        &contact,
        Arc::clone(&clock),
    );
    if let Some(c) = &cfg.controller {
        app.controller = c.clone();
    }
    // Collaborative reports (V8): their own database beside the alerts' one (or
    // `SKIMAP_COLLAB_DB`), purged hourly.
    let collab_db = std::env::var_os("SKIMAP_COLLAB_DB")
        .map(PathBuf::from)
        .unwrap_or_else(|| cfg.db.with_file_name("collab.sqlite"));
    let collab = Arc::new(skimap_server::collab::Collab::open(&collab_db)?);
    app.collab = Some(Arc::clone(&collab));
    let purge_clock = Arc::clone(&clock);
    std::thread::spawn(move || {
        loop {
            collab.purge(purge_clock.as_ref());
            std::thread::sleep(Duration::from_secs(3600));
        }
    });
    let addr = addr.unwrap_or_else(|| cfg.addr.clone());
    let running = skimap_server::http::serve_with(&addr, cfg.limits, Arc::new(app))?;
    tracing::info!(addr = %running.addr, limits = ?cfg.limits, store = %cfg.store.display(), "listening");
    // The timer of the alerts.
    if let Some(alerts) = alerts {
        std::thread::spawn(move || {
            loop {
                match alerts.tick() {
                    Ok(r) if r != Default::default() => tracing::info!(?r, "alert tick"),
                    Ok(_) => {}
                    Err(e) => tracing::error!(error = %format!("{e:#}"), "alert tick"),
                }
                std::thread::sleep(Duration::from_secs(15));
            }
        });
    }
    running.join();
    Ok(())
}

/// Every `<root>/<site>/pack/` with a `manifest.json`, by site, each followed by its voice
/// packs `<root>/<site>/voice-pack/<lang>/` (VO6).
fn site_packs(root: &std::path::Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut sites: Vec<PathBuf> = std::fs::read_dir(root)
        .with_context(|| format!("{}", root.display()))?
        .map(|e| e.map(|e| e.path()))
        .collect::<std::io::Result<_>>()?;
    sites.sort();
    for site in sites {
        let p = site.join("pack");
        if p.join("manifest.json").is_file() {
            out.push(p);
        }
        if let Ok(rd) = std::fs::read_dir(site.join("voice-pack")) {
            let mut voices: Vec<PathBuf> = rd
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|v| v.join("manifest.json").is_file())
                .collect();
            voices.sort();
            out.extend(voices);
        }
    }
    Ok(out)
}

fn run(cli: Cli) -> Result<()> {
    let cfg = Config::from_env()?;
    match cli.command {
        Command::Serve { addr } => serve(cfg, addr),
        Command::Publish {
            mut packs,
            all_in,
            store,
        } => {
            let store = Store::open(store.unwrap_or(cfg.store.clone()))?;
            let retention: Retention = cfg.retention;
            if let Some(root) = all_in {
                packs.extend(site_packs(&root)?);
            }
            let reports = store.publish_many(&packs, chrono::Utc::now(), retention)?;
            let mut failed = 0;
            for (p, r) in packs.iter().zip(reports) {
                match r {
                    Ok(r) => println!("{}", serde_json::to_string(&r)?),
                    Err(e) => {
                        failed += 1;
                        eprintln!("publishing {} : {e:#}", p.display());
                    }
                }
            }
            if failed > 0 {
                anyhow::bail!("{failed} of {} packs not published", packs.len());
            }
            Ok(())
        }
        Command::PublishFrance { dir, store } => {
            let store = Store::open(store.unwrap_or(cfg.store.clone()))?;
            let r = skimap_server::france::publish(&store, &dir)?;
            println!("{}", serde_json::to_string(&r)?);
            Ok(())
        }
        Command::SendTestEmail { to } => {
            let m = mailer(&cfg)?;
            m.send(&skimap_server::alerts::mail::Email {
                to: to.clone(),
                subject: "skimap : message de test du service d'alerte".into(),
                body: format!(
                    "Bonjour,\n\nCeci est un message de test du service d'alerte de non-retour de skimap ({}).\n\
                     Si vous le lisez, les alertes vous parviendront.\n",
                    cfg.public_url
                ),
            })?;
            println!("sent to {to} via {}", m.describe());
            Ok(())
        }
    }
}

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "skimap_server=info,info".into()),
        )
        .with_writer(std::io::stderr)
        .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stderr()))
        .init();
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("skimap-server: {e:#}");
            ExitCode::FAILURE
        }
    }
}
