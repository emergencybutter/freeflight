use crate::manifest::CycleManifest;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SyncError {
    #[error("request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("downloaded bundle failed checksum verification")]
    ChecksumMismatch,
    #[error("applying a downloaded cycle bundle is not implemented yet")]
    ApplyNotImplemented,
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

/// Download, checksum-verify, and atomically swap in a new cycle bundle.
///
/// Not implemented: the "swap in" step is platform-specific (OPFS file
/// replace on web vs. native filesystem rename on Android) and belongs in
/// `ff-wasm`/`ff-uniffi` where that platform context is available; this
/// function is the seam those bindings call through once written.
pub async fn download_and_apply(
    _http: &reqwest::Client,
    _manifest: &CycleManifest,
) -> Result<(), SyncError> {
    Err(SyncError::ApplyNotImplemented)
}
