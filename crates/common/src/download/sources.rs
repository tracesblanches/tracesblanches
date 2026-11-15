//! Per-source download policies: rate limits, cache lifetime, retries.
//!
//! Documented quotas are cited; the others are conservative defaults chosen to stay polite
//! and are marked "to confirm". Any limit can be overridden with
//! `SKIMAP_RATE_LIMIT_<SOURCE>` (see [`crate::config`]).

use std::time::Duration;

use crate::config::RateLimit;

/// Download policy of one source.
#[derive(Debug, Clone, PartialEq)]
pub struct SourcePolicy {
    /// Source name (`[a-z0-9-]+`), also the cache sub-directory.
    pub name: String,
    /// Sliding-window limits; all must allow a request.
    pub limits: Vec<RateLimit>,
    /// Longest wait accepted for the rate limit (or a `Retry-After`) before failing with
    /// [`crate::error::Error::RateLimited`].
    pub max_wait: Duration,
    /// Cached bodies younger than this are served without any request.
    pub max_age: Duration,
    /// Retries after the first attempt (transport errors, 408, 429, 5xx).
    pub retries: u32,
    /// First backoff delay; doubled at each retry.
    pub backoff_base: Duration,
    /// Upper bound of the backoff delay.
    pub backoff_max: Duration,
    /// Whole-request timeout (large archives need minutes).
    pub timeout: Duration,
    /// Requests in flight at once for this source, all threads of the process together
    /// (default 1: requests to one source are serialised). The rate limits apply to all
    /// of them; more than one only helps a source whose latency, not its quota, bounds
    /// the throughput (IGN tiles, geo.api.gouv.fr), when many sites are built at once.
    pub max_in_flight: usize,
    /// Terms allow personal, non-commercial use only: the downloader refuses the source
    /// unless `personal_sources` is on ([`crate::config::Config::personal_sources`]).
    pub personal_use_only: bool,
}

const SECOND: Duration = Duration::from_secs(1);
const MINUTE: Duration = Duration::from_secs(60);
const HOUR: Duration = Duration::from_secs(3600);
const DAY: Duration = Duration::from_secs(86_400);

impl SourcePolicy {
    /// Default policy for a source: 1 request/s, 1 day cache, 4 retries.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            limits: vec![RateLimit::new(1, SECOND)],
            max_wait: 2 * MINUTE,
            max_age: DAY,
            retries: 4,
            backoff_base: 2 * SECOND,
            backoff_max: MINUTE,
            timeout: 30 * MINUTE,
            max_in_flight: 1,
            personal_use_only: false,
        }
    }

    /// Marks the source personal use only.
    pub fn personal_use_only(mut self) -> Self {
        self.personal_use_only = true;
        self
    }

    /// Sets the number of requests in flight at once (at least 1).
    pub fn max_in_flight(mut self, n: usize) -> Self {
        self.max_in_flight = n.max(1);
        self
    }

    /// Replaces the rate limits.
    pub fn limits(mut self, limits: impl Into<Vec<RateLimit>>) -> Self {
        self.limits = limits.into();
        self
    }

    /// Sets the cache lifetime.
    pub fn max_age(mut self, d: Duration) -> Self {
        self.max_age = d;
        self
    }

    /// Sets the longest accepted wait.
    pub fn max_wait(mut self, d: Duration) -> Self {
        self.max_wait = d;
        self
    }

    /// Sets retries and backoff.
    pub fn retries(mut self, retries: u32, base: Duration, max: Duration) -> Self {
        self.retries = retries;
        self.backoff_base = base;
        self.backoff_max = max;
        self
    }

    /// Sets the request timeout.
    pub fn timeout(mut self, d: Duration) -> Self {
        self.timeout = d;
        self
    }
}

/// Policies of the known sources.
pub fn known_sources() -> Vec<SourcePolicy> {
    vec![
        // Public Overpass: slot-based, guidance < 10 000 queries/day; queries are heavy.
        SourcePolicy::new("overpass")
            .limits([RateLimit::new(1, 5 * SECOND), RateLimit::per_day(10_000)])
            .max_wait(5 * MINUTE),
        // Geofabrik OSM extracts (.osm.pbf, X5): about 20 files, 4.9 GB, published once a
        // day, downloaded one at a time; a week-old copy is enough for huts and points of
        // interest, and the conditional request (ETag, Last-Modified) avoids downloading
        // it twice.
        SourcePolicy::new("geofabrik")
            .limits([RateLimit::new(1, 10 * SECOND)])
            .max_age(7 * DAY)
            .timeout(3 * HOUR),
        // OpenSnowMap daily extract planet_pistes.osm.gz (published once a day).
        SourcePolicy::new("opensnowmap").limits([RateLimit::new(1, MINUTE)]),
        // Biodiv'sports: no published quota, to confirm.
        SourcePolicy::new("biodiv-sports")
            .max_age(7 * DAY)
            .personal_use_only(),
        // DATAtourisme API: 1000 requests/hour (api.datatourisme.fr docs).
        SourcePolicy::new("datatourisme")
            .limits([RateLimit::new(1, SECOND), RateLimit::per_hour(1000)]),
        // Skitour API: 1000 requests/day (ticket P1).
        SourcePolicy::new("skitour")
            .limits([RateLimit::new(1, SECOND), RateLimit::per_day(1000)])
            .max_age(7 * DAY),
        // Météo-France portal: 50 requests/min commonly announced, to confirm.
        SourcePolicy::new("meteofrance")
            .limits([RateLimit::new(50, MINUTE)])
            .max_age(HOUR),
        // refuges.info: no key, no published quota.
        SourcePolicy::new("refuges-info").max_age(7 * DAY),
        // Camptocamp API: no published quota.
        SourcePolicy::new("camptocamp").max_age(7 * DAY),
        // IGN Géoplateforme (data.geopf.fr): vector TMS limited to 400 req/s per IP. About
        // 0.2 to 0.3 s per tile measured alone (15/11/2026), 1 s and more under load: one
        // request at a time gives 1 to 5 tiles/s; 8 in flight gave 7 to 8/s while building
        // 4 domains at once, 16 reach the 20/s limit (G2, every domain of France).
        SourcePolicy::new("ign")
            .limits([RateLimit::new(20, SECOND)])
            .max_age(30 * DAY)
            .max_in_flight(16),
        // ENJ Snowmap Geotrek API: trail status, no licence displayed (L1): personal use.
        SourcePolicy::new("enj")
            .max_age(15 * MINUTE)
            .personal_use_only(),
        // data.gouv.fr static files (ARCEP, DATAtourisme CSV, snowpack simulation).
        SourcePolicy::new("datagouv").max_age(DAY),
        // geo.api.gouv.fr (commune outlines). 0.75 to 1.3 s per request measured
        // (15/11/2026): 4 in flight, still at most 10/s.
        SourcePolicy::new("geo-api")
            .limits([RateLimit::new(10, SECOND)])
            .max_age(30 * DAY)
            .max_in_flight(4),
        // IGN Géoplateforme WMS-R, LiDAR HD terrain tiles (P5): the answers carry neither
        // ETag nor Last-Modified, so nothing can be revalidated; LiDAR HD editions change
        // rarely. Same per-IP rate as `ign`. Cache 1 year: prudence, non sourcé.
        SourcePolicy::new("ign-lidar-wms")
            .limits([RateLimit::new(20, SECOND)])
            .max_age(365 * DAY)
            .max_in_flight(8),
        // Météo-France public BERA archive (donneespubliques.meteofrance.fr), one index and
        // one bulletin a day: no published quota (prudence, non sourcé).
        SourcePolicy::new("meteofrance-archive").max_age(DAY),
        // Ministry ATOM service (geo-ide), DDT 05 avalanche shapefile, a static dataset: no
        // published quota (prudence, non sourcé).
        SourcePolicy::new("geoide")
            .max_age(30 * DAY)
            .personal_use_only(),
        // Open-Meteo free API: 10 000 calls/day, non-commercial use only
        // (open-meteo.com/en/terms, report 03); forecasts refreshed hourly.
        SourcePolicy::new("open-meteo")
            .limits([RateLimit::new(1, SECOND), RateLimit::per_day(10_000)])
            .max_age(HOUR)
            .personal_use_only(),
        // Nordic France white-label widget: HTML without terms, personal use only, polled at
        // low frequency (prudence, non sourcé).
        SourcePolicy::new("nordicfrance")
            .limits([RateLimit::new(1, MINUTE)])
            .max_age(3 * HOUR)
            .personal_use_only(),
        // Cervières town hall « Info damage » page: same as Nordic France (prudence, non
        // sourcé).
        SourcePolicy::new("cervieres-mairie")
            .limits([RateLimit::new(1, MINUTE)])
            .max_age(3 * HOUR)
            .personal_use_only(),
        // OpenSkiMap / OpenSkiData exports (ski_areas.csv, runs.csv): automated download
        // allowed "at most once per day" (openskidata.org, report 07): each of the two
        // files once a day.
        SourcePolicy::new("openskimap")
            .limits([RateLimit::new(1, MINUTE), RateLimit::per_day(2)])
            .max_age(DAY),
        // SkiRadar geo-feed: 30 requests/min (data.gouv.fr dataservice page, report 03).
        SourcePolicy::new("skiradar").limits([RateLimit::new(30, MINUTE)]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::check_source_name;

    #[test]
    fn known_sources_are_valid_and_unique() {
        let all = known_sources();
        let mut names: Vec<_> = all.iter().map(|p| p.name.as_str()).collect();
        for n in &names {
            check_source_name(n).unwrap();
        }
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), all.len());
        let dt = all.iter().find(|p| p.name == "datatourisme").unwrap();
        assert!(dt.limits.contains(&RateLimit::per_hour(1000)));
        let st = all.iter().find(|p| p.name == "skitour").unwrap();
        assert!(st.limits.contains(&RateLimit::per_day(1000)));
        let om = all.iter().find(|p| p.name == "open-meteo").unwrap();
        assert!(om.limits.contains(&RateLimit::per_day(10_000)));
        let lidar = all.iter().find(|p| p.name == "ign-lidar-wms").unwrap();
        assert_eq!(lidar.max_age, 365 * DAY);
        // Personal-use-only sources (docs/licences-donnees.md).
        let personal: Vec<&str> = all
            .iter()
            .filter(|p| p.personal_use_only)
            .map(|p| p.name.as_str())
            .collect();
        assert_eq!(
            personal,
            [
                "biodiv-sports",
                "enj",
                "geoide",
                "open-meteo",
                "nordicfrance",
                "cervieres-mairie"
            ]
        );
        // Only the latency-bound sources have several requests in flight.
        for p in &all {
            let parallel = matches!(p.name.as_str(), "ign" | "ign-lidar-wms" | "geo-api");
            assert_eq!(p.max_in_flight > 1, parallel, "{}", p.name);
        }
        for n in [
            "meteofrance-archive",
            "geoide",
            "nordicfrance",
            "cervieres-mairie",
            "openskimap",
            "skiradar",
        ] {
            assert!(all.iter().any(|p| p.name == n), "{n}");
        }
    }
}
