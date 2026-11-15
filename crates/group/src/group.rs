//! The group created at the chalet: a key of 32 bytes, the members (one number each) and
//! the fingerprint of the common plan, shared as a QR code that the others scan. No
//! account, no server, no durable identifier.
//!
//! QR payload: `SKIMAP:G1:` followed by the binary group in base32 (RFC 4648 alphabet,
//! no padding), so that the whole text stays in the alphanumeric mode of QR codes:
//!
//! | bytes | field |
//! |---|---|
//! | 1 | version (1) |
//! | 32 | group key |
//! | 8 | fingerprint of the common plan (zeros: none) |
//! | 4 | creation, Unix seconds, big endian |
//! | 1 | number of members n (1 to [`MAX_MEMBERS`]) |
//! | n × (1 + len) | name of each member: length then UTF-8 (at most [`MAX_NAME`] bytes) |

use sha2::{Digest, Sha256};

use crate::Error;

/// Members of a group at most: the member number takes 4 bits of the frame.
pub const MAX_MEMBERS: usize = 16;

/// Length of a member's name at most, bytes of UTF-8.
pub const MAX_NAME: usize = 24;

/// Prefix of the QR text.
pub const QR_PREFIX: &str = "SKIMAP:G1:";

const VERSION: u8 = 1;
const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

/// A group: its key, its members (the index is the member's number), the common plan.
#[derive(Clone, PartialEq, Eq)]
pub struct Group {
    /// Secret of the group: whoever has it reads the positions.
    pub key: [u8; 32],
    /// Names of the members; the index is the number carried by the frames.
    pub members: Vec<String>,
    /// Fingerprint of the common plan ([`plan_fingerprint`]); zeros: no common plan.
    pub plan: [u8; 8],
    /// Creation, Unix seconds.
    pub created_s: u32,
}

impl std::fmt::Debug for Group {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never the key in a log.
        f.debug_struct("Group")
            .field("members", &self.members)
            .field("plan", &hex(&self.plan))
            .field("created_s", &self.created_s)
            .finish_non_exhaustive()
    }
}

/// Fingerprint of a plan: the first 8 bytes of the SHA-256 of its file (`plan.json`).
pub fn plan_fingerprint(plan_json: &[u8]) -> [u8; 8] {
    let d = Sha256::digest(plan_json);
    let mut out = [0u8; 8];
    out.copy_from_slice(&d[..8]);
    out
}

/// Lower-case hexadecimal.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

impl Group {
    /// A new group. `key` comes from the platform's secure random generator.
    pub fn create(
        key: [u8; 32],
        members: Vec<String>,
        plan: [u8; 8],
        created_s: u32,
    ) -> Result<Self, Error> {
        let members: Vec<String> = members.into_iter().map(|m| m.trim().to_string()).collect();
        if members.is_empty() || members.len() > MAX_MEMBERS {
            return Err(Error::Members(members.len()));
        }
        for m in &members {
            if m.is_empty() || m.len() > MAX_NAME {
                return Err(Error::Name(m.clone()));
            }
        }
        if key == [0u8; 32] {
            return Err(Error::Key);
        }
        Ok(Self {
            key,
            members,
            plan,
            created_s,
        })
    }

    /// Whether the group has a common plan.
    pub fn has_plan(&self) -> bool {
        self.plan != [0u8; 8]
    }

    /// Short identifier shown to the members to check they scanned the same group (4 hex
    /// digits of a hash of the key; reveals nothing usable of the key).
    pub fn short_id(&self) -> String {
        let mut h = Sha256::new();
        h.update(b"skimap-group id v1");
        h.update(self.key);
        hex(&h.finalize()[..2])
    }

    /// The binary group (table of the module).
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(46 + self.members.len() * 8);
        out.push(VERSION);
        out.extend_from_slice(&self.key);
        out.extend_from_slice(&self.plan);
        out.extend_from_slice(&self.created_s.to_be_bytes());
        out.push(self.members.len() as u8);
        for m in &self.members {
            out.push(m.len() as u8);
            out.extend_from_slice(m.as_bytes());
        }
        out
    }

    /// Reads [`Group::to_bytes`].
    pub fn from_bytes(b: &[u8]) -> Result<Self, Error> {
        let bad = || Error::Qr("truncated group".into());
        if b.first() != Some(&VERSION) {
            return Err(Error::Qr(format!("version {:?}", b.first())));
        }
        if b.len() < 46 {
            return Err(bad());
        }
        let mut key = [0u8; 32];
        key.copy_from_slice(&b[1..33]);
        let mut plan = [0u8; 8];
        plan.copy_from_slice(&b[33..41]);
        let created_s = u32::from_be_bytes([b[41], b[42], b[43], b[44]]);
        let n = b[45] as usize;
        let mut at = 46;
        let mut members = Vec::with_capacity(n);
        for _ in 0..n {
            let len = *b.get(at).ok_or_else(bad)? as usize;
            let name = b.get(at + 1..at + 1 + len).ok_or_else(bad)?;
            members.push(
                String::from_utf8(name.to_vec()).map_err(|_| Error::Qr("name not UTF-8".into()))?,
            );
            at += 1 + len;
        }
        if at != b.len() {
            return Err(Error::Qr("trailing bytes".into()));
        }
        Self::create(key, members, plan, created_s)
    }

    /// Text of the QR code: [`QR_PREFIX`] and the binary group in base32.
    pub fn to_qr_text(&self) -> String {
        format!("{QR_PREFIX}{}", base32(&self.to_bytes()))
    }

    /// Text of the QR code a member scans to join: the group and its number (`/n`), so that
    /// scanning is all there is to do.
    pub fn to_member_qr_text(&self, member: u8) -> String {
        format!("{}/{member}", self.to_qr_text())
    }

    /// Reads a scanned QR text (spaces and case ignored around the payload); a member's
    /// number after the group ([`Group::to_member_qr_text`]) is ignored.
    pub fn from_qr_text(text: &str) -> Result<Self, Error> {
        Self::read_qr(text).map(|(g, _)| g)
    }

    /// Reads a scanned QR text: the group, and the member it was shown for (if any).
    pub fn read_qr(text: &str) -> Result<(Self, Option<u8>), Error> {
        let t = text.trim();
        let body = t
            .strip_prefix(QR_PREFIX)
            .ok_or_else(|| Error::Qr("not a skimap group".into()))?;
        let (body, member) = match body.split_once('/') {
            Some((b, m)) => (
                b,
                Some(
                    m.parse::<u8>()
                        .map_err(|_| Error::Qr(format!("member {m:?}")))?,
                ),
            ),
            None => (body, None),
        };
        let g = Self::from_bytes(&unbase32(body)?)?;
        if let Some(m) = member
            && usize::from(m) >= g.members.len()
        {
            return Err(Error::Member(m));
        }
        Ok((g, member))
    }

    /// The QR code of [`Group::to_qr_text`] (or of [`Group::to_member_qr_text`] for
    /// `member`): its width and its modules, row by row (`true`: dark), error correction M.
    pub fn qr_modules(&self, member: Option<u8>) -> Result<(u32, Vec<bool>), Error> {
        let text = match member {
            Some(m) => self.to_member_qr_text(m),
            None => self.to_qr_text(),
        };
        let code = qrcode::QrCode::with_error_correction_level(text.as_bytes(), qrcode::EcLevel::M)
            .map_err(|e| Error::Qr(e.to_string()))?;
        let w = code.width() as u32;
        let dark = code
            .to_colors()
            .into_iter()
            .map(|c| c == qrcode::Color::Dark)
            .collect();
        Ok((w, dark))
    }
}

fn base32(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 8 / 5 + 1);
    let mut acc: u32 = 0;
    let mut bits = 0;
    for &b in bytes {
        acc = (acc << 8) | u32::from(b);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(ALPHABET[((acc >> bits) & 31) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(ALPHABET[((acc << (5 - bits)) & 31) as usize] as char);
    }
    out
}

fn unbase32(text: &str) -> Result<Vec<u8>, Error> {
    let mut out = Vec::with_capacity(text.len() * 5 / 8);
    let mut acc: u32 = 0;
    let mut bits = 0;
    for c in text.trim().bytes() {
        let c = c.to_ascii_uppercase();
        let v = ALPHABET
            .iter()
            .position(|&a| a == c)
            .ok_or_else(|| Error::Qr(format!("character {:?}", c as char)))?;
        acc = (acc << 5) | v as u32;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn group() -> Group {
        Group::create(
            [7u8; 32],
            vec!["Pierre".into(), "Léa".into(), "Jo".into()],
            plan_fingerprint(b"{\"plan\":1}"),
            1_768_000_000,
        )
        .unwrap()
    }

    #[test]
    fn qr_text_round_trips_and_stays_alphanumeric() {
        let g = group();
        let t = g.to_qr_text();
        assert!(t.starts_with(QR_PREFIX));
        // Alphanumeric mode of QR: digits, upper case, space $ % * + - . / :
        assert!(
            t.bytes()
                .all(|c| c.is_ascii_digit() || c.is_ascii_uppercase() || b" $%*+-./:".contains(&c))
        );
        assert_eq!(Group::from_qr_text(&t).unwrap(), g);
        assert_eq!(
            Group::from_qr_text(&t.to_lowercase().replace("skimap:g1:", QR_PREFIX)).unwrap(),
            g
        );
    }

    #[test]
    fn member_qr_texts_carry_the_member() {
        let g = group();
        let t = g.to_member_qr_text(2);
        assert_eq!(Group::read_qr(&t).unwrap(), (g.clone(), Some(2)));
        assert_eq!(Group::read_qr(&g.to_qr_text()).unwrap(), (g.clone(), None));
        assert_eq!(Group::from_qr_text(&t).unwrap(), g);
        assert!(matches!(
            Group::read_qr(&g.to_member_qr_text(3)),
            Err(Error::Member(3))
        ));
        assert!(Group::read_qr(&format!("{}/x", g.to_qr_text())).is_err());
        let (w1, m1) = g.qr_modules(Some(1)).unwrap();
        let (_, m2) = g.qr_modules(Some(2)).unwrap();
        assert!(w1 <= 41);
        assert_ne!(m1, m2);
    }

    #[test]
    fn qr_modules_are_square_and_small() {
        let (w, m) = group().qr_modules(None).unwrap();
        assert_eq!(m.len(), (w * w) as usize);
        // 3 members: version ≤ 6 (41 modules), readable on a phone screen at arm's length.
        assert!(w <= 41, "width {w}");
        // 16 members with long names still fit.
        let names = (0..16)
            .map(|i| format!("Membre numéro {i:02} long"))
            .collect();
        let big = Group::create([9u8; 32], names, [1u8; 8], 1).unwrap();
        let (w, _) = big.qr_modules(Some(15)).unwrap();
        assert!(w <= 101, "width {w}");
    }

    #[test]
    fn invalid_groups_are_refused() {
        assert!(matches!(
            Group::create([1; 32], vec![], [0; 8], 0),
            Err(Error::Members(0))
        ));
        let many = (0..17).map(|i| i.to_string()).collect();
        assert!(matches!(
            Group::create([1; 32], many, [0; 8], 0),
            Err(Error::Members(17))
        ));
        assert!(matches!(
            Group::create([1; 32], vec![" ".into()], [0; 8], 0),
            Err(Error::Name(_))
        ));
        assert!(matches!(
            Group::create([0; 32], vec!["a".into()], [0; 8], 0),
            Err(Error::Key)
        ));
        assert!(Group::from_qr_text("https://example.org").is_err());
        assert!(Group::from_qr_text("SKIMAP:G1:AAAA").is_err());
        let t = group().to_qr_text();
        assert!(Group::from_qr_text(&t[..t.len() - 3]).is_err());
        assert!(Group::from_qr_text(&format!("{t}1")).is_err());
    }

    #[test]
    fn debug_never_shows_the_key() {
        let s = format!("{:?}", group());
        assert!(!s.contains("key"));
        assert!(!s.contains("[7, 7"));
    }

    #[test]
    fn short_id_and_fingerprint_are_stable() {
        assert_eq!(group().short_id().len(), 4);
        assert_eq!(group().short_id(), group().short_id());
        assert_ne!(plan_fingerprint(b"a"), plan_fingerprint(b"b"));
        assert!(group().has_plan());
    }

    proptest! {
        #[test]
        fn base32_round_trips(bytes in proptest::collection::vec(any::<u8>(), 0..200)) {
            prop_assert_eq!(unbase32(&base32(&bytes)).unwrap(), bytes);
        }

        #[test]
        fn groups_round_trip(
            key in proptest::array::uniform32(1u8..),
            names in proptest::collection::vec("[a-zA-Zéè ]{1,8}[a-z]", 1..=MAX_MEMBERS),
            plan in proptest::array::uniform8(any::<u8>()),
            created in any::<u32>(),
        ) {
            let g = Group::create(key, names, plan, created).unwrap();
            prop_assert_eq!(Group::from_qr_text(&g.to_qr_text()).unwrap(), g);
        }
    }
}
