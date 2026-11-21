//! The reports on the server, in SQLite (V8 #71). Every function takes the time: the
//! server passes its clock, the tests a simulated one.
//!
//! What is stored, and nothing else:
//! - a device: SHA-256 of a secret salt of the database and of the identifier the phone
//!   sends (random, rotated by the phone every 30 days), its reputation, first and last
//!   day seen (deleted after [`DEVICE_FORGET_DAYS`] days unseen);
//! - a report: kind, position, time seen and received, author (the hashed device), its
//!   reputation when made, expiry;
//! - a vote: report, hashed device, confirm or deny, time, reputation of the voter;
//! - a photo: the JPEG cleaned of its metadata ([`crate::jpeg`]).
//!
//! No IP address, no account, no free text. Expired and refuted reports go with their votes
//! and photo at the next [`Store::purge`] (hourly).

use std::path::Path;
use std::sync::Mutex;

use jiff::{SignedDuration, Timestamp};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::rules::{self, Cast, Kind, State, TimeRefusal, Vote};

/// A device unseen for this long is forgotten (reputation included).
pub const DEVICE_FORGET_DAYS: i64 = 60;

/// Limits per device and for the whole service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Reports per device and hour.
    pub reports_per_hour: u32,
    /// Reports per device and day.
    pub reports_per_day: u32,
    /// Votes per device and hour.
    pub votes_per_hour: u32,
    /// Photos per device and day.
    pub photos_per_day: u32,
    /// Reports per day, all devices.
    pub reports_per_day_total: u32,
    /// Reports returned by one listing.
    pub list_max: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            reports_per_hour: 12,
            reports_per_day: 40,
            votes_per_hour: 60,
            photos_per_day: 10,
            reports_per_day_total: 20_000,
            list_max: 500,
        }
    }
}

/// Why a request is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// Malformed.
    Invalid(String),
    /// No such report (or not visible).
    NotFound,
    /// Not allowed (vote on one's own report, photo of someone else's report).
    Forbidden(String),
    /// Too many requests from this device or for the service.
    RateLimited(String),
    /// Already expired when received: not stored.
    Gone,
    /// Already done (a second photo).
    Conflict(String),
}

/// A new report as the phone sends it.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct NewReport {
    /// Kind identifier ([`Kind::id`]).
    pub kind: String,
    /// Longitude, WGS84.
    pub lon: f64,
    /// Latitude, WGS84.
    pub lat: f64,
    /// When it was seen (the gesture, not the sending).
    pub seen_at: Timestamp,
    /// Anonymous device identifier.
    pub device: String,
}

/// A report as shown.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReportView {
    /// Identifier.
    pub id: String,
    /// Kind identifier.
    pub kind: String,
    /// Longitude.
    pub lon: f64,
    /// Latitude.
    pub lat: f64,
    /// When it was seen.
    pub seen_at: Timestamp,
    /// When it expires (after the confirmations).
    pub expires_at: Timestamp,
    /// State now.
    pub state: State,
    /// Confirmations by others.
    pub confirmations: u32,
    /// Denials.
    pub denials: u32,
    /// Whether a photo is shown (only once confirmed by another device).
    pub photo: bool,
}

/// What a purge removed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Purged {
    /// Reports expired or refuted.
    pub reports: u32,
    /// Photos deleted with them.
    pub photos: u32,
    /// Devices forgotten.
    pub devices: u32,
}

/// Counts for the health check and the cost estimate.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Stats {
    /// Reports stored.
    pub reports: u64,
    /// Votes stored.
    pub votes: u64,
    /// Photos stored.
    pub photos: u64,
    /// Bytes of photos.
    pub photo_bytes: u64,
    /// Devices known.
    pub devices: u64,
}

/// The reports database.
pub struct Store {
    conn: Mutex<Connection>,
    salt: Vec<u8>,
    /// Limits.
    pub limits: Limits,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store")
            .field("limits", &self.limits)
            .finish()
    }
}

type Res<T> = rusqlite::Result<Result<T, Refusal>>;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value BLOB NOT NULL);
CREATE TABLE IF NOT EXISTS devices (
    hash TEXT PRIMARY KEY, reputation REAL NOT NULL, first_seen INTEGER NOT NULL,
    last_seen INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS reports (
    id TEXT PRIMARY KEY, kind TEXT NOT NULL, lon REAL NOT NULL, lat REAL NOT NULL,
    seen INTEGER NOT NULL, received INTEGER NOT NULL, author TEXT NOT NULL,
    author_rep REAL NOT NULL, expires INTEGER NOT NULL, refuted INTEGER NOT NULL DEFAULT 0);
CREATE INDEX IF NOT EXISTS reports_pos ON reports (lat, lon);
CREATE INDEX IF NOT EXISTS reports_expires ON reports (expires);
CREATE INDEX IF NOT EXISTS reports_author ON reports (author, received);
CREATE TABLE IF NOT EXISTS votes (
    report TEXT NOT NULL, device TEXT NOT NULL, vote TEXT NOT NULL, at INTEGER NOT NULL,
    reputation REAL NOT NULL, PRIMARY KEY (report, device));
CREATE INDEX IF NOT EXISTS votes_device ON votes (device, at);
CREATE TABLE IF NOT EXISTS photos (
    report TEXT PRIMARY KEY, bytes BLOB NOT NULL, width INTEGER NOT NULL,
    height INTEGER NOT NULL, created INTEGER NOT NULL, author TEXT NOT NULL);
";

fn ts(t: Timestamp) -> i64 {
    t.as_second()
}

fn at(s: i64) -> Timestamp {
    Timestamp::new(s, 0).ok().unwrap_or_default()
}

/// Random bytes from the operating system (the server runs on Linux), else a hash of the
/// time and the process (identifiers only need to be unpredictable enough not to be
/// guessed; votes and photos also need the author's device).
fn random_bytes(n: usize) -> Vec<u8> {
    use std::io::Read;
    let mut buf = vec![0u8; n];
    if std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut buf))
        .is_ok()
    {
        return buf;
    }
    let mut out = Vec::new();
    let mut i = 0u64;
    while out.len() < n {
        let mut h = Sha256::new();
        h.update(format!(
            "{:?}{}{i}",
            std::time::SystemTime::now(),
            std::process::id()
        ));
        out.extend_from_slice(&h.finalize());
        i += 1;
    }
    out.truncate(n);
    out
}

/// Whether a device identifier has the shape the app sends: 16 to 64 characters, letters,
/// digits and `-`.
pub fn valid_device(id: &str) -> bool {
    (16..=64).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// Whether a report identifier has the shape the store gives.
pub fn valid_id(id: &str) -> bool {
    id.len() == 20 && id.bytes().all(|b| b.is_ascii_hexdigit())
}

impl Store {
    /// Opens (or creates) the database at `path`.
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        Self::with(Connection::open(path)?)
    }

    /// A database in memory (tests).
    pub fn in_memory() -> rusqlite::Result<Self> {
        Self::with(Connection::open_in_memory()?)
    }

    fn with(conn: Connection) -> rusqlite::Result<Self> {
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        conn.execute_batch(SCHEMA)?;
        let salt: Option<Vec<u8>> = conn
            .query_row("SELECT value FROM meta WHERE key = 'salt'", [], |r| {
                r.get(0)
            })
            .optional()?;
        let salt = match salt {
            Some(s) => s,
            None => {
                let s = random_bytes(32);
                conn.execute(
                    "INSERT INTO meta (key, value) VALUES ('salt', ?1)",
                    params![s],
                )?;
                s
            }
        };
        Ok(Self {
            conn: Mutex::new(conn),
            salt,
            limits: Limits::default(),
        })
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// The stored form of a device identifier.
    pub fn device_hash(&self, device: &str) -> String {
        let mut h = Sha256::new();
        h.update(&self.salt);
        h.update(device.as_bytes());
        hex::encode(&h.finalize()[..16])
    }

    /// Whether the database answers.
    pub fn healthy(&self) -> bool {
        self.conn()
            .query_row("SELECT 1", [], |r| r.get::<_, i64>(0))
            .is_ok()
    }

    /// Reputation of a device (known or not: [`rules::START_REPUTATION`]), marking it seen.
    fn touch(c: &Connection, hash: &str, now: Timestamp) -> rusqlite::Result<f64> {
        c.execute(
            "INSERT INTO devices (hash, reputation, first_seen, last_seen) VALUES (?1, ?2, ?3, ?3)
             ON CONFLICT (hash) DO UPDATE SET last_seen = ?3",
            params![hash, rules::START_REPUTATION, ts(now)],
        )?;
        c.query_row(
            "SELECT reputation FROM devices WHERE hash = ?1",
            params![hash],
            |r| r.get(0),
        )
    }

    fn set_reputation(c: &Connection, hash: &str, rep: f64) -> rusqlite::Result<()> {
        c.execute(
            "UPDATE devices SET reputation = ?2 WHERE hash = ?1",
            params![hash, rep],
        )?;
        Ok(())
    }

    /// Reputation of a device as stored (tests, documentation).
    pub fn reputation(&self, device: &str) -> rusqlite::Result<Option<f64>> {
        self.conn()
            .query_row(
                "SELECT reputation FROM devices WHERE hash = ?1",
                params![self.device_hash(device)],
                |r| r.get(0),
            )
            .optional()
    }

    fn count(c: &Connection, sql: &str, p: impl rusqlite::Params) -> rusqlite::Result<u32> {
        c.query_row(sql, p, |r| r.get::<_, u32>(0))
    }

    /// Creates a report. `Gone`: already expired when received (dropped by the queue).
    pub fn create(&self, r: &NewReport, now: Timestamp) -> Res<ReportView> {
        let Some(kind) = Kind::parse(&r.kind) else {
            return Ok(Err(Refusal::Invalid(format!("unknown kind {:?}", r.kind))));
        };
        if !valid_device(&r.device) {
            return Ok(Err(Refusal::Invalid("device identifier".into())));
        }
        if !rules::in_area(r.lon, r.lat) {
            return Ok(Err(Refusal::Invalid(
                "position outside the area served".into(),
            )));
        }
        match rules::accept_observed(kind, r.seen_at, now) {
            Ok(()) => {}
            Err(TimeRefusal::Expired) => return Ok(Err(Refusal::Gone)),
            Err(TimeRefusal::Future) => {
                return Ok(Err(Refusal::Invalid("seen_at in the future".into())));
            }
            Err(TimeRefusal::TooOld) => {
                return Ok(Err(Refusal::Invalid(format!(
                    "seen_at older than {} h",
                    rules::MAX_QUEUE_AGE_H
                ))));
            }
        }
        let author = self.device_hash(&r.device);
        let mut c = self.conn();
        let tx = c.transaction()?;
        let l = self.limits;
        let hour = Self::count(
            &tx,
            "SELECT COUNT(*) FROM reports WHERE author = ?1 AND received > ?2",
            params![author, ts(now - SignedDuration::from_hours(1))],
        )?;
        let day = Self::count(
            &tx,
            "SELECT COUNT(*) FROM reports WHERE author = ?1 AND received > ?2",
            params![author, ts(now - SignedDuration::from_hours(24))],
        )?;
        if hour >= l.reports_per_hour || day >= l.reports_per_day {
            return Ok(Err(Refusal::RateLimited("reports of this device".into())));
        }
        let total = Self::count(
            &tx,
            "SELECT COUNT(*) FROM reports WHERE received > ?1",
            params![ts(now - SignedDuration::from_hours(24))],
        )?;
        if total >= l.reports_per_day_total {
            return Ok(Err(Refusal::RateLimited(
                "daily limit of the service".into(),
            )));
        }
        let rep = Self::touch(&tx, &author, now)?;
        let id = hex::encode(random_bytes(10));
        let eval = rules::evaluate(kind, r.seen_at, rep, &[], now);
        tx.execute(
            "INSERT INTO reports (id, kind, lon, lat, seen, received, author, author_rep, expires)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                id,
                kind.id(),
                r.lon,
                r.lat,
                ts(r.seen_at),
                ts(now),
                author,
                rep,
                ts(eval.expires_at)
            ],
        )?;
        tx.commit()?;
        drop(c);
        self.view(&id, now).map(|v| v.ok_or(Refusal::NotFound))
    }

    fn votes(c: &Connection, id: &str) -> rusqlite::Result<Vec<Cast>> {
        let mut st = c.prepare_cached(
            "SELECT vote, at, reputation FROM votes WHERE report = ?1 ORDER BY at",
        )?;
        st.query_map(params![id], |r| {
            let v: String = r.get(0)?;
            Ok(Cast {
                vote: if v == "confirm" {
                    Vote::Confirm
                } else {
                    Vote::Deny
                },
                at: at(r.get(1)?),
                reputation: r.get(2)?,
            })
        })?
        .collect()
    }

    #[allow(clippy::type_complexity)]
    fn row(
        c: &Connection,
        id: &str,
    ) -> rusqlite::Result<Option<(Kind, f64, f64, Timestamp, f64, String, bool)>> {
        c.query_row(
            "SELECT kind, lon, lat, seen, author_rep, author, refuted FROM reports WHERE id = ?1",
            params![id],
            |r| {
                let k: String = r.get(0)?;
                Ok((
                    Kind::parse(&k).unwrap_or(Kind::Obstacle),
                    r.get(1)?,
                    r.get(2)?,
                    at(r.get(3)?),
                    r.get(4)?,
                    r.get(5)?,
                    r.get::<_, i64>(6)? != 0,
                ))
            },
        )
        .optional()
    }

    fn view_in(c: &Connection, id: &str, now: Timestamp) -> rusqlite::Result<Option<ReportView>> {
        let Some((kind, lon, lat, seen, rep, _, _)) = Self::row(c, id)? else {
            return Ok(None);
        };
        let votes = Self::votes(c, id)?;
        let e = rules::evaluate(kind, seen, rep, &votes, now);
        let has_photo: bool = c
            .query_row(
                "SELECT 1 FROM photos WHERE report = ?1",
                params![id],
                |_| Ok(true),
            )
            .optional()?
            .unwrap_or(false);
        Ok(Some(ReportView {
            id: id.to_owned(),
            kind: kind.id().to_owned(),
            lon,
            lat,
            seen_at: seen,
            expires_at: e.expires_at,
            state: e.state,
            confirmations: e.confirmations,
            denials: e.denials,
            photo: has_photo && e.confirmations > 0 && e.state == State::Active,
        }))
    }

    /// A report, whatever its state (`None`: unknown or purged).
    pub fn view(&self, id: &str, now: Timestamp) -> rusqlite::Result<Option<ReportView>> {
        Self::view_in(&self.conn(), id, now)
    }

    /// Confirms or denies a report.
    pub fn vote(&self, id: &str, device: &str, vote: Vote, now: Timestamp) -> Res<ReportView> {
        if !valid_device(device) {
            return Ok(Err(Refusal::Invalid("device identifier".into())));
        }
        let hash = self.device_hash(device);
        let mut c = self.conn();
        let tx = c.transaction()?;
        let Some((kind, _, _, seen, author_rep, author, refuted)) = Self::row(&tx, id)? else {
            return Ok(Err(Refusal::NotFound));
        };
        if author == hash {
            return Ok(Err(Refusal::Forbidden(
                "no vote on one's own report".into(),
            )));
        }
        let before = rules::evaluate(kind, seen, author_rep, &Self::votes(&tx, id)?, now);
        if matches!(before.state, State::Expired | State::Refuted) {
            return Ok(Err(Refusal::NotFound));
        }
        let n = Self::count(
            &tx,
            "SELECT COUNT(*) FROM votes WHERE device = ?1 AND at > ?2",
            params![hash, ts(now - SignedDuration::from_hours(1))],
        )?;
        if n >= self.limits.votes_per_hour {
            return Ok(Err(Refusal::RateLimited("votes of this device".into())));
        }
        let rep = Self::touch(&tx, &hash, now)?;
        let previous: Option<String> = tx
            .query_row(
                "SELECT vote FROM votes WHERE report = ?1 AND device = ?2",
                params![id, hash],
                |r| r.get(0),
            )
            .optional()?;
        let v = match vote {
            Vote::Confirm => "confirm",
            Vote::Deny => "deny",
        };
        tx.execute(
            "INSERT INTO votes (report, device, vote, at, reputation) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (report, device) DO UPDATE SET vote = ?3, at = ?4, reputation = ?5",
            params![id, hash, v, ts(now), rep],
        )?;
        let after = rules::evaluate(kind, seen, author_rep, &Self::votes(&tx, id)?, now);
        // The author's reputation: a new confirmation by someone else, a refutation.
        let author_now: f64 = tx.query_row(
            "SELECT reputation FROM devices WHERE hash = ?1",
            params![author],
            |r| r.get(0),
        )?;
        let mut new_rep = author_now;
        if vote == Vote::Confirm && previous.as_deref() != Some("confirm") {
            new_rep = rules::after_confirmation(new_rep);
        }
        if after.state == State::Refuted && !refuted {
            new_rep = rules::after_refutation(new_rep);
            tx.execute("UPDATE reports SET refuted = 1 WHERE id = ?1", params![id])?;
        }
        if new_rep != author_now {
            Self::set_reputation(&tx, &author, new_rep)?;
        }
        tx.execute(
            "UPDATE reports SET expires = ?2 WHERE id = ?1",
            params![id, ts(after.expires_at)],
        )?;
        let view = Self::view_in(&tx, id, now)?;
        tx.commit()?;
        Ok(view.ok_or(Refusal::NotFound))
    }

    /// Adds the photo of a report (its author only, once), cleaned of its metadata.
    pub fn put_photo(&self, id: &str, device: &str, jpeg: &[u8], now: Timestamp) -> Res<()> {
        if !valid_device(device) {
            return Ok(Err(Refusal::Invalid("device identifier".into())));
        }
        let clean = match crate::jpeg::clean(jpeg) {
            Ok(c) => c,
            Err(e) => return Ok(Err(Refusal::Invalid(e.to_string()))),
        };
        let hash = self.device_hash(device);
        let c = self.conn();
        let Some((_, _, _, _, _, author, _)) = Self::row(&c, id)? else {
            return Ok(Err(Refusal::NotFound));
        };
        if author != hash {
            return Ok(Err(Refusal::Forbidden(
                "only the author adds a photo".into(),
            )));
        }
        let n = Self::count(
            &c,
            "SELECT COUNT(*) FROM photos WHERE author = ?1 AND created > ?2",
            params![hash, ts(now - SignedDuration::from_hours(24))],
        )?;
        if n >= self.limits.photos_per_day {
            return Ok(Err(Refusal::RateLimited("photos of this device".into())));
        }
        let done = c.execute(
            "INSERT OR IGNORE INTO photos (report, bytes, width, height, created, author)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, clean.bytes, clean.width, clean.height, ts(now), hash],
        )?;
        Ok(if done == 1 {
            Ok(())
        } else {
            Err(Refusal::Conflict("the report already has a photo".into()))
        })
    }

    /// The photo of a report, if shown ([`ReportView::photo`]).
    pub fn photo(&self, id: &str, now: Timestamp) -> rusqlite::Result<Option<Vec<u8>>> {
        let c = self.conn();
        if !Self::view_in(&c, id, now)?.is_some_and(|v| v.photo) {
            return Ok(None);
        }
        c.query_row(
            "SELECT bytes FROM photos WHERE report = ?1",
            params![id],
            |r| r.get(0),
        )
        .optional()
    }

    /// Flags the photo of a report: deleted at once (no moderator to review it).
    pub fn flag_photo(&self, id: &str, device: &str, now: Timestamp) -> Res<()> {
        if !valid_device(device) {
            return Ok(Err(Refusal::Invalid("device identifier".into())));
        }
        let c = self.conn();
        Self::touch(&c, &self.device_hash(device), now)?;
        let n = c.execute("DELETE FROM photos WHERE report = ?1", params![id])?;
        Ok(if n == 1 {
            Ok(())
        } else {
            Err(Refusal::NotFound)
        })
    }

    /// Deletes one's own report (and its votes and photo).
    pub fn delete(&self, id: &str, device: &str) -> Res<()> {
        let hash = self.device_hash(device);
        let mut c = self.conn();
        let tx = c.transaction()?;
        let n = tx.execute(
            "DELETE FROM reports WHERE id = ?1 AND author = ?2",
            params![id, hash],
        )?;
        if n == 0 {
            return Ok(Err(Refusal::NotFound));
        }
        tx.execute("DELETE FROM votes WHERE report = ?1", params![id])?;
        tx.execute("DELETE FROM photos WHERE report = ?1", params![id])?;
        tx.commit()?;
        Ok(Ok(()))
    }

    /// Reports shown in a box (`[min_lon, min_lat, max_lon, max_lat]`), newest first.
    pub fn list(&self, bbox: [f64; 4], now: Timestamp) -> rusqlite::Result<Vec<ReportView>> {
        let c = self.conn();
        let ids: Vec<String> = {
            let mut st = c.prepare_cached(
                "SELECT id FROM reports WHERE lat BETWEEN ?2 AND ?4 AND lon BETWEEN ?1 AND ?3
                 AND expires > ?5 AND refuted = 0 ORDER BY seen DESC LIMIT ?6",
            )?;
            st.query_map(
                params![
                    bbox[0],
                    bbox[1],
                    bbox[2],
                    bbox[3],
                    ts(now),
                    // Held reports are skipped below: read a few more.
                    self.limits.list_max * 2
                ],
                |r| r.get(0),
            )?
            .collect::<rusqlite::Result<_>>()?
        };
        let mut out = Vec::new();
        for id in ids {
            if let Some(v) = Self::view_in(&c, &id, now)?
                && v.state == State::Active
            {
                out.push(v);
                if out.len() >= self.limits.list_max as usize {
                    break;
                }
            }
        }
        Ok(out)
    }

    /// Deletes expired and refuted reports with their votes and photos, and forgets
    /// devices unseen for [`DEVICE_FORGET_DAYS`] days.
    pub fn purge(&self, now: Timestamp) -> rusqlite::Result<Purged> {
        let mut c = self.conn();
        let tx = c.transaction()?;
        let gone = "SELECT id FROM reports WHERE expires <= ?1 OR refuted = 1";
        let photos = tx.execute(
            &format!("DELETE FROM photos WHERE report IN ({gone})"),
            params![ts(now)],
        )?;
        tx.execute(
            &format!("DELETE FROM votes WHERE report IN ({gone})"),
            params![ts(now)],
        )?;
        let reports = tx.execute(
            "DELETE FROM reports WHERE expires <= ?1 OR refuted = 1",
            params![ts(now)],
        )?;
        let devices = tx.execute(
            "DELETE FROM devices WHERE last_seen < ?1",
            params![ts(now - SignedDuration::from_hours(24 * DEVICE_FORGET_DAYS))],
        )?;
        tx.commit()?;
        let n = |x: usize| u32::try_from(x).unwrap_or(u32::MAX);
        Ok(Purged {
            reports: n(reports),
            photos: n(photos),
            devices: n(devices),
        })
    }

    /// Counts.
    pub fn stats(&self) -> rusqlite::Result<Stats> {
        let c = self.conn();
        let q = |sql: &str| {
            c.query_row(sql, [], |r| r.get::<_, i64>(0))
                .map(|v| v.max(0) as u64)
        };
        Ok(Stats {
            reports: q("SELECT COUNT(*) FROM reports")?,
            votes: q("SELECT COUNT(*) FROM votes")?,
            photos: q("SELECT COUNT(*) FROM photos")?,
            photo_bytes: q("SELECT COALESCE(SUM(LENGTH(bytes)), 0) FROM photos")?,
            devices: q("SELECT COUNT(*) FROM devices")?,
        })
    }
}
