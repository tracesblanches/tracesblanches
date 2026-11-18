//! Configuration from the environment.
//!
//! | Variable | Meaning |
//! |---|---|
//! | `SKIMAP_DATA_DIR` | data directory (default `./data`) |
//! | `METEOFRANCE_API_KEY` | Météo-France portal key (BERA, AROME, observations) |
//! | `DATATOURISME_API_KEY` | DATAtourisme API key |
//! | `SKITOUR_API_KEY` | Skitour API key |
//! | `SKIMAP_RATE_LIMIT_<SOURCE>` | rate-limit override, e.g. `SKIMAP_RATE_LIMIT_DATATOURISME=1000/h` |
//! | `SKIMAP_PERSONAL_SOURCES` | `1` or `true`: also use the personal-use-only sources (default off) |
//! | `SKIMAP_BUILD_MEMORY_MIB` | memory limit `build-all` plans its parallel sites with, MiB (default: the container's cgroup limit, else 4096) |
//!
//! Keys are wrapped in [`Secret`], whose `Debug` and `Display` never print the value:
//! a key never reaches a log, an error message or a cache file.

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use crate::error::{Error, Result};
use crate::layout::DataLayout;

/// A secret value (API key). Formatting prints `***`; use [`Secret::expose`] to send it.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    /// Wraps a secret value.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The secret value, only to be placed in an outgoing request.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(***)")
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("***")
    }
}

/// At most `max_requests` requests in any sliding window of length `per`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateLimit {
    /// Maximum number of requests in the window.
    pub max_requests: u32,
    /// Window length.
    pub per: Duration,
}

impl RateLimit {
    /// `max_requests` per `per`.
    pub const fn new(max_requests: u32, per: Duration) -> Self {
        Self { max_requests, per }
    }

    /// Requests per hour.
    pub const fn per_hour(max_requests: u32) -> Self {
        Self::new(max_requests, Duration::from_secs(3600))
    }

    /// Requests per day (24 h sliding window).
    pub const fn per_day(max_requests: u32) -> Self {
        Self::new(max_requests, Duration::from_secs(86_400))
    }

    /// Parses `N/period` where period is `[k]unit`, unit in `ms`, `s`, `min`, `h`, `d`
    /// (e.g. `1000/h`, `1/5s`, `100/15min`). Several limits may be joined with `,`.
    pub fn parse_list(spec: &str) -> Result<Vec<RateLimit>> {
        spec.split(',').map(|s| Self::parse(s.trim())).collect()
    }

    /// Parses a single `N/period` limit, see [`RateLimit::parse_list`].
    pub fn parse(spec: &str) -> Result<RateLimit> {
        let bad = || {
            Error::Config(format!(
                "invalid rate limit `{spec}` (expected e.g. `1000/h`)"
            ))
        };
        let (n, period) = spec.split_once('/').ok_or_else(bad)?;
        let max_requests: u32 = n.trim().parse().map_err(|_| bad())?;
        let period = period.trim();
        let split = period.find(|c: char| !c.is_ascii_digit()).ok_or_else(bad)?;
        let (k, unit) = period.split_at(split);
        let k: u64 = if k.is_empty() {
            1
        } else {
            k.parse().map_err(|_| bad())?
        };
        let unit_ms: u64 = match unit {
            "ms" => 1,
            "s" => 1_000,
            "min" | "m" => 60_000,
            "h" => 3_600_000,
            "d" => 86_400_000,
            _ => return Err(bad()),
        };
        if max_requests == 0 || k == 0 {
            return Err(bad());
        }
        Ok(RateLimit::new(
            max_requests,
            Duration::from_millis(k * unit_ms),
        ))
    }
}

/// Validates a source name: `[a-z0-9-]+`, used as a cache directory and in env names.
pub fn check_source_name(name: &str) -> Result<()> {
    let ok = !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if ok {
        Ok(())
    } else {
        Err(Error::Config(format!(
            "invalid source name `{name}` (expected [a-z0-9-]+)"
        )))
    }
}

/// Environment variable holding the rate-limit override of a source.
pub fn rate_limit_var(source: &str) -> String {
    format!(
        "SKIMAP_RATE_LIMIT_{}",
        source.to_ascii_uppercase().replace('-', "_")
    )
}

/// Boolean environment flag: `1` / `true` or `0` / `false` (case-insensitive).
fn parse_flag(name: &str, v: &str) -> Result<bool> {
    match v.trim().to_ascii_lowercase().as_str() {
        "1" | "true" => Ok(true),
        "0" | "false" => Ok(false),
        _ => Err(Error::Config(format!(
            "{name}: expected 1, true, 0 or false, got `{v}`"
        ))),
    }
}

/// Process configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    /// Data directory.
    pub data_dir: PathBuf,
    /// Météo-France portal API key.
    pub meteofrance_api_key: Option<Secret>,
    /// DATAtourisme API key.
    pub datatourisme_api_key: Option<Secret>,
    /// Skitour API key.
    pub skitour_api_key: Option<Secret>,
    /// Rate-limit overrides per source name.
    pub rate_limits: BTreeMap<String, Vec<RateLimit>>,
    /// Also use the sources whose terms only allow personal, non-commercial use (Nordic
    /// France widget, Open-Meteo free API, trail status pages without a licence: ENJ, town
    /// hall pages; Biodiv'sports; the DDT 05 CLPA). Off by default: their content
    /// must not end up in a redistributed pack. Enforced at the root: a downloader built
    /// from this configuration refuses any request to a source whose policy is
    /// `personal_use_only` when this is off (`Error::PersonalSourceRefused`).
    pub personal_sources: bool,
    /// AROME run used by every hourly weather refresh of the process instead of the
    /// latest complete one (`SKIMAP_AROME_RUN`, RFC 3339): replay of a run still published
    /// (measurements of successive runs, tests). Unset in production.
    pub arome_run: Option<String>,
    /// Memory limit, MiB, under which `build-all` plans the sites it builds together
    /// (`SKIMAP_BUILD_MEMORY_MIB`); unset: the cgroup limit of the container when
    /// readable, else a safe default (`batch::DEFAULT_MEMORY_LIMIT`).
    pub build_memory_mib: Option<u64>,
}

impl Config {
    /// Reads the process environment.
    pub fn from_env() -> Result<Self> {
        Self::from_vars(std::env::vars())
    }

    /// Reads configuration from `(name, value)` pairs (testable form of [`Config::from_env`]).
    pub fn from_vars<I, K, V>(vars: I) -> Result<Self>
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<str>,
        V: Into<String>,
    {
        let mut cfg = Config {
            data_dir: PathBuf::from("data"),
            meteofrance_api_key: None,
            datatourisme_api_key: None,
            skitour_api_key: None,
            rate_limits: BTreeMap::new(),
            personal_sources: false,
            arome_run: None,
            build_memory_mib: None,
        };
        for (k, v) in vars {
            let (k, v) = (k.as_ref(), v.into());
            if v.trim().is_empty() {
                continue;
            }
            match k {
                "SKIMAP_DATA_DIR" => cfg.data_dir = PathBuf::from(v),
                "METEOFRANCE_API_KEY" => cfg.meteofrance_api_key = Some(Secret::new(v)),
                "DATATOURISME_API_KEY" => cfg.datatourisme_api_key = Some(Secret::new(v)),
                "SKITOUR_API_KEY" => cfg.skitour_api_key = Some(Secret::new(v)),
                "SKIMAP_PERSONAL_SOURCES" => cfg.personal_sources = parse_flag(k, &v)?,
                "SKIMAP_AROME_RUN" => cfg.arome_run = Some(v),
                "SKIMAP_BUILD_MEMORY_MIB" => {
                    cfg.build_memory_mib = Some(v.trim().parse().map_err(|_| {
                        Error::Config(format!("{k}: `{v}` n'est pas un nombre de Mio"))
                    })?)
                }
                _ => {
                    if let Some(src) = k.strip_prefix("SKIMAP_RATE_LIMIT_") {
                        let source = src.to_ascii_lowercase().replace('_', "-");
                        check_source_name(&source)?;
                        let limits = RateLimit::parse_list(&v)
                            .map_err(|e| Error::Config(format!("{k}: {e}")))?;
                        cfg.rate_limits.insert(source, limits);
                    }
                }
            }
        }
        Ok(cfg)
    }

    /// Data layout rooted at [`Config::data_dir`].
    pub fn layout(&self) -> DataLayout {
        DataLayout::new(&self.data_dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_are_never_formatted() {
        let cfg = Config::from_vars([("METEOFRANCE_API_KEY", "abc-SECRET-123")]).unwrap();
        let key = cfg.meteofrance_api_key.as_ref().unwrap();
        assert_eq!(key.expose(), "abc-SECRET-123");
        let printed = format!("{cfg:?} {key} {key:?}");
        assert!(!printed.contains("SECRET"), "{printed}");
        assert!(printed.contains("***"));
    }

    #[test]
    fn defaults_and_overrides() {
        let cfg = Config::from_vars(Vec::<(String, String)>::new()).unwrap();
        assert_eq!(cfg.data_dir, PathBuf::from("data"));
        assert!(cfg.meteofrance_api_key.is_none());

        let cfg = Config::from_vars([
            ("SKIMAP_DATA_DIR", "/tmp/x"),
            ("DATATOURISME_API_KEY", "  "),
            ("SKIMAP_RATE_LIMIT_BIODIV_SPORTS", "1/2s, 100/h"),
            ("UNRELATED", "1"),
        ])
        .unwrap();
        assert_eq!(cfg.data_dir, PathBuf::from("/tmp/x"));
        assert!(
            cfg.datatourisme_api_key.is_none(),
            "blank values are ignored"
        );
        assert_eq!(
            cfg.rate_limits["biodiv-sports"],
            vec![
                RateLimit::new(1, Duration::from_secs(2)),
                RateLimit::per_hour(100)
            ]
        );
        assert!(Config::from_vars([("SKIMAP_RATE_LIMIT_X", "fast")]).is_err());
    }

    #[test]
    fn personal_sources_flag() {
        let get = |v: &str| Config::from_vars([("SKIMAP_PERSONAL_SOURCES", v)]);
        assert!(
            !Config::from_vars(Vec::<(String, String)>::new())
                .unwrap()
                .personal_sources
        );
        for on in ["1", "true", "TRUE", " true "] {
            assert!(get(on).unwrap().personal_sources, "{on}");
        }
        for off in ["0", "false", ""] {
            assert!(!get(off).unwrap().personal_sources, "{off}");
        }
        assert!(get("yes").is_err());
    }

    #[test]
    fn parse_rate_limits() {
        assert_eq!(
            RateLimit::parse("1000/h").unwrap(),
            RateLimit::per_hour(1000)
        );
        assert_eq!(
            RateLimit::parse("1000/1d").unwrap(),
            RateLimit::per_day(1000)
        );
        assert_eq!(
            RateLimit::parse("2/400ms").unwrap(),
            RateLimit::new(2, Duration::from_millis(400))
        );
        assert_eq!(
            RateLimit::parse("100/15min").unwrap(),
            RateLimit::new(100, Duration::from_secs(900))
        );
        for bad in ["", "10", "0/h", "10/0s", "10/h2", "x/h", "10/fortnight"] {
            assert!(RateLimit::parse(bad).is_err(), "{bad}");
        }
        assert_eq!(
            rate_limit_var("biodiv-sports"),
            "SKIMAP_RATE_LIMIT_BIODIV_SPORTS"
        );
        assert!(check_source_name("Overpass").is_err());
        assert!(check_source_name("overpass").is_ok());
    }
}
