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
        }
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
        // OpenSnowMap daily extract planet_pistes.osm.gz (published once a day).
        SourcePolicy::new("opensnowmap").limits([RateLimit::new(1, MINUTE)]),
        // Biodiv'sports: no published quota, to confirm.
        SourcePolicy::new("biodiv-sports").max_age(7 * DAY),
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
        // IGN Géoplateforme (data.geopf.fr): vector TMS limited to 400 req/s per IP.
        SourcePolicy::new("ign")
            .limits([RateLimit::new(20, SECOND)])
            .max_age(30 * DAY),
        // ENJ Snowmap Geotrek API: trail status polled for everyone.
        SourcePolicy::new("enj").max_age(15 * MINUTE),
        // data.gouv.fr static files (ARCEP, DATAtourisme CSV, snowpack simulation).
        SourcePolicy::new("datagouv").max_age(DAY),
        // geo.api.gouv.fr (commune outlines).
        SourcePolicy::new("geo-api")
            .limits([RateLimit::new(10, SECOND)])
            .max_age(30 * DAY),
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
    }
}
