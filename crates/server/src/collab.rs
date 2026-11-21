//! Collaborative reports (V8 #71 to V11 #74): the routes under `/v1/collab/`. The rules,
//! the store and the photo cleaning are in `skimap-collab`; this file only maps HTTP to
//! them, adds a limit per client address in memory (never stored) and runs the purge.
//! Reference: `docs/collab.md`.

use std::path::Path;
use std::sync::Arc;

use jiff::SignedDuration;
use serde::Deserialize;
use skimap_collab::rules::{Kind, Vote};
use skimap_collab::store::{NewReport, Refusal, Store};

use crate::clock::Clock;
use crate::http::{Reply, Req};
use crate::ratelimit::Limiter;

/// Writes per client address and hour (all devices behind it: a refuge's Wi-Fi, a car
/// park's 4G cell): far above a group's use, far below a script's.
const WRITES_PER_ADDRESS_HOUR: usize = 240;
/// Header of the anonymous device identifier on writes.
pub const DEVICE_HEADER: &str = "x-skimap-device";

/// The collaborative service.
pub struct Collab {
    /// Reports, votes, photos.
    pub store: Store,
    limiter: Limiter,
}

impl Collab {
    /// Over a store.
    pub fn new(store: Store) -> Self {
        Self {
            store,
            limiter: Limiter::default(),
        }
    }

    /// Opens the database at `path` (`SKIMAP_COLLAB_DB`).
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        Ok(Self::new(Store::open(path)?))
    }

    /// Hourly: deletes expired and refuted reports, their votes and photos.
    pub fn purge(&self, clock: &dyn Clock) {
        match self.store.purge(clock.now()) {
            Ok(p) if p != Default::default() => tracing::info!(?p, "collab purge"),
            Ok(_) => {}
            Err(e) => tracing::error!(error = %e, "collab purge"),
        }
    }
}

#[derive(Deserialize)]
struct DeviceBody {
    device: String,
}

fn refusal(r: Refusal) -> Reply {
    match r {
        Refusal::Invalid(m) => Reply::error(400, "invalid", m),
        Refusal::NotFound => Reply::error(404, "not_found", "no such report"),
        Refusal::Forbidden(m) => Reply::error(403, "forbidden", m),
        Refusal::RateLimited(m) => Reply::error(429, "rate_limited", m).with("Retry-After", "600"),
        Refusal::Gone => Reply::error(410, "expired", "already expired: not stored"),
        Refusal::Conflict(m) => Reply::error(409, "conflict", m),
    }
}

fn internal(e: &dyn std::fmt::Display) -> Reply {
    tracing::error!(error = %e, "collab internal error");
    Reply::error(500, "internal", "internal error")
}

/// The device of a write: the header, else `{"device": ...}` in a JSON body.
fn device(req: &Req) -> Option<String> {
    req.get(DEVICE_HEADER)
        .map(|s| s.trim().to_owned())
        .or_else(|| {
            serde_json::from_slice::<DeviceBody>(&req.body)
                .ok()
                .map(|b| b.device)
        })
}

fn bbox(req: &Req) -> Option<[f64; 4]> {
    let v: Vec<f64> = req
        .param("bbox")?
        .split(',')
        .map(|s| s.trim().parse().ok())
        .collect::<Option<_>>()?;
    let b: [f64; 4] = v.try_into().ok()?;
    // At most 2° by 2°: a domain or a massif, not France at once.
    (b.iter().all(|x| x.is_finite())
        && b[0] < b[2]
        && b[1] < b[3]
        && b[2] - b[0] <= 2.0
        && b[3] - b[1] <= 2.0)
        .then_some(b)
}

/// Rules shown by the app and the documentation.
fn rules() -> serde_json::Value {
    serde_json::json!({
        "kinds": Kind::ALL.iter().map(|k| serde_json::json!({
            "id": k.id(), "fr": k.fr(), "lifetime_hours": k.lifetime().as_hours()
        })).collect::<Vec<_>>(),
        "max_lifetimes": skimap_collab::rules::MAX_LIFETIMES,
        "max_queue_age_hours": skimap_collab::rules::MAX_QUEUE_AGE_H,
        "photo_max_bytes": skimap_collab::jpeg::MAX_BYTES,
        "photo_max_side": skimap_collab::jpeg::MAX_SIDE,
        "free_text": false,
    })
}

/// `/v1/collab/...`.
pub fn route(collab: Option<&Arc<Collab>>, clock: &dyn Clock, req: &Req, rest: &[&str]) -> Reply {
    let Some(c) = collab else {
        return Reply::error(503, "unavailable", "reports are not configured");
    };
    let now = clock.now();
    let read = req.is_read();
    if !read
        && !c.limiter.allow(
            &format!("collab:{}", req.client),
            WRITES_PER_ADDRESS_HOUR,
            SignedDuration::from_hours(1),
            now,
        )
    {
        return Reply::error(429, "rate_limited", "too many requests from this address")
            .with("Retry-After", "600");
    }
    let s = &c.store;
    let result: rusqlite::Result<Reply> = (|| {
        Ok(match (req.method.as_str(), rest) {
            ("GET" | "HEAD", ["rules"]) => {
                Reply::json(200, &rules()).with("Cache-Control", "public, max-age=3600")
            }
            ("GET" | "HEAD", ["reports"]) => {
                let Some(b) = bbox(req) else {
                    return Ok(Reply::error(
                        400,
                        "invalid",
                        "bbox=min_lon,min_lat,max_lon,max_lat, at most 2° a side",
                    ));
                };
                let list = s.list(b, now)?;
                Reply::json(200, &serde_json::json!({"now": now, "reports": list}))
                    .with("Cache-Control", "public, max-age=60")
            }
            ("POST", ["reports"]) => {
                let r: NewReport = match serde_json::from_slice(&req.body) {
                    Ok(r) => r,
                    Err(e) => return Ok(Reply::error(400, "bad_request", format!("report: {e}"))),
                };
                match s.create(&r, now)? {
                    Ok(v) => Reply::json(201, &v).with("Cache-Control", "no-store"),
                    Err(r) => refusal(r),
                }
            }
            ("GET" | "HEAD", ["reports", id]) => match s.view(id, now)? {
                Some(v) => Reply::json(200, &v).with("Cache-Control", "no-store"),
                None => refusal(Refusal::NotFound),
            },
            ("DELETE", ["reports", id]) => {
                let Some(d) = device(req) else {
                    return Ok(refusal(Refusal::Invalid("device".into())));
                };
                match s.delete(id, &d)? {
                    Ok(()) => Reply::text(204, ""),
                    Err(r) => refusal(r),
                }
            }
            ("POST", ["reports", id, op @ ("confirm" | "deny")]) => {
                let Some(d) = device(req) else {
                    return Ok(refusal(Refusal::Invalid("device".into())));
                };
                let vote = if *op == "confirm" {
                    Vote::Confirm
                } else {
                    Vote::Deny
                };
                match s.vote(id, &d, vote, now)? {
                    Ok(v) => Reply::json(200, &v).with("Cache-Control", "no-store"),
                    Err(r) => refusal(r),
                }
            }
            ("POST", ["reports", id, "photo"]) => {
                let Some(d) = req.get(DEVICE_HEADER) else {
                    return Ok(refusal(Refusal::Invalid(format!("{DEVICE_HEADER} header"))));
                };
                match s.put_photo(id, d.trim(), &req.body, now)? {
                    Ok(()) => Reply::text(204, ""),
                    Err(r) => refusal(r),
                }
            }
            ("POST", ["reports", id, "photo", "flag"]) => {
                let Some(d) = device(req) else {
                    return Ok(refusal(Refusal::Invalid("device".into())));
                };
                match s.flag_photo(id, &d, now)? {
                    Ok(()) => Reply::text(204, ""),
                    Err(r) => refusal(r),
                }
            }
            ("GET" | "HEAD", ["photos", name]) => {
                let id = name.strip_suffix(".jpg").unwrap_or(name);
                match s.photo(id, now)? {
                    Some(b) => Reply::bytes(200, "image/jpeg", b)
                        .with("Cache-Control", "public, max-age=300"),
                    None => Reply::error(404, "not_found", "no photo shown for this report"),
                }
            }
            _ => Reply::error(404, "not_found", "no such endpoint"),
        })
    })();
    result.unwrap_or_else(|e| internal(&e))
}
