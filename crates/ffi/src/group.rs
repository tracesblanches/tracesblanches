//! Group mode without network (N2, #128): the group created and read from its QR code,
//! and one phone's group session during the outing (`skimap-group`), in coarse calls.
//!
//! The app's location service calls [`GroupSession::tick`] once per GPS batch and once per
//! batch of frames scanned: its own position in, the frames heard in; the frames to
//! advertise, the alerts, the members and the scan filters out. The radio (BLE
//! advertising, batched scan) is the app's.

use std::path::Path;
use std::sync::{Arc, Mutex};

use skimap_group::{Config, Group, Lang as GroupLang, OwnFix, Phase, Session, Source};

use crate::card::Lang;
use crate::tracking_day::PLAN_FILE;

/// Errors of the group mode (`GroupException` on the Kotlin side).
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum GroupError {
    /// Invalid group, QR text, member or plan.
    #[error("{detail}")]
    Invalid {
        /// What is wrong.
        detail: String,
    },
}

fn invalid(e: impl std::fmt::Display) -> GroupError {
    GroupError::Invalid {
        detail: e.to_string(),
    }
}

/// A group, as the screens show it.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct GroupInfo {
    /// Text of the QR code (kept by the app: it is the group).
    pub qr_text: String,
    /// Four hex digits shown to check everyone scanned the same group.
    pub short_id: String,
    /// Names of the members; the index is the member's number.
    pub members: Vec<String>,
    /// Whether the group has a common plan.
    pub has_plan: bool,
    /// Creation, Unix seconds.
    pub created_s: u32,
}

/// The QR code of a group: its width and its modules row by row (`true`: dark).
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct QrMatrix {
    /// Modules per side.
    pub width: u32,
    /// `width × width` modules.
    pub modules: Vec<bool>,
}

fn info(g: &Group) -> GroupInfo {
    GroupInfo {
        qr_text: g.to_qr_text(),
        short_id: g.short_id(),
        members: g.members.clone(),
        has_plan: g.has_plan(),
        created_s: g.created_s,
    }
}

fn plan_of(plan_dir: Option<&str>) -> Result<Option<[u8; 8]>, GroupError> {
    let Some(dir) = plan_dir else { return Ok(None) };
    let bytes = std::fs::read(Path::new(dir).join(PLAN_FILE))
        .map_err(|e| invalid(format!("{dir}/{PLAN_FILE}: {e}")))?;
    Ok(Some(skimap_group::plan_fingerprint(&bytes)))
}

/// A new group: `key` of 32 bytes from the platform's secure random generator, the
/// members' names (the creator first), the common plan (`plan_dir` of the validated day,
/// `None`: no common plan).
#[uniffi::export]
pub fn group_mode_create(
    key: Vec<u8>,
    members: Vec<String>,
    plan_dir: Option<String>,
    created_s: u32,
) -> Result<GroupInfo, GroupError> {
    let key: [u8; 32] = key
        .try_into()
        .map_err(|k: Vec<u8>| invalid(format!("key of {} bytes (32)", k.len())))?;
    let plan = plan_of(plan_dir.as_deref())?.unwrap_or([0u8; 8]);
    Group::create(key, members, plan, created_s)
        .map(|g| info(&g))
        .map_err(invalid)
}

/// The group of a scanned QR text.
#[uniffi::export]
pub fn group_mode_read(qr_text: String) -> Result<GroupInfo, GroupError> {
    Group::from_qr_text(&qr_text)
        .map(|g| info(&g))
        .map_err(invalid)
}

/// The QR code of a group's text.
#[uniffi::export]
pub fn group_mode_qr(qr_text: String) -> Result<QrMatrix, GroupError> {
    let g = Group::from_qr_text(&qr_text).map_err(invalid)?;
    let (width, modules) = g.qr_modules().map_err(invalid)?;
    Ok(QrMatrix { width, modules })
}

/// Whether the plan in `plan_dir` is the group's common plan.
#[uniffi::export]
pub fn group_mode_plan_matches(qr_text: String, plan_dir: String) -> Result<bool, GroupError> {
    let g = Group::from_qr_text(&qr_text).map_err(invalid)?;
    Ok(g.has_plan() && plan_of(Some(&plan_dir))? == Some(g.plan))
}

/// 16-bit UUID of the Service Data of the frames, and their length.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct GroupRadio {
    /// Service Data UUID (16 bits).
    pub service_uuid: u16,
    /// Bytes of a frame.
    pub frame_len: u32,
    /// Bytes of the prefix the scan filters on (version and tag).
    pub prefix_len: u32,
}

/// The radio constants of the frames.
#[uniffi::export]
pub fn group_mode_radio() -> GroupRadio {
    GroupRadio {
        service_uuid: skimap_group::SERVICE_UUID,
        frame_len: skimap_group::FRAME_LEN as u32,
        prefix_len: skimap_group::frame::PREFIX_LEN as u32,
    }
}

/// Phase of the outing, from the tracker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum GroupPhase {
    /// Moving or waiting.
    Moving,
    /// Paused, not at a refuge.
    Paused,
    /// Paused at a refuge.
    Refuge,
}

/// This phone's position, from the tracker's last batch.
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)]
pub struct GroupFix {
    /// Unix milliseconds.
    pub t_ms: i64,
    /// Degrees.
    pub lat: f64,
    /// Degrees.
    pub lon: f64,
    /// Phase of the outing.
    pub phase: GroupPhase,
    /// Off the route followed.
    pub off_route: bool,
    /// Progress along the plan followed, metres (`None`: not on the plan).
    pub progress_m: Option<f64>,
    /// Battery low.
    pub battery_low: bool,
}

/// Kind of alert.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum GroupAlertKind {
    /// Not heard for long and far behind.
    Straggler,
    /// Stopped for long away from a refuge.
    LongStop,
    /// Off the common route.
    OffRoute,
}

/// An alert about a member: said and vibrated by the location service.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct GroupAlert {
    /// Kind.
    pub kind: GroupAlertKind,
    /// Member concerned.
    pub member: u8,
    /// What to say.
    pub text: String,
    /// Age of the member's last position, seconds.
    pub age_s: i64,
    /// Distance from this phone, metres.
    pub distance_m: f64,
}

/// A member, as the map and the list show it.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct GroupMember {
    /// Number.
    pub member: u8,
    /// Name.
    pub name: String,
    /// This phone's member.
    pub me: bool,
    /// Last known position (`None`: never heard).
    pub lat: Option<f64>,
    /// Last known position.
    pub lon: Option<f64>,
    /// Age of the last position, seconds.
    pub age_s: Option<i64>,
    /// Hops of the last position: 0 heard from the member's phone, n relayed.
    pub hops: Option<u8>,
    /// Stopped for long away from a refuge.
    pub stopped: bool,
    /// Off the route.
    pub off_route: bool,
    /// Battery low.
    pub battery_low: bool,
    /// Distance from this phone, metres.
    pub distance_m: Option<f64>,
    /// Behind this phone along the plan, metres.
    pub behind_m: Option<f64>,
}

/// What one call to the session produced.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct GroupTick {
    /// This phone's frame to advertise (first advertising set).
    pub own_frame: Option<Vec<u8>>,
    /// The next relayed position to advertise (second advertising set).
    pub relay_frame: Option<Vec<u8>>,
    /// Prefixes the scan filters on (they change every 15 min).
    pub scan_prefixes: Vec<Vec<u8>>,
    /// Alerts raised.
    pub alerts: Vec<GroupAlert>,
    /// Every member, this phone's first.
    pub members: Vec<GroupMember>,
    /// Frames heard that gave a newer position.
    pub new_positions: u32,
    /// Frames refused since the start (other group, old tag, MAC).
    pub refused: u32,
}

/// One phone's group session during an outing.
#[derive(uniffi::Object)]
pub struct GroupSession {
    session: Mutex<Session>,
}

impl std::fmt::Debug for GroupSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GroupSession").finish_non_exhaustive()
    }
}

/// The session of member `me` of the group of `qr_text`, following the plan in
/// `plan_dir` (its progress is compared only if it is the group's plan).
#[uniffi::export]
pub fn open_group_session(
    qr_text: String,
    me: u8,
    plan_dir: Option<String>,
    lang: Lang,
    utc_offset_s: i64,
) -> Result<Arc<GroupSession>, GroupError> {
    let g = Group::from_qr_text(&qr_text).map_err(invalid)?;
    // A plan that cannot be read: the group still works, on distances alone.
    let plan_matches = g.has_plan() && plan_of(plan_dir.as_deref()).ok().flatten() == Some(g.plan);
    let cfg = Config {
        plan_matches,
        utc_offset_s,
        lang: match lang {
            Lang::Fr => GroupLang::Fr,
            Lang::En => GroupLang::En,
        },
        ..Config::default()
    };
    let s = Session::new(g, me, cfg).map_err(invalid)?;
    Ok(Arc::new(GroupSession {
        session: Mutex::new(s),
    }))
}

#[uniffi::export]
impl GroupSession {
    /// This phone's member.
    pub fn me(&self) -> u8 {
        self.lock().me()
    }

    /// Whether this phone's plan is the group's (progress compared along it).
    pub fn plan_matches(&self) -> bool {
        self.lock().config().plan_matches
    }

    /// One step: this phone's new position (if any) and the frames heard since the last
    /// call, at `now_ms`; the frames to advertise, the alerts, the members, the filters.
    pub fn tick(&self, now_ms: i64, fix: Option<GroupFix>, frames: Vec<Vec<u8>>) -> GroupTick {
        let mut s = self.lock();
        if let Some(f) = fix {
            s.own(OwnFix {
                t_ms: f.t_ms,
                lat: f.lat,
                lon: f.lon,
                phase: match f.phase {
                    GroupPhase::Moving => Phase::Moving,
                    GroupPhase::Paused => Phase::Paused,
                    GroupPhase::Refuge => Phase::Refuge,
                },
                off_route: f.off_route,
                progress_m: f.progress_m,
                battery_low: f.battery_low,
            });
        }
        let mut new_positions = 0;
        for fr in &frames {
            if matches!(
                s.receive(now_ms, fr),
                Ok(skimap_group::Received::New | skimap_group::Received::Shorter)
            ) {
                new_positions += 1;
            }
        }
        let alerts = s
            .alerts(now_ms)
            .into_iter()
            .map(|a| GroupAlert {
                kind: match a.kind {
                    skimap_group::AlertKind::Straggler => GroupAlertKind::Straggler,
                    skimap_group::AlertKind::LongStop => GroupAlertKind::LongStop,
                    skimap_group::AlertKind::OffRoute => GroupAlertKind::OffRoute,
                },
                member: a.member,
                text: a.text,
                age_s: a.age_s,
                distance_m: a.distance_m,
            })
            .collect();
        GroupTick {
            own_frame: s.own_frame(now_ms).map(|f| f.to_vec()),
            relay_frame: s.relay_frame(now_ms).map(|f| f.to_vec()),
            scan_prefixes: s.scan_prefixes(now_ms).iter().map(|p| p.to_vec()).collect(),
            alerts,
            members: members(&s, now_ms),
            new_positions,
            refused: s.stats().refused,
        }
    }

    /// Every member at `now_ms`, this phone's first.
    pub fn members(&self, now_ms: i64) -> Vec<GroupMember> {
        members(&self.lock(), now_ms)
    }
}

impl GroupSession {
    fn lock(&self) -> std::sync::MutexGuard<'_, Session> {
        // A panic while locked leaves a table still coherent (each update is one assignment).
        self.session.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn members(s: &Session, now_ms: i64) -> Vec<GroupMember> {
    use skimap_group::frame::flags;
    s.members(now_ms)
        .into_iter()
        .map(|m| GroupMember {
            member: m.member,
            name: m.name,
            me: m.me,
            lat: m.position.map(|p| p.0),
            lon: m.position.map(|p| p.1),
            age_s: m.age_s,
            hops: m.source.map(|s| match s {
                Source::Direct => 0,
                Source::Relayed(n) => n,
            }),
            stopped: m.flags & flags::STOPPED != 0,
            off_route: m.flags & flags::OFF_ROUTE != 0,
            battery_low: m.flags & flags::BATTERY_LOW != 0,
            distance_m: m.distance_m,
            behind_m: m.behind_m,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: i64 = 1_768_036_800_000;

    fn fix(t_ms: i64, lat: f64, progress: f64) -> Option<GroupFix> {
        Some(GroupFix {
            t_ms,
            lat,
            lon: 6.73,
            phase: GroupPhase::Moving,
            off_route: false,
            progress_m: Some(progress),
            battery_low: false,
        })
    }

    #[test]
    fn create_read_and_two_phones() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(PLAN_FILE), b"{\"plan\":\"test\"}").unwrap();
        let plan = Some(dir.path().to_string_lossy().into_owned());
        let g = group_mode_create(
            vec![9u8; 32],
            vec!["Pierre".into(), "Léa".into()],
            plan.clone(),
            1,
        )
        .unwrap();
        assert!(g.has_plan);
        assert_eq!(group_mode_read(g.qr_text.clone()).unwrap(), g);
        let qr = group_mode_qr(g.qr_text.clone()).unwrap();
        assert_eq!(qr.modules.len() as u32, qr.width * qr.width);
        assert!(group_mode_plan_matches(g.qr_text.clone(), plan.clone().unwrap()).unwrap());
        assert!(group_mode_create(vec![1u8; 31], vec!["a".into()], None, 0).is_err());
        assert!(group_mode_read("SKIMAP:G1:ZZZ".into()).is_err());

        let a = open_group_session(g.qr_text.clone(), 0, plan.clone(), Lang::Fr, 3600).unwrap();
        let b = open_group_session(g.qr_text.clone(), 1, plan, Lang::En, 3600).unwrap();
        assert!(a.plan_matches());
        let tb = b.tick(T0, fix(T0, 44.87, 1_000.0), vec![]);
        assert_eq!(
            tb.own_frame.as_ref().unwrap().len(),
            group_mode_radio().frame_len as usize
        );
        assert_eq!(tb.scan_prefixes.len(), 3);
        let ta = a.tick(
            T0,
            fix(T0, 44.87, 1_000.0),
            vec![tb.own_frame.unwrap(), vec![1, 2, 3]],
        );
        assert_eq!((ta.new_positions, ta.refused), (1, 1));
        assert_eq!(ta.members[1].name, "Léa");
        assert_eq!(ta.members[1].hops, Some(0));
        // Ten minutes later, Pierre is 600 m further and Léa not heard: one straggler alert.
        let now = T0 + 600_000;
        let ta = a.tick(now, fix(now, 44.87 + 600.0 / 111_195.0, 1_600.0), vec![]);
        assert_eq!(ta.alerts.len(), 1);
        assert_eq!(ta.alerts[0].kind, GroupAlertKind::Straggler);
        assert!(
            ta.alerts[0].text.contains("600 m derrière"),
            "{}",
            ta.alerts[0].text
        );
        assert_eq!(a.members(now)[1].age_s, Some(600));
    }

    #[test]
    fn another_plan_compares_distances_only() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(PLAN_FILE), b"one").unwrap();
        let g = group_mode_create(
            vec![9u8; 32],
            vec!["a".into(), "b".into()],
            Some(dir.path().to_string_lossy().into_owned()),
            1,
        )
        .unwrap();
        std::fs::write(dir.path().join(PLAN_FILE), b"two").unwrap();
        let s = open_group_session(
            g.qr_text,
            1,
            Some(dir.path().to_string_lossy().into_owned()),
            Lang::Fr,
            0,
        )
        .unwrap();
        assert!(!s.plan_matches());
        assert!(open_group_session("x".into(), 0, None, Lang::Fr, 0).is_err());
    }
}
