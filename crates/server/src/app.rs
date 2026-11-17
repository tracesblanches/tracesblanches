//! Routes. API reference for the app (K2): `docs/api.md`.

use std::sync::Arc;

use chrono::Duration;
use serde::Deserialize;

use crate::alerts::{Alerts, Position, Refusal, TripPlan};
use crate::clock::Clock;
use crate::http::{Handler, Reply, Req, file_reply};
use crate::ratelimit::Limiter;
use crate::store::Store;
use crate::strava::Strava;

/// Cache header of immutable contents (bundles, named by their SHA-256).
pub const IMMUTABLE: &str = "public, max-age=31536000, immutable";
/// Cache header of the catalog and the site indexes (Cloudflare revalidates them at most
/// once a minute per edge, whatever the number of phones).
pub const REVALIDATE: &str = "public, max-age=60, must-revalidate";

/// Requests per client and hour on the write endpoints.
const TRIP_CREATE_PER_HOUR: usize = 10;
const TRIP_CALLS_PER_HOUR: usize = 240;
const STRAVA_PER_HOUR: usize = 60;
/// Trips created per day, all clients together.
const TRIPS_PER_DAY: usize = 500;

/// The application.
pub struct App {
    /// Published packs.
    pub store: Store,
    /// Non-return alerts (absent: the endpoints answer 503).
    pub alerts: Option<Arc<Alerts>>,
    /// Strava exchange (absent: 503).
    pub strava: Option<Strava>,
    /// Strava config for `/v1/strava/config`.
    pub strava_public: crate::strava::PublicConfig,
    /// App redirect of the callback.
    pub strava_app_redirect: String,
    /// Publisher named on the privacy page (`SKIMAP_CONTROLLER`).
    pub controller: String,
    /// Privacy contact.
    pub contact_email: String,
    /// Public URL.
    pub public_url: String,
    /// Time.
    pub clock: Arc<dyn Clock>,
    /// Collaborative reports (V8; absent: 503).
    pub collab: Option<Arc<crate::collab::Collab>>,
    limiter: Limiter,
}

impl App {
    /// An application over these parts.
    pub fn new(
        store: Store,
        alerts: Option<Arc<Alerts>>,
        strava: Option<(Strava, crate::strava::StravaConfig)>,
        public_url: &str,
        contact_email: &str,
        clock: Arc<dyn Clock>,
    ) -> Self {
        let (strava, cfg) = match strava {
            Some((s, c)) => (Some(s), Some(c)),
            None => (None, None),
        };
        Self {
            strava_public: Strava::public_config(cfg.as_ref(), public_url),
            strava_app_redirect: cfg.as_ref().map_or_else(
                || "skimap://strava/callback".to_owned(),
                |c| c.app_redirect.clone(),
            ),
            store,
            alerts,
            strava,
            controller: "l'éditeur de l'application skimap".to_owned(),
            contact_email: contact_email.to_owned(),
            public_url: public_url.to_owned(),
            clock,
            collab: None,
            limiter: Limiter::default(),
        }
    }

    fn limited(&self, req: &Req, bucket: &str, per_hour: usize) -> Option<Reply> {
        let key = format!("{bucket}:{}", req.client);
        (!self
            .limiter
            .allow(&key, per_hour, Duration::hours(1), self.clock.now()))
        .then(|| {
            Reply::error(429, "rate_limited", "too many requests, try again later")
                .with("Retry-After", "600")
        })
    }

    fn healthz(&self) -> Reply {
        let store_ok = self.store.public().is_dir();
        let db_ok = self.alerts.as_ref().is_none_or(|a| a.healthy());
        // The alert timer must have turned in the last two minutes (once started).
        let timer_ok = self.alerts.as_ref().is_none_or(|a| {
            a.last_tick()
                .is_none_or(|t| self.clock.now() - t <= Duration::seconds(120))
        });
        if store_ok && db_ok && timer_ok {
            Reply::text(200, "ok\n").with("Cache-Control", "no-store")
        } else {
            Reply::text(
                503,
                &format!("store {store_ok}, database {db_ok}, alert timer {timer_ok}\n"),
            )
            .with("Cache-Control", "no-store")
        }
    }

    /// A published file that changes at each publication (catalog, index): short cache,
    /// `ETag` from its size and modification time (no hashing per request).
    fn mutable(&self, req: &Req, path: Option<std::path::PathBuf>) -> Reply {
        let Some(path) = path else {
            return Reply::error(404, "not_found", "no such site");
        };
        let etag = match std::fs::metadata(&path) {
            Ok(m) => {
                let t = m
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map_or(0, |d| d.as_nanos());
                format!("\"{:x}-{t:x}\"", m.len())
            }
            Err(_) => return Reply::error(404, "not_found", "nothing published"),
        };
        file_reply(req, &path, &etag, "application/json", REVALIDATE)
    }

    /// A bundle: immutable, named by its SHA-256, byte ranges for resuming.
    fn bundle(&self, req: &Req, name: &str) -> Reply {
        match name
            .strip_suffix(".skb")
            .and_then(|sha| self.store.bundle_path(sha).map(|p| (sha, p)))
        {
            Some((sha, p)) => file_reply(
                req,
                &p,
                &format!("\"{sha}\""),
                "application/octet-stream",
                IMMUTABLE,
            ),
            None => Reply::error(404, "not_found", "no such bundle"),
        }
    }

    fn trips(&self, req: &Req, rest: &[&str]) -> Reply {
        let Some(alerts) = &self.alerts else {
            return Reply::error(503, "unavailable", "alerts are not configured");
        };
        let token = req
            .get("authorization")
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(str::trim)
            .unwrap_or("");
        let result = match (req.method.as_str(), rest) {
            ("POST", []) => {
                if let Some(r) = self.limited(req, "trip-create", TRIP_CREATE_PER_HOUR) {
                    return r;
                }
                if !self.limiter.allow(
                    "trips-global",
                    TRIPS_PER_DAY,
                    Duration::days(1),
                    self.clock.now(),
                ) {
                    return Reply::error(429, "rate_limited", "daily limit of the service reached")
                        .with("Retry-After", "3600");
                }
                let plan: TripPlan = match serde_json::from_slice(&req.body) {
                    Ok(p) => p,
                    Err(e) => return Reply::error(400, "bad_request", format!("plan: {e}")),
                };
                return match alerts.create(plan) {
                    Ok(Ok(c)) => Reply::json(201, &c).with("Cache-Control", "no-store"),
                    Ok(Err(r)) => refusal(r),
                    Err(e) => internal(&e),
                };
            }
            (m, [id, ..]) => {
                if let Some(r) = self.limited(req, "trip", TRIP_CALLS_PER_HOUR) {
                    return r;
                }
                match (m, &rest[1..]) {
                    ("GET" | "HEAD", []) => match alerts.get(id, token) {
                        Ok(Some(t)) => Ok(Ok(t)),
                        Ok(None) => Ok(Err(Refusal::NotFound)),
                        Err(e) => Err(e),
                    },
                    ("POST", ["checkin"]) => alerts.check_in(id, token),
                    ("POST", ["cancel"]) | ("DELETE", []) => alerts.cancel(id, token),
                    ("POST", ["extend"]) => {
                        #[derive(Deserialize)]
                        struct B {
                            deadline: chrono::DateTime<chrono::Utc>,
                        }
                        match serde_json::from_slice::<B>(&req.body) {
                            Ok(b) => alerts.extend(id, token, b.deadline),
                            Err(e) => {
                                return Reply::error(400, "bad_request", format!("{e}"));
                            }
                        }
                    }
                    ("POST", ["position"]) => match serde_json::from_slice::<Position>(&req.body) {
                        Ok(p) => alerts.position(id, token, p),
                        Err(e) => return Reply::error(400, "bad_request", format!("{e}")),
                    },
                    _ => return Reply::error(404, "not_found", "no such endpoint"),
                }
            }
            _ => return Reply::error(404, "not_found", "no such endpoint"),
        };
        match result {
            Ok(Ok(t)) => Reply::json(200, &t).with("Cache-Control", "no-store"),
            Ok(Err(r)) => refusal(r),
            Err(e) => internal(&e),
        }
    }

    fn strava(&self, req: &Req, rest: &[&str]) -> Reply {
        match (req.method.as_str(), rest) {
            ("GET" | "HEAD", ["config"]) => {
                Reply::json(200, &self.strava_public).with("Cache-Control", "no-cache")
            }
            ("GET" | "HEAD", ["callback"]) => {
                crate::strava::callback(&self.strava_app_redirect, req)
            }
            ("POST", [op @ ("token" | "refresh" | "revoke")]) => {
                if let Some(r) = self.limited(req, "strava", STRAVA_PER_HOUR) {
                    return r;
                }
                let Some(s) = &self.strava else {
                    return Reply::error(503, "unavailable", "Strava is not configured");
                };
                match *op {
                    "token" => s.exchange(req),
                    "refresh" => s.refresh(req),
                    _ => s.revoke(req),
                }
            }
            _ => Reply::error(404, "not_found", "no such endpoint"),
        }
    }
}

fn refusal(r: Refusal) -> Reply {
    match r {
        Refusal::Invalid(m) => Reply::error(400, "invalid", m),
        Refusal::NotFound => Reply::error(404, "not_found", "no such trip, or wrong token"),
        Refusal::Conflict(m) => Reply::error(409, "conflict", m),
    }
}

fn internal(e: &anyhow::Error) -> Reply {
    tracing::error!(error = %format!("{e:#}"), "internal error");
    Reply::error(500, "internal", "internal error")
}

impl Handler for App {
    fn handle(&self, req: &Req) -> Reply {
        let parts: Vec<&str> = req.path.split('/').filter(|s| !s.is_empty()).collect();
        if req.method == "OPTIONS" {
            return Reply {
                status: 204,
                headers: vec![
                    ("Access-Control-Allow-Origin".into(), "*".into()),
                    (
                        "Access-Control-Allow-Methods".into(),
                        "GET, HEAD, POST, DELETE, OPTIONS".into(),
                    ),
                    (
                        "Access-Control-Allow-Headers".into(),
                        "Range, If-None-Match, If-Range, Content-Type, Authorization".into(),
                    ),
                    ("Access-Control-Max-Age".into(), "86400".into()),
                ],
                body: crate::http::Body::Empty,
            };
        }
        let read = req.is_read();
        match parts.as_slice() {
            [] if read => Reply::html(200, &crate::privacy::index())
                .with("Cache-Control", "public, max-age=3600"),
            ["healthz"] if read => self.healthz(),
            ["privacy" | "privacy.html" | "confidentialite"] if read => Reply::html(
                200,
                &crate::privacy::page(&self.controller, &self.contact_email, &self.public_url),
            )
            .with("Cache-Control", "public, max-age=3600"),
            ["v1", "catalog.json" | "catalog"] if read => {
                self.mutable(req, Some(self.store.catalog_path()))
            }
            ["v1", "sites", site, "index.json"] if read => {
                self.mutable(req, self.store.index_path(site))
            }
            ["v1", "sites", site, "voice", lang, "index.json"] if read => {
                self.mutable(req, self.store.voice_index_path(site, lang))
            }
            ["v1", "bundles", name] if read => self.bundle(req, name),
            ["v1", "trips", rest @ ..] => self.trips(req, rest),
            ["v1", "strava", rest @ ..] => self.strava(req, rest),
            ["v1", "collab", rest @ ..] => {
                crate::collab::route(self.collab.as_ref(), self.clock.as_ref(), req, rest)
            }
            _ if !read && !matches!(req.method.as_str(), "POST" | "DELETE") => {
                Reply::error(405, "method_not_allowed", "method not allowed")
                    .with("Allow", "GET, HEAD, POST, DELETE, OPTIONS")
            }
            _ => Reply::error(404, "not_found", "no such endpoint"),
        }
    }
}
