//! Disk cache of the downloader: one body file and one JSON metadata file per request.
//!
//! The key is the SHA-256 of `method \n public URL \n`, then for a request with a body
//! `body <length> <content type>\n<body>` (POST: Overpass `data=`), then `\nRange: <value>`
//! for a range request (two parts of one file are two entries; a request without body nor
//! `Range` keeps the key it always had); secrets (query parameters or
//! headers added with [`super::Request::secret_query`] / [`super::Request::secret_header`])
//! never enter the key, the metadata or the file names.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};

/// Metadata stored next to a cached body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheMeta {
    /// Public URL (without secrets).
    pub url: String,
    /// HTTP method.
    pub method: String,
    /// Status of the response that produced the body.
    pub status: u16,
    /// `ETag` response header.
    pub etag: Option<String>,
    /// `Last-Modified` response header.
    pub last_modified: Option<String>,
    /// `Content-Type` response header.
    pub content_type: Option<String>,
    /// When the body was downloaded (Unix seconds).
    pub fetched_at_unix: u64,
    /// When the body was last confirmed current by the server (200 or 304, Unix seconds).
    pub validated_at_unix: u64,
    /// Body size in bytes.
    pub size: u64,
    /// SHA-256 of the body, hex.
    pub sha256: String,
    /// `Range` request header (`bytes=a-b`) when the body is a part of the resource.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<String>,
}

/// Body of a request as it enters the cache key: bytes and `Content-Type`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct KeyBody<'a> {
    pub(crate) bytes: &'a [u8],
    pub(crate) content_type: Option<&'a str>,
}

/// Cache key of a request: SHA-256 of the method, the public URL, the body framed by its
/// length and content type (a body can hold any byte, including what follows it in the
/// key), and the `Range` header. Without a body the key is that of the first cache format
/// (existing GET and range entries stay valid).
pub(crate) fn key(
    method: &str,
    public_url: &str,
    body: Option<KeyBody<'_>>,
    range: Option<&str>,
) -> String {
    let mut h = Sha256::new();
    h.update(method.as_bytes());
    h.update(b"\n");
    h.update(public_url.as_bytes());
    h.update(b"\n");
    if let Some(b) = body {
        // A header value holds no line break: the content type ends at the first one.
        h.update(
            format!(
                "body {} {}\n",
                b.bytes.len(),
                b.content_type.unwrap_or("").trim()
            )
            .as_bytes(),
        );
        h.update(b.bytes);
    }
    if let Some(r) = range {
        h.update(b"\nRange: ");
        h.update(r.trim().as_bytes());
    }
    hex::encode(h.finalize())
}

/// Paths of one cache entry.
#[derive(Debug, Clone)]
pub(crate) struct Entry {
    pub(crate) body: PathBuf,
    pub(crate) meta: PathBuf,
}

impl Entry {
    pub(crate) fn new(dir: &Path, key: &str) -> Self {
        Self {
            body: dir.join(format!("{key}.body")),
            meta: dir.join(format!("{key}.json")),
        }
    }

    /// Metadata if both files exist and the body size matches.
    pub(crate) fn load(&self) -> Result<Option<CacheMeta>> {
        let bytes = match fs::read(&self.meta) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(Error::io(&self.meta, e)),
        };
        let meta: CacheMeta = serde_json::from_slice(&bytes)
            .map_err(|e| Error::json(self.meta.display().to_string(), e))?;
        match fs::metadata(&self.body) {
            Ok(m) if m.len() == meta.size => Ok(Some(meta)),
            Ok(_) => {
                tracing::warn!(path = %self.body.display(), "cached body size mismatch, ignoring entry");
                Ok(None)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(Error::io(&self.body, e)),
        }
    }

    pub(crate) fn store_meta(&self, meta: &CacheMeta) -> Result<()> {
        let bytes =
            serde_json::to_vec_pretty(meta).map_err(|e| Error::json("cache metadata", e))?;
        write_atomic(&self.meta, &bytes)
    }
}

/// Writes `bytes` to `path` through a temporary file and a rename.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = tmp_path(path);
    let mut f = fs::File::create(&tmp).map_err(|e| Error::io(&tmp, e))?;
    f.write_all(bytes).map_err(|e| Error::io(&tmp, e))?;
    f.sync_all().map_err(|e| Error::io(&tmp, e))?;
    drop(f);
    fs::rename(&tmp, path).map_err(|e| Error::io(path, e))
}

/// Temporary sibling path used while writing `path`.
pub(crate) fn tmp_path(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(format!(".tmp{}", std::process::id()));
    path.with_file_name(name)
}

/// Streams `reader` into the (temporary) file `tmp` and returns (size, sha256 hex). The
/// caller renames it into place; on error the file is removed.
pub(crate) fn store_body(
    tmp: &Path,
    reader: &mut dyn std::io::Read,
) -> std::io::Result<(u64, String)> {
    let result = (|| {
        let mut f = fs::File::create(tmp)?;
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; 64 * 1024];
        let mut size = 0u64;
        loop {
            let n = reader.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            f.write_all(&buf[..n])?;
            size += n as u64;
        }
        f.sync_all()?;
        Ok((size, hex::encode(hasher.finalize())))
    })();
    if result.is_err() {
        let _ = fs::remove_file(tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body<'a>(bytes: &'a [u8], content_type: &'a str) -> Option<KeyBody<'a>> {
        Some(KeyBody {
            bytes,
            content_type: Some(content_type),
        })
    }

    #[test]
    fn keys_depend_on_method_url_and_body() {
        const FORM: &str = "application/x-www-form-urlencoded";
        let a = key("GET", "https://x/a", None, None);
        assert_eq!(a.len(), 64);
        assert_eq!(a, key("GET", "https://x/a", None, None));
        assert_ne!(a, key("POST", "https://x/a", None, None));
        assert_ne!(a, key("GET", "https://x/b", None, None));
        assert_ne!(
            key("POST", "u", body(b"q=1", FORM), None),
            key("POST", "u", body(b"q=2", FORM), None)
        );
        // An empty body is a body; the content type is part of it.
        assert_ne!(
            key("POST", "u", None, None),
            key("POST", "u", body(b"", FORM), None)
        );
        assert_ne!(
            key("POST", "u", body(b"{}", FORM), None),
            key("POST", "u", body(b"{}", "application/json"), None)
        );
        // The body is framed: bytes that look like the range part do not collide with a
        // real range, nor a body with the URL of another request.
        assert_ne!(
            key("POST", "u", body(b"q\nRange: bytes=0-9", FORM), None),
            key("POST", "u", body(b"q", FORM), Some("bytes=0-9"))
        );
        assert_ne!(
            key("POST", "u\nv", None, None),
            key("POST", "u", body(b"v", ""), None)
        );
        // Range requests: one entry per range, none shared with the whole resource.
        let r1 = key("GET", "https://x/a", None, Some("bytes=0-99"));
        let r2 = key("GET", "https://x/a", None, Some("bytes=100-199"));
        assert_ne!(r1, r2);
        assert_ne!(r1, a);
        assert_eq!(r1, key("GET", "https://x/a", None, Some(" bytes=0-99 ")));
        // Keys without a body are unchanged (existing cache entries stay valid).
        let mut h = Sha256::new();
        h.update(b"GET\nhttps://x/a\n");
        assert_eq!(a, hex::encode(h.finalize()));
        let mut h = Sha256::new();
        h.update(b"GET\nhttps://x/a\n\nRange: bytes=0-99");
        assert_eq!(r1, hex::encode(h.finalize()));
    }

    #[test]
    fn body_round_trip_and_size_check() {
        let dir = tempfile::tempdir().unwrap();
        let e = Entry::new(dir.path(), "k");
        assert!(e.load().unwrap().is_none());
        let tmp = tmp_path(&e.body);
        let (size, sha) = store_body(&tmp, &mut &b"hello"[..]).unwrap();
        fs::rename(&tmp, &e.body).unwrap();
        assert_eq!(size, 5);
        assert_eq!(
            sha,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
        let meta = CacheMeta {
            url: "u".into(),
            method: "GET".into(),
            status: 200,
            etag: None,
            last_modified: None,
            content_type: None,
            fetched_at_unix: 1,
            validated_at_unix: 1,
            size,
            sha256: sha,
            range: None,
        };
        e.store_meta(&meta).unwrap();
        assert_eq!(e.load().unwrap(), Some(meta));
        fs::write(&e.body, b"truncated?").unwrap();
        assert!(e.load().unwrap().is_none(), "size mismatch must invalidate");
    }
}
