//! Plays the phone: syncs a pack directory from a skimap server and prints the requests
//! and bytes it took. What the app does in Kotlin, with the same pack-crate calls.
//!
//! ```sh
//! cargo run --release -p skimap-server --example phone_sync -- http://127.0.0.1:8080 <site> /tmp/phone/<site>
//! ```

#![allow(
    clippy::disallowed_methods,
    reason = "developer tool, not a pipeline output"
)]
use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Context, Result, bail};
use skimap_pack::sync::{SyncState, plan, plan_full, plan_whole_files, read_state, write_state};
use skimap_pack::update::{MISSING_CONTENTS, PATCH_FAILED, apply_bundle};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [base, site, dir] = args.as_slice() else {
        bail!("usage: phone_sync <server URL> <site> <pack directory>");
    };
    let pack = PathBuf::from(dir);
    if let Some(p) = pack.parent() {
        std::fs::create_dir_all(p)?;
    }
    let tls = std::sync::Arc::unwrap_or_clone(skimap_server::alerts::mail::tls_config()?);
    let http = reqwest::blocking::Client::builder()
        .tls_backend_preconfigured(tls)
        .build()?;
    let t0 = Instant::now();
    let state = read_state(&pack)?;
    let mut rq = http.get(format!("{base}/v1/sites/{site}/index.json"));
    if let Some(e) = state.as_ref().and_then(|s| s.index_etag.as_deref()) {
        rq = rq.header("If-None-Match", e);
    }
    let r = rq.send()?;
    let mut requests = 1;
    let status = r.status().as_u16();
    let etag = r
        .headers()
        .get("etag")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let body = r.bytes()?;
    let mut bytes = body.len() as u64;
    if status == 304 {
        println!("up to date: 1 request, {bytes} bytes of body (304)");
        return Ok(());
    }
    if status != 200 {
        bail!("index: HTTP {status}");
    }
    let index: skimap_pack::sync::SiteIndex = serde_json::from_slice(&body)?;
    let mut todo = plan(&index, state.as_ref());
    let work = pack.with_file_name(format!(
        ".{}.download",
        pack.file_name().unwrap_or_default().to_string_lossy()
    ));
    std::fs::create_dir_all(&work)?;
    let mut fell_back = false;
    let mut whole_files = std::env::var_os("PHONE_SYNC_WHOLE_FILES").is_some();
    if whole_files {
        todo = plan_whole_files(&index, state.as_ref());
    }
    let mut i = 0;
    while i < todo.len() {
        let b = &todo[i];
        let data = http
            .get(format!("{base}{}", b.url))
            .send()?
            .error_for_status()?
            .bytes()?;
        requests += 1;
        bytes += data.len() as u64;
        let path = work.join(&b.sha256);
        std::fs::write(&path, &data)?;
        let t = Instant::now();
        match apply_bundle(&pack, &path) {
            Ok(a) => {
                println!(
                    "bundle {} -> {}: {} bytes, {} files written, {} reused, {} removed, applied in {:.2} s",
                    b.from.as_deref().unwrap_or("(nothing)"),
                    b.to,
                    data.len(),
                    a.fetched_files,
                    a.reused_files,
                    a.removed_files,
                    t.elapsed().as_secs_f64()
                );
                std::fs::remove_file(&path)?;
                i += 1;
            }
            Err(e) if e.to_string().contains(PATCH_FAILED) && !whole_files => {
                println!("{e}: falling back to the bundles with whole files");
                whole_files = true;
                todo = plan_whole_files(&index, state.as_ref());
                i = 0;
            }
            Err(e) if e.to_string().contains(MISSING_CONTENTS) && !fell_back => {
                println!("{e}: falling back to the base bundle");
                fell_back = true;
                todo = plan_full(&index);
                i = 0;
            }
            Err(e) => return Err(e).context("applying the bundle"),
        }
    }
    write_state(
        &pack,
        &SyncState {
            version: index.latest.version.clone(),
            base_version: index.base_version.clone(),
            index_etag: etag,
        },
    )?;
    let _ = std::fs::remove_dir(&work);
    println!(
        "synced to {}: {requests} requests, {bytes} bytes, {:.2} s",
        index.latest.version,
        t0.elapsed().as_secs_f64()
    );
    Ok(())
}
