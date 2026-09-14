//! Cycle bundle discovery, checksum verification, and atomic apply for
//! offline sync (DESIGN.md §7, §8).
//!
//! The bytes-over-the-wire half (`client`) is an optional feature; the
//! parts every consumer needs — the `/cycles/latest` wire type and the
//! checksum/apply steps that guard a bundle before it becomes the active
//! cycle — are always available. See `apply`'s module docs for why the
//! Android client downloads in Kotlin and only comes back to Rust to
//! verify and swap.

pub mod apply;
pub mod checksum;
#[cfg(feature = "client")]
pub mod client;
pub mod manifest;

pub use apply::{apply_downloaded_bundle, prune_superseded_cycles, ApplyError, BundleLayout};
pub use checksum::{sha256_file_hex, sha256_hex, verify as verify_checksum, ChecksumError};
#[cfg(feature = "client")]
pub use client::{download_and_apply, fetch_latest_manifest, SyncError};
pub use manifest::{is_newer, CycleManifest};
