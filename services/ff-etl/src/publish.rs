//! Publishes a built cycle bundle to a local directory and updates a
//! "latest" pointer file — a stand-in for DESIGN.md §7's "upload to
//! object storage and flip the 'latest' pointer" until this project has
//! an actual bucket to publish to. Same interface either way, so
//! swapping in real object storage later only touches this module.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PublishError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Serialize, Deserialize)]
struct LatestPointer {
    cycle_id: String,
    /// Relative to the data directory, so the pointer file stays valid if
    /// the whole data directory is moved/re-rooted.
    sqlite_path: String,
}

/// Copies `bundle_path` into `<data_dir>/cycles/<cycle_id>/cycle.sqlite`
/// and updates `<data_dir>/latest.json` to point at it. Returns the
/// published file's path.
pub fn publish_bundle(bundle_path: &Path, cycle_id: &str, data_dir: &Path) -> Result<PathBuf, PublishError> {
    let cycle_dir = data_dir.join("cycles").join(cycle_id);
    std::fs::create_dir_all(&cycle_dir)?;
    let published_path = cycle_dir.join("cycle.sqlite");
    std::fs::copy(bundle_path, &published_path)?;

    let pointer = LatestPointer {
        cycle_id: cycle_id.to_string(),
        sqlite_path: format!("cycles/{cycle_id}/cycle.sqlite"),
    };
    std::fs::write(data_dir.join("latest.json"), serde_json::to_string_pretty(&pointer)?)?;

    Ok(published_path)
}

/// Reads `<data_dir>/latest.json`, if present, and returns the path to
/// that cycle's published bundle (for `validate_bundle` to compare
/// against, and for `ff-api` to serve).
pub fn latest_bundle_path(data_dir: &Path) -> Result<Option<PathBuf>, PublishError> {
    let pointer_path = data_dir.join("latest.json");
    if !pointer_path.exists() {
        return Ok(None);
    }
    let pointer: LatestPointer = serde_json::from_str(&std::fs::read_to_string(&pointer_path)?)?;
    Ok(Some(data_dir.join(pointer.sqlite_path)))
}
