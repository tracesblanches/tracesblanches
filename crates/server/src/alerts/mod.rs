//! Non-return alert (S2): the phone leaves a trip plan with a return deadline before going
//! out; if nobody confirms the return by the deadline plus a grace delay, the server
//! e-mails the contacts, even if the phone is dead or out of coverage. A confirmation
//! after the alert sends a second message ("rentré").
//!
//! State is in SQLite (`trips`, `outbox`); the timer is [`Alerts::tick`], called every few
//! seconds by a thread in production and by hand with a [`crate::clock::ManualClock`] in
//! tests. Messages go through an outbox with retries, one row per recipient.

pub mod mail;

use std::path::Path;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::clock::Clock;
use crate::util::{clean_text, paris_time, random_hex, sha256_hex, valid_email};
use mail::{Email, Mailer};

/// Limits on what a plan may hold.
pub mod limits {
    /// Contacts per trip.
    pub const MAX_CONTACTS: usize = 3;
    /// Characters of the skier's name.
    pub const MAX_NAME: usize = 80;
    /// Characters of the plan text.
    pub const MAX_PLAN: usize = 2000;
    /// Points of the route.
    pub const MAX_ROUTE_POINTS: usize = 2000;
    /// Earliest deadline, minutes from now.
    pub const MIN_LEAD_MINUTES: i64 = 5;
    /// Latest deadline, days from now (multi-day trips with a bivouac).
    pub const MAX_DAYS: i64 = 7;
    /// Largest grace delay, minutes.
    pub const MAX_GRACE_MINUTES: u32 = 180;
    /// Default grace delay, minutes.
    pub const DEFAULT_GRACE_MINUTES: u32 = 30;
    /// Days a closed trip is kept before deletion.
    pub const KEEP_DAYS: i64 = 7;
    /// Hours a message is retried before it is marked failed.
    pub const RETRY_HOURS: i64 = 24;
}

/// A position reported by the phone.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Position {
    /// Longitude, WGS84.
    pub lon: f64,
    /// Latitude, WGS84.
    pub lat: f64,
    /// Time of the fix (default: when received).
    #[serde(default)]
    pub at: Option<DateTime<Utc>>,
    /// Accuracy, metres.
    #[serde(default)]
    pub accuracy_m: Option<f64>,
}

/// What the phone sends when leaving (`POST /v1/trips`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TripPlan {
    /// Name the contacts know the skier by.
    pub skier_name: String,
    /// Skier's phone number, for the contacts to call first.
    #[serde(default)]
    pub skier_phone: Option<String>,
    /// E-mail addresses of 1 to 3 contacts.
    pub contacts: Vec<String>,
    /// Return deadline, RFC 3339.
    pub deadline: DateTime<Utc>,
    /// Delay after the deadline before alerting, minutes (default 30, at most 180).
    #[serde(default)]
    pub grace_minutes: Option<u32>,
    /// Site identifier (`cervieres`, `chapelle-des-bois`), if any.
    #[serde(default)]
    pub site: Option<String>,
    /// The plan in words: route, start, equipment, people.
    pub plan: String,
    /// Planned route `[lon, lat]` (from the phone's routing), optional.
    #[serde(default)]
    pub route: Vec<[f64; 2]>,
    /// Current position when leaving, optional.
    #[serde(default)]
    pub position: Option<Position>,
}

/// Why a request was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// Invalid input; the message says what.
    Invalid(String),
    /// No such trip, or wrong token.
    NotFound,
    /// Not possible in the trip's state.
    Conflict(String),
}

/// A trip as the phone sees it (`GET /v1/trips/<id>`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TripStatus {
    /// Identifier.
    pub id: String,
    /// `active`, `alerted`, `returned`, `cancelled`.
    pub state: String,
    /// Deadline.
    pub deadline: DateTime<Utc>,
    /// When the alert goes (or went) out: deadline + grace.
    pub alert_at: DateTime<Utc>,
    /// Creation time.
    pub created_at: DateTime<Utc>,
    /// Alert time, if alerted.
    pub alerted_at: Option<DateTime<Utc>>,
    /// Return or cancellation time.
    pub closed_at: Option<DateTime<Utc>>,
    /// Last position received.
    pub last_position: Option<Position>,
    /// Messages to the contacts.
    pub notifications: Vec<Notification>,
}

/// One message to one contact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notification {
    /// `alert` or `returned`.
    pub kind: String,
    /// Recipient.
    pub to: String,
    /// `pending`, `sent`, `failed`, `cancelled`.
    pub status: String,
    /// Attempts so far.
    pub attempts: u32,
    /// When sent.
    pub sent_at: Option<DateTime<Utc>>,
}

/// Reply to a creation: the token is shown once, only its hash is stored.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Created {
    /// Trip identifier.
    pub id: String,
    /// Bearer token for every later call on the trip.
    pub token: String,
    /// The trip.
    pub trip: TripStatus,
}

/// What a [`Alerts::tick`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct TickReport {
    /// Trips that passed their deadline + grace.
    pub alerted: u32,
    /// Messages delivered.
    pub sent: u32,
    /// Delivery attempts that failed (retried later).
    pub retried: u32,
    /// Messages given up after [`limits::RETRY_HOURS`].
    pub failed: u32,
    /// Trips deleted (closed for more than [`limits::KEEP_DAYS`]).
    pub purged: u32,
    /// Messages held back by the daily cap.
    pub capped: u32,
}

const SCHEMA: &str = "
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;
CREATE TABLE IF NOT EXISTS trips (
    id TEXT PRIMARY KEY,
    token_sha256 TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    deadline INTEGER NOT NULL,
    grace_s INTEGER NOT NULL,
    state TEXT NOT NULL,
    skier_name TEXT NOT NULL,
    skier_phone TEXT,
    contacts TEXT NOT NULL,
    site TEXT,
    plan TEXT NOT NULL,
    route TEXT NOT NULL,
    last_lon REAL, last_lat REAL, last_at INTEGER, last_accuracy REAL,
    alerted_at INTEGER,
    closed_at INTEGER
);
CREATE INDEX IF NOT EXISTS trips_due ON trips(state, deadline);
CREATE TABLE IF NOT EXISTS outbox (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    trip_id TEXT NOT NULL REFERENCES trips(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    recipient TEXT NOT NULL,
    subject TEXT NOT NULL,
    body TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    next_attempt_at INTEGER NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0,
    sent_at INTEGER,
    failed_at INTEGER,
    cancelled_at INTEGER,
    last_error TEXT
);
CREATE INDEX IF NOT EXISTS outbox_due ON outbox(sent_at, failed_at, cancelled_at, next_attempt_at);
";

fn ts(t: DateTime<Utc>) -> i64 {
    t.timestamp()
}

fn from_ts(s: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(s, 0).unwrap_or_default()
}

/// The alert service.
pub struct Alerts {
    db: Mutex<Connection>,
    mailer: Arc<dyn Mailer>,
    clock: Arc<dyn Clock>,
    /// Public URL of the server, quoted in the messages.
    public_url: String,
    /// Most messages sent in 24 h (stays under the relay's daily quota).
    pub daily_cap: u32,
    /// Unix time of the last [`Alerts::tick`] (0: never), for the health check.
    last_tick: std::sync::atomic::AtomicI64,
}

struct TripRow {
    id: String,
    token_sha256: String,
    created_at: i64,
    deadline: i64,
    grace_s: i64,
    state: String,
    skier_name: String,
    skier_phone: Option<String>,
    contacts: Vec<String>,
    site: Option<String>,
    plan: String,
    route: Vec<[f64; 2]>,
    last: Option<Position>,
    alerted_at: Option<i64>,
    closed_at: Option<i64>,
}

const TRIP_COLUMNS: &str = "id, token_sha256, created_at, deadline, grace_s, state, skier_name, \
    skier_phone, contacts, site, plan, route, last_lon, last_lat, last_at, last_accuracy, \
    alerted_at, closed_at";

fn trip_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<TripRow> {
    let contacts: String = r.get(8)?;
    let route: String = r.get(11)?;
    let lon: Option<f64> = r.get(12)?;
    let lat: Option<f64> = r.get(13)?;
    let at: Option<i64> = r.get(14)?;
    Ok(TripRow {
        id: r.get(0)?,
        token_sha256: r.get(1)?,
        created_at: r.get(2)?,
        deadline: r.get(3)?,
        grace_s: r.get(4)?,
        state: r.get(5)?,
        skier_name: r.get(6)?,
        skier_phone: r.get(7)?,
        contacts: serde_json::from_str(&contacts).unwrap_or_default(),
        site: r.get(9)?,
        plan: r.get(10)?,
        route: serde_json::from_str(&route).unwrap_or_default(),
        last: match (lon, lat) {
            (Some(lon), Some(lat)) => Some(Position {
                lon,
                lat,
                at: at.map(from_ts),
                accuracy_m: r.get(15)?,
            }),
            _ => None,
        },
        alerted_at: r.get(16)?,
        closed_at: r.get(17)?,
    })
}

fn valid_position(p: &Position) -> bool {
    p.lon.is_finite()
        && p.lat.is_finite()
        && (-180.0..=180.0).contains(&p.lon)
        && (-90.0..=90.0).contains(&p.lat)
        && p.accuracy_m.is_none_or(|a| a.is_finite() && a >= 0.0)
}

fn fmt_point(lat: f64, lon: f64) -> String {
    format!(
        "{lat:.5}, {lon:.5} (https://www.openstreetmap.org/?mlat={lat:.5}&mlon={lon:.5}#map=15/{lat:.5}/{lon:.5})"
    )
}

impl Alerts {
    /// Opens (and creates) the database.
    pub fn open(
        path: &Path,
        mailer: Arc<dyn Mailer>,
        clock: Arc<dyn Clock>,
        public_url: &str,
    ) -> Result<Self> {
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p).with_context(|| format!("creating {}", p.display()))?;
        }
        let db = Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
        db.busy_timeout(std::time::Duration::from_secs(5))?;
        db.execute_batch(SCHEMA)?;
        Ok(Self {
            db: Mutex::new(db),
            mailer,
            clock,
            public_url: public_url.trim_end_matches('/').to_owned(),
            daily_cap: 400,
            last_tick: std::sync::atomic::AtomicI64::new(0),
        })
    }

    fn db(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.db.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Whether the database answers (health check).
    pub fn healthy(&self) -> bool {
        self.db()
            .query_row("SELECT 1", [], |r| r.get::<_, i64>(0))
            .is_ok()
    }

    /// Time of the last [`Alerts::tick`], `None` before the first one.
    pub fn last_tick(&self) -> Option<DateTime<Utc>> {
        match self.last_tick.load(std::sync::atomic::Ordering::Relaxed) {
            0 => None,
            t => Some(from_ts(t)),
        }
    }

    /// The mailer, described.
    pub fn mailer(&self) -> String {
        self.mailer.describe()
    }

    /// Records a plan.
    pub fn create(&self, plan: TripPlan) -> Result<std::result::Result<Created, Refusal>> {
        let now = self.clock.now();
        let invalid = |m: &str| Ok(Err(Refusal::Invalid(m.to_owned())));
        let name = clean_text(&plan.skier_name).replace('\n', " ");
        if name.is_empty() || name.chars().count() > limits::MAX_NAME {
            return invalid("skier_name: 1 to 80 characters");
        }
        let phone = plan
            .skier_phone
            .as_deref()
            .map(str::trim)
            .filter(|p| !p.is_empty());
        if let Some(p) = phone
            && (p.len() > 30
                || !p
                    .chars()
                    .all(|c| c.is_ascii_digit() || " +-.()".contains(c)))
        {
            return invalid("skier_phone: digits, spaces and + - . ( ) only, 30 at most");
        }
        let mut contacts: Vec<String> = plan
            .contacts
            .iter()
            .map(|c| c.trim().to_ascii_lowercase())
            .collect();
        contacts.dedup();
        if contacts.is_empty() || contacts.len() > limits::MAX_CONTACTS {
            return invalid("contacts: 1 to 3 e-mail addresses");
        }
        if let Some(bad) = contacts.iter().find(|c| !valid_email(c)) {
            return invalid(&format!("contacts: {bad:?} is not an e-mail address"));
        }
        if plan.deadline < now + Duration::minutes(limits::MIN_LEAD_MINUTES)
            || plan.deadline > now + Duration::days(limits::MAX_DAYS)
        {
            return invalid("deadline: between 5 minutes and 7 days from now");
        }
        let grace = plan.grace_minutes.unwrap_or(limits::DEFAULT_GRACE_MINUTES);
        if grace > limits::MAX_GRACE_MINUTES {
            return invalid("grace_minutes: 0 to 180");
        }
        let text = clean_text(&plan.plan);
        if text.chars().count() > limits::MAX_PLAN {
            return invalid("plan: 2000 characters at most");
        }
        let site = plan
            .site
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        if let Some(s) = site
            && skimap_common::site::site(s).is_err()
        {
            return invalid("site: unknown site");
        }
        if plan.route.len() > limits::MAX_ROUTE_POINTS
            || plan.route.iter().any(|[lon, lat]| {
                !valid_position(&Position {
                    lon: *lon,
                    lat: *lat,
                    at: None,
                    accuracy_m: None,
                })
            })
        {
            return invalid("route: at most 2000 [lon, lat] points");
        }
        if let Some(p) = &plan.position
            && !valid_position(p)
        {
            return invalid("position: bad coordinates");
        }
        let id = random_hex(16)?;
        let token = random_hex(32)?;
        let pos = plan.position;
        self.db().execute(
            &format!(
                "INSERT INTO trips ({TRIP_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, 'active', ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, NULL, NULL)"
            ),
            params![
                id,
                sha256_hex(&token),
                ts(now),
                ts(plan.deadline),
                i64::from(grace) * 60,
                name,
                phone,
                serde_json::to_string(&contacts)?,
                site,
                text,
                serde_json::to_string(&plan.route)?,
                pos.map(|p| p.lon),
                pos.map(|p| p.lat),
                pos.map(|p| ts(p.at.unwrap_or(now))),
                pos.and_then(|p| p.accuracy_m),
            ],
        )?;
        let trip = self.status(&id)?.expect("trip just inserted");
        Ok(Ok(Created { id, token, trip }))
    }

    fn row(&self, id: &str) -> Result<Option<TripRow>> {
        Ok(self
            .db()
            .query_row(
                &format!("SELECT {TRIP_COLUMNS} FROM trips WHERE id = ?1"),
                [id],
                trip_row,
            )
            .optional()?)
    }

    /// The trip if `token` is its token.
    fn authorized(&self, id: &str, token: &str) -> Result<Option<TripRow>> {
        if id.len() != 32 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Ok(None);
        }
        Ok(self
            .row(id)?
            .filter(|t| crate::util::constant_time_eq(&t.token_sha256, &sha256_hex(token))))
    }

    fn status(&self, id: &str) -> Result<Option<TripStatus>> {
        let Some(t) = self.row(id)? else {
            return Ok(None);
        };
        let db = self.db();
        let mut st = db.prepare(
            "SELECT kind, recipient, sent_at, failed_at, cancelled_at, attempts FROM outbox WHERE trip_id = ?1 ORDER BY id",
        )?;
        let notifications = st
            .query_map([id], |r| {
                let sent: Option<i64> = r.get(2)?;
                let failed: Option<i64> = r.get(3)?;
                let cancelled: Option<i64> = r.get(4)?;
                Ok(Notification {
                    kind: r.get(0)?,
                    to: r.get(1)?,
                    status: if sent.is_some() {
                        "sent"
                    } else if cancelled.is_some() {
                        "cancelled"
                    } else if failed.is_some() {
                        "failed"
                    } else {
                        "pending"
                    }
                    .to_owned(),
                    attempts: r.get(5)?,
                    sent_at: sent.map(from_ts),
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(Some(TripStatus {
            id: t.id,
            state: t.state,
            deadline: from_ts(t.deadline),
            alert_at: from_ts(t.deadline + t.grace_s),
            created_at: from_ts(t.created_at),
            alerted_at: t.alerted_at.map(from_ts),
            closed_at: t.closed_at.map(from_ts),
            last_position: t.last,
            notifications,
        }))
    }

    /// The trip, for its owner.
    pub fn get(&self, id: &str, token: &str) -> Result<Option<TripStatus>> {
        match self.authorized(id, token)? {
            Some(_) => self.status(id),
            None => Ok(None),
        }
    }

    /// The skier is back. After an alert, the contacts who received it are told.
    pub fn check_in(
        &self,
        id: &str,
        token: &str,
    ) -> Result<std::result::Result<TripStatus, Refusal>> {
        let Some(t) = self.authorized(id, token)? else {
            return Ok(Err(Refusal::NotFound));
        };
        let now = self.clock.now();
        match t.state.as_str() {
            "active" => {
                self.db().execute(
                    "UPDATE trips SET state = 'returned', closed_at = ?2 WHERE id = ?1",
                    params![id, ts(now)],
                )?;
            }
            "alerted" => {
                let mut db = self.db();
                let tx = db.transaction()?;
                tx.execute(
                    "UPDATE trips SET state = 'returned', closed_at = ?2 WHERE id = ?1",
                    params![id, ts(now)],
                )?;
                // Alerts not delivered yet: no longer needed.
                tx.execute(
                    "UPDATE outbox SET cancelled_at = ?2 WHERE trip_id = ?1 AND kind = 'alert' AND sent_at IS NULL AND failed_at IS NULL",
                    params![id, ts(now)],
                )?;
                let told: Vec<String> = {
                    let mut st = tx.prepare(
                        "SELECT recipient FROM outbox WHERE trip_id = ?1 AND kind = 'alert' AND sent_at IS NOT NULL",
                    )?;
                    st.query_map([id], |r| r.get(0))?
                        .collect::<rusqlite::Result<_>>()?
                };
                let (subject, body) = self.returned_message(&t, now);
                for to in told {
                    tx.execute(
                        "INSERT INTO outbox (trip_id, kind, recipient, subject, body, created_at, next_attempt_at) VALUES (?1, 'returned', ?2, ?3, ?4, ?5, ?5)",
                        params![id, to, subject, body, ts(now)],
                    )?;
                }
                tx.commit()?;
            }
            s => return Ok(Err(Refusal::Conflict(format!("trip is {s}")))),
        }
        Ok(Ok(self.status(id)?.expect("trip exists")))
    }

    /// Moves the deadline (before the alert only).
    pub fn extend(
        &self,
        id: &str,
        token: &str,
        deadline: DateTime<Utc>,
    ) -> Result<std::result::Result<TripStatus, Refusal>> {
        let Some(t) = self.authorized(id, token)? else {
            return Ok(Err(Refusal::NotFound));
        };
        let now = self.clock.now();
        if t.state != "active" {
            return Ok(Err(Refusal::Conflict(format!(
                "trip is {}: confirm the return instead",
                t.state
            ))));
        }
        if deadline <= now || deadline > from_ts(t.created_at) + Duration::days(limits::MAX_DAYS) {
            return Ok(Err(Refusal::Invalid(
                "deadline: after now, at most 7 days after the plan".into(),
            )));
        }
        self.db().execute(
            "UPDATE trips SET deadline = ?2 WHERE id = ?1",
            params![id, ts(deadline)],
        )?;
        Ok(Ok(self.status(id)?.expect("trip exists")))
    }

    /// Records a position (when the phone has some network on the way).
    pub fn position(
        &self,
        id: &str,
        token: &str,
        p: Position,
    ) -> Result<std::result::Result<TripStatus, Refusal>> {
        let Some(t) = self.authorized(id, token)? else {
            return Ok(Err(Refusal::NotFound));
        };
        if !valid_position(&p) {
            return Ok(Err(Refusal::Invalid("bad coordinates".into())));
        }
        if t.state != "active" && t.state != "alerted" {
            return Ok(Err(Refusal::Conflict(format!("trip is {}", t.state))));
        }
        let now = self.clock.now();
        let at = p.at.unwrap_or(now).min(now);
        self.db().execute(
            "UPDATE trips SET last_lon = ?2, last_lat = ?3, last_at = ?4, last_accuracy = ?5 WHERE id = ?1",
            params![id, p.lon, p.lat, ts(at), p.accuracy_m],
        )?;
        Ok(Ok(self.status(id)?.expect("trip exists")))
    }

    /// Cancels a trip before its alert.
    pub fn cancel(
        &self,
        id: &str,
        token: &str,
    ) -> Result<std::result::Result<TripStatus, Refusal>> {
        let Some(t) = self.authorized(id, token)? else {
            return Ok(Err(Refusal::NotFound));
        };
        if t.state != "active" {
            return Ok(Err(Refusal::Conflict(format!(
                "trip is {}: confirm the return instead",
                t.state
            ))));
        }
        self.db().execute(
            "UPDATE trips SET state = 'cancelled', closed_at = ?2 WHERE id = ?1",
            params![id, ts(self.clock.now())],
        )?;
        Ok(Ok(self.status(id)?.expect("trip exists")))
    }

    fn alert_message(&self, t: &TripRow) -> (String, String) {
        let name = &t.skier_name;
        let deadline = paris_time(from_ts(t.deadline));
        let mut b = format!(
            "Bonjour,\n\n{name} vous a désigné(e) comme contact de sécurité dans l'application skimap avant de partir à ski.\n\
             Le retour devait être confirmé avant le {deadline}. Il ne l'a pas été, et le délai de {} minutes après cette heure est dépassé.\n\n\
             Ce n'est peut-être qu'un oubli ou un téléphone déchargé. Essayez d'abord de joindre {name}",
            t.grace_s / 60
        );
        if let Some(p) = &t.skier_phone {
            b.push_str(&format!(" au {p}"));
        }
        b.push_str(
            ".\nSans nouvelles, appelez le 112 (secours en montagne) et donnez-leur les informations ci-dessous.\n\n",
        );
        if !t.plan.is_empty() {
            b.push_str(&format!("Itinéraire et plan annoncés :\n{}\n\n", t.plan));
        }
        if let Some(site) = t
            .site
            .as_deref()
            .and_then(|s| skimap_common::site::site(s).ok())
        {
            b.push_str(&format!("Secteur : {} ({})\n", site.name, site.department));
        }
        if let Some([lon, lat]) = t.route.first() {
            b.push_str(&format!("Départ prévu : {}\n", fmt_point(*lat, *lon)));
        }
        if t.route.len() > 1 {
            let [lon, lat] = t.route[t.route.len() - 1];
            b.push_str(&format!("Arrivée prévue : {}\n", fmt_point(lat, lon)));
            let n = t.route.len();
            let step = n.div_ceil(15).max(1);
            let pts: Vec<String> = t
                .route
                .iter()
                .step_by(step)
                .map(|[lon, lat]| format!("{lat:.5},{lon:.5}"))
                .collect();
            b.push_str(&format!(
                "Points de l'itinéraire (latitude,longitude) : {}\n",
                pts.join(" ; ")
            ));
        }
        match &t.last {
            Some(p) => {
                b.push_str(&format!(
                    "Dernière position reçue : {}, le {}",
                    fmt_point(p.lat, p.lon),
                    paris_time(p.at.unwrap_or_else(|| from_ts(t.created_at)))
                ));
                if let Some(a) = p.accuracy_m {
                    b.push_str(&format!(", précision {a:.0} m"));
                }
                b.push('\n');
            }
            None => b.push_str("Aucune position n'a été reçue depuis le départ.\n"),
        }
        b.push_str(&format!(
            "Plan déposé le {}.\n\n\
             Vous recevrez un autre message si {name} confirme son retour.\n\n\
             Message automatique du serveur skimap ({}), qui efface ces informations 7 jours après la sortie. Ne répondez pas à ce message.\n",
            paris_time(from_ts(t.created_at)),
            self.public_url
        ));
        (
            format!("Alerte skimap : {name} n'a pas confirmé son retour"),
            b,
        )
    }

    fn returned_message(&self, t: &TripRow, now: DateTime<Utc>) -> (String, String) {
        let name = &t.skier_name;
        (
            format!("skimap : {name} est rentré(e), alerte levée"),
            format!(
                "Bonjour,\n\n{name} a confirmé son retour le {}.\nL'alerte envoyée le {} est levée : rien d'autre à faire.\n\n\
                 Message automatique du serveur skimap ({}). Ne répondez pas à ce message.\n",
                paris_time(now),
                t.alerted_at
                    .map_or_else(|| "?".to_owned(), |a| paris_time(from_ts(a))),
                self.public_url
            ),
        )
    }

    /// One turn of the timer at the clock's time: alerts the trips past their deadline
    /// plus grace, delivers due messages (retrying failures with a growing delay), deletes
    /// trips closed for more than [`limits::KEEP_DAYS`] days.
    pub fn tick(&self) -> Result<TickReport> {
        let now = self.clock.now();
        let mut rep = TickReport::default();
        self.last_tick
            .store(ts(now), std::sync::atomic::Ordering::Relaxed);

        // 1. Deadlines.
        let due: Vec<TripRow> = {
            let db = self.db();
            let mut st = db.prepare(&format!(
                "SELECT {TRIP_COLUMNS} FROM trips WHERE state = 'active' AND deadline + grace_s <= ?1"
            ))?;
            st.query_map([ts(now)], trip_row)?
                .collect::<rusqlite::Result<_>>()?
        };
        for t in due {
            let (subject, body) = self.alert_message(&t);
            let mut db = self.db();
            let tx = db.transaction()?;
            let changed = tx.execute(
                "UPDATE trips SET state = 'alerted', alerted_at = ?2 WHERE id = ?1 AND state = 'active'",
                params![t.id, ts(now)],
            )?;
            if changed == 1 {
                for to in &t.contacts {
                    tx.execute(
                        "INSERT INTO outbox (trip_id, kind, recipient, subject, body, created_at, next_attempt_at) VALUES (?1, 'alert', ?2, ?3, ?4, ?5, ?5)",
                        params![t.id, to, subject, body, ts(now)],
                    )?;
                }
                rep.alerted += 1;
                tracing::warn!(trip = %t.id, contacts = t.contacts.len(), "non-return alert");
            }
            tx.commit()?;
        }

        // 2. Deliveries (outside the lock: the relay can take seconds).
        let sent_24h: i64 = self.db().query_row(
            "SELECT COUNT(*) FROM outbox WHERE sent_at > ?1",
            [ts(now - Duration::hours(24))],
            |r| r.get(0),
        )?;
        let mut budget = i64::from(self.daily_cap) - sent_24h;
        let pending: Vec<(i64, String, String, String, i64, u32)> = {
            let db = self.db();
            let mut st = db.prepare(
                "SELECT id, recipient, subject, body, created_at, attempts FROM outbox \
                 WHERE sent_at IS NULL AND failed_at IS NULL AND cancelled_at IS NULL AND next_attempt_at <= ?1 \
                 ORDER BY kind = 'alert' DESC, id",
            )?;
            st.query_map([ts(now)], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            })?
            .collect::<rusqlite::Result<_>>()?
        };
        for (id, to, subject, body, created, attempts) in pending {
            if budget <= 0 {
                rep.capped += 1;
                continue;
            }
            let mail = Email { to, subject, body };
            match self.mailer.send(&mail) {
                Ok(()) => {
                    budget -= 1;
                    rep.sent += 1;
                    self.db().execute(
                        "UPDATE outbox SET sent_at = ?2, attempts = attempts + 1, last_error = NULL WHERE id = ?1",
                        params![id, ts(now)],
                    )?;
                }
                Err(e) => {
                    let n = attempts + 1;
                    let give_up = now - from_ts(created) >= Duration::hours(limits::RETRY_HOURS);
                    // 1, 2, 4, 8, 16, then every 30 minutes.
                    let wait = Duration::minutes(match n {
                        0 | 1 => 1,
                        2 => 2,
                        3 => 4,
                        4 => 8,
                        5 => 16,
                        _ => 30,
                    });
                    let msg = format!("{e:#}");
                    tracing::warn!(outbox = id, attempts = n, error = %msg, give_up, "delivery failed");
                    self.db().execute(
                        "UPDATE outbox SET attempts = ?2, last_error = ?3, next_attempt_at = ?4, failed_at = ?5 WHERE id = ?1",
                        params![
                            id,
                            n,
                            msg,
                            ts(now + wait),
                            give_up.then(|| ts(now))
                        ],
                    )?;
                    if give_up {
                        rep.failed += 1;
                    } else {
                        rep.retried += 1;
                    }
                }
            }
        }

        // 3. Retention.
        let cutoff = ts(now - Duration::days(limits::KEEP_DAYS));
        rep.purged = self.db().execute(
            "DELETE FROM trips WHERE (closed_at IS NOT NULL AND closed_at < ?1) \
             OR (state = 'alerted' AND deadline < ?1 AND NOT EXISTS \
                 (SELECT 1 FROM outbox o WHERE o.trip_id = trips.id AND o.sent_at IS NULL AND o.failed_at IS NULL AND o.cancelled_at IS NULL))",
            [cutoff],
        )? as u32;
        Ok(rep)
    }
}
