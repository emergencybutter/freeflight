//! Reading raster chart tiles out of locally-installed PMTiles archives.
//!
//! On web, MapLibre GL JS speaks PMTiles itself over HTTP range requests
//! (DESIGN.md §4) — there is no local copy to read. Android is the offline
//! client, so the same archive lives on the device and something has to
//! turn `(z, x, y)` into PNG bytes. That something is here, because the
//! archive format is already a Rust dependency (`ff-charts` writes these
//! files with the same crate) and because a tile read must not cost a
//! JNI-side file parse per tile.
//!
//! Opened archives are cached and kept open. `PMTiles::from_reader` parses
//! the directories — offset/length per tile — but not the tile bodies,
//! which stay on disk and are seeked to on demand; so a held-open sectional
//! costs its directory, not its ~100MB of PNGs. Re-opening per tile would
//! re-parse that directory for every one of the dozens of tiles a single
//! map frame asks for.

use crate::error::CoreError;
use pmtiles2::PMTiles;
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;

/// The zoom levels an archive actually holds tiles for.
///
/// The map has to be told this. A raster source asked for tiles outside
/// the range gets 404s and draws nothing — so a chart tiled to zoom 11
/// (what `ff-etl`'s overview factors produce for a sectional) silently
/// goes blank the moment the pilot zooms in past it, which is precisely
/// when they are looking hardest. Given the real range, MapLibre scales
/// the deepest tiles up instead.
#[derive(Debug, Clone, Copy)]
pub struct ZoomRange {
    pub min: u8,
    pub max: u8,
}

/// Read the zoom range out of a PMTiles file's fixed 127-byte v3 header.
///
/// Deliberately not `PMTiles::from_reader`: that parses every directory in
/// the archive, which for a 250MB sectional is real work, and this is
/// called for each installed chart just to populate a list. The header's
/// layout is fixed by the PMTiles v3 specification — `min_zoom` and
/// `max_zoom` are single bytes at offsets 100 and 101, after the 7-byte
/// magic — so two bytes is all it costs.
pub fn zoom_range(path: &Path) -> Option<ZoomRange> {
    let mut header = [0u8; 127];
    File::open(path).ok()?.read_exact(&mut header).ok()?;
    if &header[..7] != b"PMTiles" {
        return None;
    }
    let (min, max) = (header[100], header[101]);
    // A max below the min is a malformed header, not a usable range.
    (min <= max).then_some(ZoomRange { min, max })
}

type OpenArchive = PMTiles<File>;

/// Lazily-opened chart archives, keyed by `chart_catalog.id`.
#[derive(Default)]
pub struct ChartCache {
    open: HashMap<String, OpenArchive>,
    /// Ids whose archive failed to open, so a corrupt or truncated file
    /// isn't re-parsed on every tile request for the rest of the session.
    /// Cleared whenever a chart is installed or removed.
    failed: HashMap<String, String>,
}

impl ChartCache {
    /// PNG bytes for one tile, or `None` where the archive has no tile —
    /// which is normal, not an error: a sectional covers a quadrilateral,
    /// and the map asks for the whole square viewport around it.
    pub fn tile(
        &mut self,
        chart_id: &str,
        path: &Path,
        z: u8,
        x: u64,
        y: u64,
    ) -> Result<Option<Vec<u8>>, CoreError> {
        if let Some(err) = self.failed.get(chart_id) {
            return Err(CoreError::Chart(err.clone()));
        }
        if !self.open.contains_key(chart_id) {
            let opened = File::open(path)
                .map_err(|e| format!("opening {}: {e}", path.display()))
                .and_then(|file| {
                    PMTiles::from_reader(file).map_err(|e| format!("reading {chart_id}: {e}"))
                });
            match opened {
                Ok(archive) => {
                    self.open.insert(chart_id.to_string(), archive);
                }
                Err(message) => {
                    self.failed.insert(chart_id.to_string(), message.clone());
                    return Err(CoreError::Chart(message));
                }
            }
        }
        let archive = self
            .open
            .get_mut(chart_id)
            .expect("just inserted above if absent");
        // Note the argument order: pmtiles2 takes (x, y, z), not (z, x, y).
        archive
            .get_tile(x, y, z)
            .map_err(|e| CoreError::Chart(format!("tile {z}/{x}/{y} of {chart_id}: {e}")))
    }

    /// Drop any cached handle for `chart_id`. Called when a chart is
    /// installed or deleted, so a replaced file is never served from a
    /// stale open handle — and, on Windows-like filesystems, so the old
    /// file isn't held open while something tries to delete it.
    pub fn forget(&mut self, chart_id: &str) {
        self.open.remove(chart_id);
        self.failed.remove(chart_id);
    }

    pub fn forget_all(&mut self) {
        self.open.clear();
        self.failed.clear();
    }
}
