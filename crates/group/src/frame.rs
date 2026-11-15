//! The frame: the Service Data of a legacy non-connectable BLE advertisement
//! (ADV_NONCONN_IND, 16-bit UUID [`SERVICE_UUID`]), [`FRAME_LEN`] bytes, so that it fits in
//! the 31 bytes of a legacy advertisement with the flags (3) and the Service Data header (4).
//!
//! | bytes | field | |
//! |---|---|---|
//! | 0 | version ([`VERSION`]) | clear |
//! | 1..5 | rotating tag: HMAC(k_tag, epoch)[..4], epoch of [`EPOCH_S`] s | clear (scan filter) |
//! | 5 | member (high 4 bits), hops (low 4 bits) | clear |
//! | 6..8 | time of the fix, Unix seconds mod 65536, big endian | clear |
//! | 8..17 | latitude (24 bits), longitude (24 bits), flags (8), progress along the plan (16, ×4 m) | AES-256-CTR |
//! | 17..21 | MAC: HMAC(k_mac, bytes 0..17 ‖ full time)[..4] | |
//!
//! The counter block of AES-CTR is tag ‖ member-and-hops ‖ full time (u32) ‖ zeros: a
//! fresh nonce for every (epoch, member, hops, second). The tag is the epoch of the
//! *transmission* (the receiver filters on it); the time is that of the *fix* (a relayed
//! position keeps its own). Keys: HMAC(group key, label) for the tag, the cipher and the
//! MAC. Anti-replay is in the table ([`crate::session`]): only a newer fix of a member is
//! kept, and only the tags of the epochs around now are accepted.

use aes::cipher::{KeyIvInit, StreamCipher};
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

use crate::Error;

/// 16-bit UUID of the Service Data. Provisional: 16-bit UUIDs are allotted by the
/// Bluetooth SIG to its members and the project has none; this value has not been checked
/// against the SIG's list (to do before a release: a SIG allotment, or a value checked
/// unassigned). Only the phones of a group read these frames (the tag filters them).
pub const SERVICE_UUID: u16 = 0xFD5C;

/// Version of the frame.
pub const VERSION: u8 = 1;

/// Length of the frame, bytes.
pub const FRAME_LEN: usize = 21;

/// Clear prefix the scan filters on: version and tag.
pub const PREFIX_LEN: usize = 5;

/// Epoch of the rotating tag, seconds (15 min, within the 10 to 20 min of GAEN).
pub const EPOCH_S: i64 = 900;

/// Hops of a relayed position at most (4 bits).
pub const MAX_HOPS: u8 = 15;

/// Progress unknown (no common plan, or not on it).
pub const PROGRESS_UNKNOWN: u16 = u16::MAX;

/// Metres per unit of progress.
pub const PROGRESS_STEP_M: f64 = 4.0;

const LAT_STEPS: f64 = 16_777_215.0;

/// Flags of the frame, decided by the phone of the member concerned.
pub mod flags {
    /// Stopped for long away from a refuge.
    pub const STOPPED: u8 = 0x01;
    /// Off the route followed (the tracker's 40 m during 15 s).
    pub const OFF_ROUTE: u8 = 0x02;
    /// Battery low.
    pub const BATTERY_LOW: u8 = 0x04;
}

type Aes256Ctr = ctr::Ctr128BE<aes::Aes256>;
type HmacSha256 = Hmac<Sha256>;

/// What a frame says of one member.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Position {
    /// Member number.
    pub member: u8,
    /// Hops: 0 from the member itself, n relayed n times.
    pub hops: u8,
    /// Time of the fix, Unix seconds.
    pub t_s: i64,
    /// Degrees (quantised to ≈ 1.2 m).
    pub lat: f64,
    /// Degrees (quantised to ≈ 2.4 m at the equator, 1.6 m in the Alps).
    pub lon: f64,
    /// [`flags`].
    pub flags: u8,
    /// Progress along the common plan, metres (quantised to 4 m); `None`: unknown.
    pub progress_m: Option<f64>,
}

/// The keys of a group, derived from its key.
#[derive(Clone)]
pub struct Keys {
    tag: [u8; 32],
    enc: [u8; 32],
    mac: [u8; 32],
}

fn hmac(key: &[u8], parts: &[&[u8]]) -> [u8; 32] {
    let mut m = <HmacSha256 as KeyInit>::new_from_slice(key).expect("HMAC takes any key length");
    for p in parts {
        m.update(p);
    }
    m.finalize().into_bytes().into()
}

/// Epoch of a time, Unix seconds.
pub fn epoch(t_s: i64) -> u32 {
    t_s.div_euclid(EPOCH_S) as u32
}

impl Keys {
    /// The keys of the group key `key`.
    pub fn derive(key: &[u8; 32]) -> Self {
        Self {
            tag: hmac(key, &[b"skimap-group tag v1"]),
            enc: hmac(key, &[b"skimap-group enc v1"]),
            mac: hmac(key, &[b"skimap-group mac v1"]),
        }
    }

    /// Tag of an epoch.
    pub fn tag(&self, epoch: u32) -> [u8; 4] {
        let h = hmac(&self.tag, &[&epoch.to_be_bytes()]);
        [h[0], h[1], h[2], h[3]]
    }

    /// Clear prefix of the frames sent during `epoch` (what the scan filters on).
    pub fn prefix(&self, epoch: u32) -> [u8; PREFIX_LEN] {
        let t = self.tag(epoch);
        [VERSION, t[0], t[1], t[2], t[3]]
    }

    fn cipher(&self, tag: &[u8], mh: u8, t_s: i64) -> Aes256Ctr {
        let mut iv = [0u8; 16];
        iv[..4].copy_from_slice(tag);
        iv[4] = mh;
        iv[5..9].copy_from_slice(&(t_s as u32).to_be_bytes());
        Aes256Ctr::new_from_slices(&self.enc, &iv).expect("32-byte key, 16-byte IV")
    }

    fn mac(&self, head: &[u8], t_s: i64) -> [u8; 4] {
        let h = hmac(&self.mac, &[head, &(t_s as u32).to_be_bytes()]);
        [h[0], h[1], h[2], h[3]]
    }

    /// The frame of `p`, sent at `now_s` (the tag of its epoch).
    pub fn seal(&self, p: &Position, now_s: i64) -> [u8; FRAME_LEN] {
        let mut f = [0u8; FRAME_LEN];
        f[..PREFIX_LEN].copy_from_slice(&self.prefix(epoch(now_s)));
        let mh = (p.member.min(15) << 4) | p.hops.min(MAX_HOPS);
        f[5] = mh;
        f[6..8].copy_from_slice(&((p.t_s as u32 & 0xFFFF) as u16).to_be_bytes());
        let lat = (((p.lat.clamp(-90.0, 90.0) + 90.0) / 180.0) * LAT_STEPS).round() as u32;
        let lon = (((p.lon.clamp(-180.0, 180.0) + 180.0) / 360.0) * LAT_STEPS).round() as u32;
        f[8..11].copy_from_slice(&lat.to_be_bytes()[1..]);
        f[11..14].copy_from_slice(&lon.to_be_bytes()[1..]);
        f[14] = p.flags;
        let prog = match p.progress_m {
            Some(m) if m.is_finite() && m >= 0.0 => {
                ((m / PROGRESS_STEP_M).round() as u32).min(u32::from(PROGRESS_UNKNOWN) - 1) as u16
            }
            _ => PROGRESS_UNKNOWN,
        };
        f[15..17].copy_from_slice(&prog.to_be_bytes());
        let tag = [f[1], f[2], f[3], f[4]];
        self.cipher(&tag, mh, p.t_s).apply_keystream(&mut f[8..17]);
        let mac = self.mac(&f[..17], p.t_s);
        f[17..].copy_from_slice(&mac);
        f
    }

    /// Reads a frame received at `now_s`: its tag must be that of the epoch of now, the one
    /// before or the one after (clock skew), its MAC right; the time of the fix is the one
    /// nearest to now with the 16 bits sent (positions older than 18 h do not exist here).
    pub fn open(&self, frame: &[u8], now_s: i64) -> Result<Position, Error> {
        if frame.len() != FRAME_LEN {
            return Err(Error::Frame("length"));
        }
        if frame[0] != VERSION {
            return Err(Error::Frame("version"));
        }
        let e = epoch(now_s);
        let tag = [frame[1], frame[2], frame[3], frame[4]];
        if ![e.wrapping_sub(1), e, e.wrapping_add(1)]
            .iter()
            .any(|&x| self.tag(x) == tag)
        {
            return Err(Error::Frame("tag"));
        }
        let low = i64::from(u16::from_be_bytes([frame[6], frame[7]]));
        // Fix time ≡ low (mod 65536), at most 32768 s after now (half the range each side).
        let mut t_s = now_s - (now_s - low).rem_euclid(65_536);
        if now_s - t_s > 32_768 {
            t_s += 65_536;
        }
        let mac = self.mac(&frame[..17], t_s);
        // Constant-time comparison of the 4 bytes.
        if mac
            .iter()
            .zip(&frame[17..])
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            != 0
        {
            return Err(Error::Frame("mac"));
        }
        let mh = frame[5];
        let mut body = [0u8; 9];
        body.copy_from_slice(&frame[8..17]);
        self.cipher(&tag, mh, t_s).apply_keystream(&mut body);
        let lat = u32::from_be_bytes([0, body[0], body[1], body[2]]);
        let lon = u32::from_be_bytes([0, body[3], body[4], body[5]]);
        let prog = u16::from_be_bytes([body[7], body[8]]);
        Ok(Position {
            member: mh >> 4,
            hops: mh & 0x0F,
            t_s,
            lat: f64::from(lat) / LAT_STEPS * 180.0 - 90.0,
            lon: f64::from(lon) / LAT_STEPS * 360.0 - 180.0,
            flags: body[6],
            progress_m: (prog != PROGRESS_UNKNOWN).then(|| f64::from(prog) * PROGRESS_STEP_M),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    const NOW: i64 = 1_768_000_000;

    fn pos() -> Position {
        Position {
            member: 3,
            hops: 0,
            t_s: NOW - 4,
            lat: 44.8712,
            lon: 6.7345,
            flags: flags::STOPPED,
            progress_m: Some(4_200.0),
        }
    }

    #[test]
    fn frame_fits_a_legacy_advertisement() {
        // Flags AD (3) + Service Data AD header (length, type, UUID: 4) + frame ≤ 31.
        const { assert!(3 + 4 + FRAME_LEN <= 31) };
    }

    #[test]
    fn seal_then_open() {
        let k = Keys::derive(&[5u8; 32]);
        let f = k.seal(&pos(), NOW);
        let p = k.open(&f, NOW + 3).unwrap();
        assert_eq!(
            (p.member, p.hops, p.t_s, p.flags),
            (3, 0, NOW - 4, flags::STOPPED)
        );
        assert!((p.lat - 44.8712).abs() < 1.2e-5 && (p.lon - 6.7345).abs() < 2.2e-5);
        assert_eq!(p.progress_m, Some(4_200.0));
    }

    #[test]
    fn fields_are_encrypted() {
        let k = Keys::derive(&[5u8; 32]);
        let a = k.seal(&pos(), NOW);
        let mut moved = pos();
        moved.lat += 0.001;
        let b = k.seal(&moved, NOW);
        // Same header, different ciphertext; the clear latitude bytes are not in the frame.
        assert_eq!(a[..8], b[..8]);
        assert_ne!(a[8..17], b[8..17]);
        let clear = (((44.8712f64 + 90.0) / 180.0) * LAT_STEPS).round() as u32;
        assert_ne!(a[8..11], clear.to_be_bytes()[1..]);
    }

    #[test]
    fn another_group_reads_nothing() {
        let f = Keys::derive(&[5u8; 32]).seal(&pos(), NOW);
        assert!(matches!(
            Keys::derive(&[6u8; 32]).open(&f, NOW),
            Err(Error::Frame("tag"))
        ));
    }

    #[test]
    fn tag_rotates_every_epoch_and_old_tags_are_refused() {
        let k = Keys::derive(&[5u8; 32]);
        let e = epoch(NOW);
        assert_ne!(k.tag(e), k.tag(e + 1));
        let f = k.seal(&pos(), NOW);
        assert!(k.open(&f, NOW + EPOCH_S).is_ok());
        // Two epochs later (30 min): the tag is no longer accepted (replay of an old frame).
        assert!(matches!(
            k.open(&f, NOW + 3 * EPOCH_S),
            Err(Error::Frame("tag"))
        ));
    }

    #[test]
    fn time_is_rebuilt_across_the_wrap() {
        let k = Keys::derive(&[5u8; 32]);
        // Fix just before a multiple of 65536 s, received just after.
        let wrap = 1_768_030_208; // multiple of 65536
        let mut p = pos();
        p.t_s = wrap - 10;
        let f = k.seal(&p, wrap - 5);
        assert_eq!(k.open(&f, wrap + 20).unwrap().t_s, wrap - 10);
        // A relayed position 2 h old still has its own time.
        p.t_s = NOW - 7_200;
        let f = k.seal(&p, NOW);
        assert_eq!(k.open(&f, NOW).unwrap().t_s, NOW - 7_200);
    }

    #[test]
    fn unknown_progress() {
        let k = Keys::derive(&[5u8; 32]);
        let mut p = pos();
        p.progress_m = None;
        assert_eq!(k.open(&k.seal(&p, NOW), NOW).unwrap().progress_m, None);
        p.progress_m = Some(f64::NAN);
        assert_eq!(k.open(&k.seal(&p, NOW), NOW).unwrap().progress_m, None);
    }

    proptest! {
        #[test]
        fn round_trip(
            member in 0u8..16, hops in 0u8..16, age in 0i64..10_800,
            lat in -90.0f64..90.0, lon in -180.0f64..180.0, fl in any::<u8>(),
            prog in proptest::option::of(0.0f64..200_000.0), skew in -800i64..800,
            key in proptest::array::uniform32(any::<u8>()),
        ) {
            let k = Keys::derive(&key);
            let p = Position { member, hops, t_s: NOW - age, lat, lon, flags: fl, progress_m: prog };
            let q = k.open(&k.seal(&p, NOW), NOW + skew).unwrap();
            prop_assert_eq!((q.member, q.hops, q.t_s, q.flags), (member, hops, NOW - age, fl));
            prop_assert!((q.lat - lat).abs() <= 180.0 / LAT_STEPS);
            prop_assert!((q.lon - lon).abs() <= 360.0 / LAT_STEPS);
            match (prog, q.progress_m) {
                (Some(a), Some(b)) => prop_assert!((a - b).abs() <= PROGRESS_STEP_M / 2.0 + 1e-9),
                (None, None) => {}
                other => prop_assert!(false, "{:?}", other),
            }
        }

        /// Any change of any bit is refused (MAC or tag).
        #[test]
        fn tampering_is_refused(byte in 0usize..FRAME_LEN, bit in 0u8..8) {
            let k = Keys::derive(&[9u8; 32]);
            let mut f = k.seal(&pos(), NOW);
            f[byte] ^= 1 << bit;
            prop_assert!(k.open(&f, NOW).is_err());
        }
    }
}
