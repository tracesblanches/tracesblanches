//! Group mode without network (N2, #128; report 10): a group created at the chalet and
//! shared by QR code, the phones of its members advertising their position over BLE
//! (advertising only, batched filtered scan), relaying the last known positions of the
//! others, and alerting when a member drops behind, stops for long away from a refuge or
//! leaves the common route.
//!
//! - [`group`]: the group (key of 32 bytes, members, fingerprint of the common plan) and
//!   its QR code.
//! - [`frame`]: the frame of 21 bytes (rotating tag, AES-256-CTR, truncated HMAC).
//! - [`session`]: one phone's table of the members' last positions (age, source, hops),
//!   the frames to send in turn, anti-replay, alerts.
//!
//! No radio, no I/O, no clock here: the app advertises and scans, gives the frames and
//! the time. Not a DVA: never used to search for anyone under the snow.

pub mod frame;
pub mod group;
pub mod session;

pub use frame::{FRAME_LEN, Keys, Position, SERVICE_UUID};
pub use group::{Group, MAX_MEMBERS, plan_fingerprint};
pub use session::{
    Alert, AlertKind, Config, Lang, MemberView, OwnFix, Phase, Received, Session, Source, Stats,
};

/// Errors of the group mode.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// A group has 1 to [`MAX_MEMBERS`] members.
    #[error("{0} members (1 to 16)")]
    Members(usize),
    /// A member's name is empty or too long.
    #[error("invalid member name {0:?}")]
    Name(String),
    /// The key is all zeros (no random generator behind it).
    #[error("invalid group key")]
    Key,
    /// Not a member of the group.
    #[error("no member {0}")]
    Member(u8),
    /// Unreadable QR text.
    #[error("not a group QR code: {0}")]
    Qr(String),
    /// A frame refused: length, version, tag (other group or old epoch), MAC, future.
    #[error("frame refused: {0}")]
    Frame(&'static str),
}
