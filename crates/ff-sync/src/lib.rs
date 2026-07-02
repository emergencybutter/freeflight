//! Cycle bundle discovery, checksum verification, and (eventually) atomic
//! apply for offline sync (DESIGN.md §7, §8).

pub mod checksum;
pub mod client;
pub mod manifest;

pub use checksum::{sha256_hex, verify as verify_checksum};
pub use client::{download_and_apply, fetch_latest_manifest, SyncError};
pub use manifest::{is_newer, CycleManifest};
