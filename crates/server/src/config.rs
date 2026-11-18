//! Configuration from environment variables (Kubernetes ConfigMap / Secret).

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Result, bail};

use crate::alerts::mail::{SmtpConfig, Tls};
use crate::store::Retention;
use crate::strava::StravaConfig;

/// Server settings.
#[derive(Debug, Clone)]
pub struct Config {
    /// Listen address (`SKIMAP_SERVER_ADDR`, default `0.0.0.0:8080`).
    pub addr: String,
    /// Worker threads (`SKIMAP_SERVER_WORKERS`, default 16), file downloads at the same
    /// time (`SKIMAP_SERVER_MAX_DOWNLOADS`, default 512), timeout of a silent connection
    /// (`SKIMAP_SERVER_IO_TIMEOUT_SECS`, default 60).
    pub limits: crate::http::Limits,
    /// Published packs (`SKIMAP_STORE`, default `/data/store`).
    pub store: PathBuf,
    /// Alert database (`SKIMAP_SERVER_DB`, default `/data/server/skimap.sqlite`).
    pub db: PathBuf,
    /// Public URL (`SKIMAP_PUBLIC_URL`, default `https://skimap.warnier.net`).
    pub public_url: String,
    /// Contact address on the privacy page (`SKIMAP_CONTACT_EMAIL`), required by `serve`.
    pub contact_email: Option<String>,
    /// Publisher named on the privacy page (`SKIMAP_CONTROLLER`).
    pub controller: Option<String>,
    /// SMTP relay, if `SMTP_HOST` is set.
    pub smtp: Option<SmtpConfig>,
    /// Most alert e-mails per 24 h (`SKIMAP_ALERT_DAILY_CAP`, default 400: under Gmail's
    /// 500 recipients a day).
    pub alert_daily_cap: u32,
    /// Strava exchange, if `STRAVA_CLIENT_ID` and `STRAVA_CLIENT_SECRET` are set.
    pub strava: Option<StravaConfig>,
    /// Versions kept for deltas (`SKIMAP_RECENT_VERSIONS`, default 8) and rebase threshold.
    pub retention: Retention,
}

impl Config {
    /// From the process environment.
    pub fn from_env() -> Result<Self> {
        Self::from_vars(std::env::vars())
    }

    /// From explicit variables (tests).
    pub fn from_vars<I, K, V>(vars: I) -> Result<Self>
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<str>,
        V: AsRef<str>,
    {
        let map: std::collections::HashMap<String, String> = vars
            .into_iter()
            .map(|(k, v)| (k.as_ref().to_owned(), v.as_ref().to_owned()))
            .filter(|(_, v)| !v.is_empty())
            .collect();
        let get = |k: &str| map.get(k).cloned();
        let or = |k: &str, d: &str| get(k).unwrap_or_else(|| d.to_owned());
        let num = |k: &str, d: u64| -> Result<u64> {
            match get(k) {
                Some(v) => v
                    .trim()
                    .parse()
                    .map_err(|_| anyhow::anyhow!("{k}: not a number")),
                None => Ok(d),
            }
        };
        let public_url = or("SKIMAP_PUBLIC_URL", "https://skimap.warnier.net")
            .trim_end_matches('/')
            .to_owned();
        let smtp = match get("SMTP_HOST") {
            None => None,
            Some(host) => {
                let tls = match or("SMTP_TLS", "starttls").to_ascii_lowercase().as_str() {
                    "starttls" => Tls::StartTls,
                    "implicit" | "tls" => Tls::Implicit,
                    "none" => {
                        if !matches!(host.as_str(), "localhost" | "127.0.0.1" | "::1") {
                            bail!("SMTP_TLS=none is only allowed for a loopback relay");
                        }
                        Tls::None
                    }
                    t => bail!("SMTP_TLS: {t:?} (starttls, implicit or none)"),
                };
                let port = num("SMTP_PORT", if tls == Tls::Implicit { 465 } else { 587 })?;
                let user = get("SMTP_USER");
                let from = get("MAIL_FROM")
                    .or_else(|| user.clone())
                    .unwrap_or_default();
                if !crate::util::valid_email(&from) {
                    bail!("MAIL_FROM (or SMTP_USER) must be the sender's e-mail address");
                }
                Some(SmtpConfig {
                    host,
                    port: u16::try_from(port)?,
                    tls,
                    user,
                    pass: get("SMTP_PASS"),
                    from,
                    from_name: or("MAIL_FROM_NAME", "skimap (alertes)"),
                    helo: or("MAIL_HELO", "skimap.warnier.net"),
                    timeout: Duration::from_secs(30),
                })
            }
        };
        let strava = match (get("STRAVA_CLIENT_ID"), get("STRAVA_CLIENT_SECRET")) {
            (Some(id), Some(secret)) => Some(StravaConfig {
                client_id: id.trim().to_owned(),
                client_secret: secret.trim().to_owned(),
                base_url: or("STRAVA_BASE_URL", "https://www.strava.com"),
                scope: or("STRAVA_SCOPE", "activity:write"),
                app_redirect: or("STRAVA_APP_REDIRECT", "skimap://strava/callback"),
                public_url: public_url.clone(),
            }),
            _ => None,
        };
        Ok(Self {
            addr: or("SKIMAP_SERVER_ADDR", "0.0.0.0:8080"),
            limits: {
                let d = crate::http::Limits::default();
                crate::http::Limits {
                    workers: usize::try_from(num("SKIMAP_SERVER_WORKERS", d.workers as u64)?)?,
                    max_downloads: usize::try_from(num(
                        "SKIMAP_SERVER_MAX_DOWNLOADS",
                        d.max_downloads as u64,
                    )?)?,
                    io_timeout: Duration::from_secs(num(
                        "SKIMAP_SERVER_IO_TIMEOUT_SECS",
                        d.io_timeout.as_secs(),
                    )?),
                }
            },
            store: PathBuf::from(or("SKIMAP_STORE", "/data/store")),
            db: PathBuf::from(or("SKIMAP_SERVER_DB", "/data/server/skimap.sqlite")),
            contact_email: get("SKIMAP_CONTACT_EMAIL"),
            controller: get("SKIMAP_CONTROLLER"),
            public_url,
            smtp,
            alert_daily_cap: u32::try_from(num("SKIMAP_ALERT_DAILY_CAP", 400)?)?,
            strava,
            retention: Retention {
                recent_versions: usize::try_from(num("SKIMAP_RECENT_VERSIONS", 8)?)?,
                rebase_fraction: Retention::default().rebase_fraction,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_secrets() {
        let c = Config::from_vars([
            ("SMTP_HOST", "smtp.gmail.com"),
            ("SMTP_USER", "a@gmail.com"),
            ("SMTP_PASS", "s3cret"),
        ])
        .unwrap();
        let s = c.smtp.unwrap();
        assert_eq!(
            (s.port, s.tls, s.from.as_str()),
            (587, Tls::StartTls, "a@gmail.com")
        );
        assert!(!format!("{s:?}").contains("s3cret"));
        assert!(c.strava.is_none());
        assert_eq!(c.public_url, "https://skimap.warnier.net");
        assert_eq!(c.limits, crate::http::Limits::default());
        let c = Config::from_vars([
            ("SKIMAP_SERVER_MAX_DOWNLOADS", "64"),
            ("SKIMAP_SERVER_IO_TIMEOUT_SECS", "120"),
        ])
        .unwrap();
        assert_eq!(
            (c.limits.max_downloads, c.limits.io_timeout.as_secs()),
            (64, 120)
        );
        let c =
            Config::from_vars([("STRAVA_CLIENT_ID", "1"), ("STRAVA_CLIENT_SECRET", "zz")]).unwrap();
        assert!(!format!("{:?}", c.strava.unwrap()).contains("zz"));
        assert!(
            Config::from_vars([
                ("SMTP_HOST", "smtp.gmail.com"),
                ("SMTP_TLS", "none"),
                ("MAIL_FROM", "a@b.fr")
            ])
            .is_err()
        );
        assert!(Config::from_vars([("SMTP_HOST", "smtp.gmail.com")]).is_err());
    }
}
