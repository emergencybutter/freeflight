//! HTTP side of cycle sync, behind the optional `client` feature.
//!
//! Nothing in-tree enables it today. The Android client — the only
//! offline-capable one (DESIGN.md §8) — does its transfers in Kotlin so
//! the download can survive the app going to the background, resume with
//! a `Range` request, and drive a notification's progress bar, none of
//! which a Rust HTTP call from inside a JNI frame gets for free. It calls
//! back into [`crate::apply`] once the bytes are on disk. This module is
//! the equivalent path for a Rust-side caller (a desktop tool, a test
//! harness, a future headless client) that just wants the whole thing
//! done in one call.

use crate::apply::{apply_downloaded_bundle, ApplyError, BundleLayout};
use crate::manifest::CycleManifest;
use std::io::Write;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SyncError {
    #[error("request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("writing the downloaded bundle failed: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Apply(#[from] ApplyError),
}

/// Fetch the manifest for the latest published cycle from `ff-api`
/// (DESIGN.md §7 `GET /cycles/latest`).
pub async fn fetch_latest_manifest(
    http: &reqwest::Client,
    api_base_url: &str,
) -> Result<CycleManifest, SyncError> {
    let url = format!("{api_base_url}/cycles/latest");
    let resp = http.get(&url).send().await?.error_for_status()?;
    Ok(resp.json::<CycleManifest>().await?)
}

/// Download the bundle `manifest` describes, verify it, and swap it in as
/// the active cycle of `layout`.
///
/// Streams to a file under the layout's downloads directory rather than
/// buffering the body: a real bundle is ~145MB (§11). No resume — a caller
/// that needs one wants the Kotlin path this module's docs describe.
pub async fn download_and_apply(
    http: &reqwest::Client,
    layout: &BundleLayout,
    manifest: &CycleManifest,
) -> Result<(), SyncError> {
    let downloads = layout.downloads_dir();
    std::fs::create_dir_all(&downloads)?;
    let staged = downloads.join("cycle.sqlite.partial");

    let mut resp = http
        .get(&manifest.sqlite_url)
        .send()
        .await?
        .error_for_status()?;
    let mut file = std::fs::File::create(&staged)?;
    while let Some(chunk) = resp.chunk().await? {
        file.write_all(&chunk)?;
    }
    file.sync_all()?;
    drop(file);

    apply_downloaded_bundle(layout, &manifest.cycle_id, &staged, &manifest.sqlite_sha256)?;
    Ok(())
}
