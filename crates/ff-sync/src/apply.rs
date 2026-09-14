//! Turning a downloaded cycle bundle into *the* cycle the client reads
//! from, and the on-disk layout that makes that swap atomic (DESIGN.md §8).
//!
//! The transfer itself isn't here. On Android the download runs in Kotlin
//! (OkHttp, on an application-scoped coroutine, resuming with a `Range`
//! request and reporting progress the UI can show) because that's where
//! the platform's networking and process-lifetime machinery lives; Rust's
//! job starts once the bytes are on disk — verify them, then make them current
//! without ever leaving a half-applied state. Splitting it there keeps a
//! second HTTP/TLS stack (and its root-certificate problem) out of the
//! `.so`, which is why `client` is an optional feature of this crate.
//!
//! The invariant every step below protects, from §8: *the previous cycle
//! stays usable until the swap completes, so the app is never
//! mid-download-unusable.* Nothing here mutates the live cycle. A new
//! cycle is assembled beside it under a temporary name, and becomes
//! current by a single rename of the `current` marker — which either
//! happened or didn't. A crash at any point leaves the old cycle intact
//! and at worst some `.incoming-*` litter, which [`apply_downloaded_bundle`]
//! clears on its next run.

use crate::checksum::{sha256_file_hex, ChecksumError};
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ApplyError {
    #[error("checksumming the downloaded bundle failed: {0}")]
    Checksum(#[from] ChecksumError),
    #[error(
        "downloaded bundle failed checksum verification (expected {expected}, got {actual}) — \
         the active cycle was left untouched"
    )]
    ChecksumMismatch { expected: String, actual: String },
    #[error("cycle id {0:?} is not usable as a directory name")]
    InvalidCycleId(String),
    #[error("filesystem error at {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
}

/// Where everything a client keeps on disk lives, rooted at one directory
/// (on Android, the app's private files dir).
///
/// ```text
/// <root>/
///   current                                  active cycle id, one line
///   cycles/<cycle_id>/cycle.sqlite           the bundle itself
///   cycles/.incoming-<cycle_id>/             half-applied, never read
///   charts/<cycle_id>/<chart_id>.pmtiles     per-cycle chart tiles
/// ```
///
/// Charts are nested under the cycle id rather than shared across cycles
/// because `ff-etl` republishes chart tiles per cycle and `chart_catalog`
/// rows carry a `cycle_id`: a chart file is only meaningful next to the
/// bundle that catalogues it, so they're installed and pruned together.
#[derive(Debug, Clone)]
pub struct BundleLayout {
    root: PathBuf,
}

impl BundleLayout {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn current_marker(&self) -> PathBuf {
        self.root.join("current")
    }

    pub fn cycles_dir(&self) -> PathBuf {
        self.root.join("cycles")
    }

    pub fn cycle_dir(&self, cycle_id: &str) -> PathBuf {
        self.cycles_dir().join(cycle_id)
    }

    pub fn bundle_path(&self, cycle_id: &str) -> PathBuf {
        self.cycle_dir(cycle_id).join("cycle.sqlite")
    }

    pub fn charts_dir(&self, cycle_id: &str) -> PathBuf {
        self.root.join("charts").join(cycle_id)
    }

    pub fn chart_path(&self, cycle_id: &str, chart_id: &str) -> PathBuf {
        self.charts_dir(cycle_id)
            .join(format!("{chart_id}.pmtiles"))
    }

    /// Scratch space for in-flight downloads. Deliberately inside `root`
    /// so a finished download can be `rename`d into place rather than
    /// copied — a cross-filesystem move of a 145MB bundle on a phone is
    /// slow enough to be visible, and wouldn't be atomic.
    pub fn downloads_dir(&self) -> PathBuf {
        self.root.join("downloads")
    }

    /// The cycle id currently marked active, if one has been applied.
    /// A marker naming a cycle whose bundle is missing reads as "no cycle"
    /// rather than handing back a path that can't be opened.
    pub fn current_cycle_id(&self) -> Option<String> {
        let marker = fs::read_to_string(self.current_marker()).ok()?;
        let cycle_id = marker.trim();
        if cycle_id.is_empty() || !self.bundle_path(cycle_id).is_file() {
            return None;
        }
        Some(cycle_id.to_string())
    }

    /// Every cycle with a readable bundle on disk, oldest first. Cycle ids
    /// sort lexicographically by date (`manifest::is_newer` relies on the
    /// same property).
    pub fn installed_cycle_ids(&self) -> Vec<String> {
        let Ok(entries) = fs::read_dir(self.cycles_dir()) else {
            return Vec::new();
        };
        let mut ids: Vec<String> = entries
            .flatten()
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                // Skips `.incoming-*` for free: those aren't valid ids.
                (is_safe_cycle_id(&name) && self.bundle_path(&name).is_file()).then_some(name)
            })
            .collect();
        ids.sort();
        ids
    }
}

/// Verify a downloaded bundle and make it the active cycle.
///
/// `downloaded` is consumed — on success it has been moved into the
/// layout, so the caller must not reuse the path. On *any* failure the
/// previously active cycle is still current and still readable.
pub fn apply_downloaded_bundle(
    layout: &BundleLayout,
    cycle_id: &str,
    downloaded: &Path,
    expected_sha256: &str,
) -> Result<(), ApplyError> {
    if !is_safe_cycle_id(cycle_id) {
        return Err(ApplyError::InvalidCycleId(cycle_id.to_string()));
    }

    // Verify before anything is moved. A bundle that fails here never
    // enters the layout at all, so there's nothing to roll back.
    let actual = sha256_file_hex(downloaded)?;
    if !actual.eq_ignore_ascii_case(expected_sha256) {
        return Err(ApplyError::ChecksumMismatch {
            expected: expected_sha256.to_string(),
            actual,
        });
    }

    let incoming = layout.cycles_dir().join(format!(".incoming-{cycle_id}"));
    // Litter from an interrupted earlier attempt; `rename` onto a
    // non-empty directory fails on Windows and on some Android
    // filesystems, so clear it rather than hoping it isn't there.
    remove_dir_if_present(&incoming)?;
    create_dir_all(&incoming)?;
    rename(downloaded, &incoming.join("cycle.sqlite"))?;

    let final_dir = layout.cycle_dir(cycle_id);
    // Re-applying a cycle already on disk is a legitimate repair path
    // (a corrupted bundle re-downloaded under the same id), and it's the
    // one case where an existing cycle directory is replaced. It's still
    // not the *active* one until the marker moves, below.
    remove_dir_if_present(&final_dir)?;
    rename(&incoming, &final_dir)?;

    // The swap. Everything above only added files nothing reads yet; this
    // single rename is what makes the new cycle live.
    let marker_tmp = layout.root.join("current.tmp");
    write(&marker_tmp, cycle_id.as_bytes())?;
    rename(&marker_tmp, &layout.current_marker())?;
    Ok(())
}

/// Delete every installed cycle older than the active one, with the chart
/// tiles installed alongside it, and report the bytes reclaimed.
///
/// Separate from [`apply_downloaded_bundle`] and never implied by it:
/// reclaiming space is a decision about a pilot's device, and the swap
/// must not be able to fail partway through because a delete did.
pub fn prune_superseded_cycles(layout: &BundleLayout) -> Result<u64, ApplyError> {
    let Some(current) = layout.current_cycle_id() else {
        return Ok(0);
    };
    let mut freed = 0;
    for cycle_id in layout.installed_cycle_ids() {
        if cycle_id >= current {
            continue;
        }
        freed += dir_size(&layout.cycle_dir(&cycle_id));
        freed += dir_size(&layout.charts_dir(&cycle_id));
        remove_dir_if_present(&layout.cycle_dir(&cycle_id))?;
        remove_dir_if_present(&layout.charts_dir(&cycle_id))?;
    }
    Ok(freed)
}

/// Cycle ids come off the network (`CycleManifest::cycle_id`) and are used
/// as path segments, so they're checked before they reach the filesystem.
/// `ff-etl` publishes dates like `"2026-07-09"`; anything that isn't
/// plainly that shape — a separator, a `..`, a leading dot — is refused
/// rather than sanitized, since a manifest that names one isn't a bundle
/// worth guessing about.
fn is_safe_cycle_id(cycle_id: &str) -> bool {
    !cycle_id.is_empty()
        && cycle_id.len() <= 64
        && cycle_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
        && !cycle_id.starts_with('.')
        && !cycle_id.contains("..")
}

fn dir_size(path: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(path) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| match entry.file_type() {
            Ok(t) if t.is_dir() => dir_size(&entry.path()),
            _ => entry.metadata().map(|m| m.len()).unwrap_or(0),
        })
        .sum()
}

fn remove_dir_if_present(path: &Path) -> Result<(), ApplyError> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(io_error(path, source)),
    }
}

fn create_dir_all(path: &Path) -> Result<(), ApplyError> {
    fs::create_dir_all(path).map_err(|source| io_error(path, source))
}

fn rename(from: &Path, to: &Path) -> Result<(), ApplyError> {
    fs::rename(from, to).map_err(|source| io_error(to, source))
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), ApplyError> {
    if let Some(parent) = path.parent() {
        create_dir_all(parent)?;
    }
    fs::write(path, bytes).map_err(|source| io_error(path, source))
}

fn io_error(path: &Path, source: std::io::Error) -> ApplyError {
    ApplyError::Io {
        path: path.display().to_string(),
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checksum::sha256_hex;

    struct Fixture {
        _dir: tempfile::TempDir,
        layout: BundleLayout,
    }

    impl Fixture {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let layout = BundleLayout::new(dir.path().join("data"));
            Self { _dir: dir, layout }
        }

        /// Writes `contents` where a download would have landed and
        /// applies it, as the client does.
        fn apply(&self, cycle_id: &str, contents: &[u8]) -> Result<(), ApplyError> {
            self.apply_claiming(cycle_id, contents, &sha256_hex(contents))
        }

        fn apply_claiming(
            &self,
            cycle_id: &str,
            contents: &[u8],
            sha256: &str,
        ) -> Result<(), ApplyError> {
            let downloads = self.layout.downloads_dir();
            fs::create_dir_all(&downloads).unwrap();
            let staged = downloads.join("staged.partial");
            fs::write(&staged, contents).unwrap();
            apply_downloaded_bundle(&self.layout, cycle_id, &staged, sha256)
        }

        fn active_bundle_contents(&self) -> Option<Vec<u8>> {
            let id = self.layout.current_cycle_id()?;
            fs::read(self.layout.bundle_path(&id)).ok()
        }
    }

    #[test]
    fn a_fresh_install_has_no_current_cycle() {
        let fixture = Fixture::new();
        assert_eq!(fixture.layout.current_cycle_id(), None);
        assert!(fixture.layout.installed_cycle_ids().is_empty());
    }

    #[test]
    fn applying_a_bundle_makes_it_current() {
        let fixture = Fixture::new();
        fixture.apply("2026-07-09", b"cycle one").unwrap();

        assert_eq!(
            fixture.layout.current_cycle_id().as_deref(),
            Some("2026-07-09")
        );
        assert_eq!(fixture.active_bundle_contents().unwrap(), b"cycle one");
    }

    #[test]
    fn a_second_cycle_supersedes_the_first_but_leaves_it_on_disk() {
        let fixture = Fixture::new();
        fixture.apply("2026-07-09", b"cycle one").unwrap();
        fixture.apply("2026-08-06", b"cycle two").unwrap();

        assert_eq!(
            fixture.layout.current_cycle_id().as_deref(),
            Some("2026-08-06")
        );
        assert_eq!(fixture.active_bundle_contents().unwrap(), b"cycle two");
        // §8: the previous cycle stays usable until it's explicitly pruned.
        assert_eq!(
            fixture.layout.installed_cycle_ids(),
            vec!["2026-07-09".to_string(), "2026-08-06".to_string()]
        );
    }

    #[test]
    fn a_corrupt_download_is_refused_and_the_active_cycle_survives() {
        let fixture = Fixture::new();
        fixture.apply("2026-07-09", b"cycle one").unwrap();

        let err = fixture
            .apply_claiming("2026-08-06", b"truncated", &sha256_hex(b"cycle two"))
            .unwrap_err();

        assert!(matches!(err, ApplyError::ChecksumMismatch { .. }));
        assert_eq!(
            fixture.layout.current_cycle_id().as_deref(),
            Some("2026-07-09")
        );
        assert_eq!(fixture.active_bundle_contents().unwrap(), b"cycle one");
        assert!(!fixture.layout.cycle_dir("2026-08-06").exists());
    }

    #[test]
    fn an_interrupted_earlier_attempt_does_not_block_a_retry() {
        let fixture = Fixture::new();
        // What a crash between "assemble" and "swap" leaves behind.
        let incoming = fixture.layout.cycles_dir().join(".incoming-2026-07-09");
        fs::create_dir_all(&incoming).unwrap();
        fs::write(incoming.join("cycle.sqlite"), b"half a bundle").unwrap();

        fixture.apply("2026-07-09", b"cycle one").unwrap();

        assert_eq!(fixture.active_bundle_contents().unwrap(), b"cycle one");
        assert!(!incoming.exists());
    }

    #[test]
    fn re_applying_the_same_cycle_id_replaces_the_bundle() {
        let fixture = Fixture::new();
        fixture.apply("2026-07-09", b"corrupted somehow").unwrap();
        fixture.apply("2026-07-09", b"re-downloaded").unwrap();

        assert_eq!(fixture.active_bundle_contents().unwrap(), b"re-downloaded");
        assert_eq!(fixture.layout.installed_cycle_ids().len(), 1);
    }

    #[test]
    fn a_marker_pointing_at_a_missing_bundle_reads_as_no_cycle() {
        let fixture = Fixture::new();
        fixture.apply("2026-07-09", b"cycle one").unwrap();
        fs::remove_dir_all(fixture.layout.cycle_dir("2026-07-09")).unwrap();

        assert_eq!(fixture.layout.current_cycle_id(), None);
    }

    #[test]
    fn cycle_ids_that_would_escape_the_layout_are_refused() {
        let fixture = Fixture::new();
        for hostile in ["../evil", "..", ".hidden", "a/b", ""] {
            let err = fixture.apply(hostile, b"payload").unwrap_err();
            assert!(
                matches!(err, ApplyError::InvalidCycleId(_)),
                "{hostile:?} should have been refused, got {err:?}"
            );
        }
    }

    #[test]
    fn pruning_removes_older_cycles_and_their_charts_but_not_the_active_one() {
        let fixture = Fixture::new();
        fixture.apply("2026-07-09", b"cycle one").unwrap();
        let old_chart = fixture.layout.chart_path("2026-07-09", "seattle");
        fs::create_dir_all(old_chart.parent().unwrap()).unwrap();
        fs::write(&old_chart, vec![0u8; 1024]).unwrap();
        fixture.apply("2026-08-06", b"cycle two").unwrap();

        let freed = prune_superseded_cycles(&fixture.layout).unwrap();

        assert!(
            freed >= 1024,
            "expected the chart's bytes counted, got {freed}"
        );
        assert!(!old_chart.exists());
        assert_eq!(
            fixture.layout.installed_cycle_ids(),
            vec!["2026-08-06".to_string()]
        );
        assert_eq!(fixture.active_bundle_contents().unwrap(), b"cycle two");
    }

    #[test]
    fn pruning_with_no_cycle_installed_is_a_no_op() {
        let fixture = Fixture::new();
        assert_eq!(prune_superseded_cycles(&fixture.layout).unwrap(), 0);
    }
}
