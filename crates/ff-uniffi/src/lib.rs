//! UniFFI bindings over the freeflight Rust core for the Android client
//! (DESIGN.md §4, §5).
//!
//! Android is the offline-capable client (§8), so this binding is wider
//! than `ff-wasm`'s: as well as the shared planning math that both clients
//! run, it owns the device's local cycle bundle — opening it, querying it,
//! swapping in a newer one, and reading chart tiles out of the PMTiles
//! archives installed beside it. The queries answer exactly what `ff-api`'s
//! `/data/*` routes answer for web, from a file on the device instead of a
//! file on a server, which is the whole difference between the two clients.
//!
//! What is *not* here: HTTP. Every byte this binding consumes was fetched
//! by Kotlin (`ff-sync`'s `apply` module explains why) and handed over as a
//! path. That keeps a second TLS stack and its root-certificate problem out
//! of the `.so`, and leaves downloads where Android's foreground-service and
//! progress-notification machinery can reach them.
//!
//! Uses UniFFI's proc-macro export mode (no `.udl` file). Kotlin bindings
//! are generated from the built library with `uniffi-bindgen` — see
//! `apps/android`'s Gradle wiring.

mod charts;
mod error;
mod query;
mod types;

use charts::ChartCache;
pub use error::CoreError;
pub use types::*;

use ff_planning::{
    distance_nm as core_distance_nm, initial_bearing_deg as core_initial_bearing_deg, plan_route,
    AircraftProfile, RoutePoint, Wind,
};
use ff_sync::{apply_downloaded_bundle, prune_superseded_cycles, BundleLayout};
use rusqlite::Connection;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

uniffi::setup_scaffolding!();

/// The app's handle on everything stored for it on this device.
///
/// One instance per process, constructed with the app's private files
/// directory. It is `Send + Sync` and every method takes `&self`, so Kotlin
/// can hold a single instance and call it from any dispatcher; the state
/// behind it (an open SQLite connection and any open chart archives) sits
/// behind one mutex, since SQLite reads here are single-milliseconds and
/// not worth a connection pool.
#[derive(uniffi::Object)]
pub struct Freeflight {
    layout: BundleLayout,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    /// The open bundle and the cycle it belongs to, so a cycle swapped in
    /// while the app is running is picked up without a restart.
    db: Option<(String, Connection)>,
    charts: ChartCache,
}

#[uniffi::export]
impl Freeflight {
    /// `data_dir` is the app's private files directory (Kotlin:
    /// `context.filesDir`). Nothing is opened or created here — a first run
    /// with no cycle downloaded is a normal state, not a failure.
    #[uniffi::constructor]
    pub fn new(data_dir: String) -> Arc<Self> {
        Arc::new(Self {
            layout: BundleLayout::new(PathBuf::from(data_dir)),
            state: Mutex::new(State::default()),
        })
    }

    // ---- cycle lifecycle -------------------------------------------------

    /// What the app is flying on: `Ok(None)` before the first sync, an
    /// error if a cycle is installed but unreadable. The UI is required to
    /// show this (§11: never let a pilot mistake stale data for current),
    /// so it reads the counts and the effective date out of the bundle
    /// itself rather than trusting the directory name.
    pub fn current_cycle(&self) -> Result<Option<CycleInfo>, CoreError> {
        let Some(cycle_id) = self.layout.current_cycle_id() else {
            return Ok(None);
        };
        let bundle_bytes = fs::metadata(self.layout.bundle_path(&cycle_id))
            .map(|m| m.len())
            .unwrap_or(0);
        let mut state = self.lock()?;
        // Deliberately propagated rather than swallowed into `None`. A
        // bundle that is present but won't open — a bad download, a
        // half-applied migration — is not the same thing as no bundle, and
        // reporting it as "nothing downloaded" tells a pilot something
        // false about what their device is carrying (§11).
        let conn = self.connection(&mut state)?;
        Ok(Some(CycleInfo {
            effective_date: query::cycle_effective_date(conn),
            airport_count: query::count(conn, "airport"),
            procedure_count: query::count(conn, "procedure"),
            installed_chart_ids: self.installed_chart_ids(conn),
            cycle_id,
            bundle_bytes,
        }))
    }

    /// Parse a `GET /cycles/latest` body. Deliberately Rust's job: the wire
    /// type is `ff_sync::CycleManifest`, the same type `ff-api` serializes,
    /// so the two cannot disagree about the shape — §4.1 records that they
    /// silently did once.
    pub fn parse_manifest(&self, json: String) -> Result<CycleManifest, CoreError> {
        let manifest: ff_sync::CycleManifest =
            serde_json::from_str(&json).map_err(|e| CoreError::InvalidManifest(e.to_string()))?;
        Ok(CycleManifest {
            cycle_id: manifest.cycle_id,
            sqlite_url: manifest.sqlite_url,
            sqlite_sha256: manifest.sqlite_sha256,
        })
    }

    /// Whether `cycle_id` is worth downloading — i.e. newer than whatever
    /// is installed. `true` on a fresh install.
    pub fn is_update_available(&self, cycle_id: String) -> bool {
        ff_sync::is_newer(self.layout.current_cycle_id().as_deref(), &cycle_id)
    }

    /// Directory Kotlin should download into. Inside the app's data root on
    /// purpose: a finished download is then `rename`d into place instead of
    /// copied, which for a ~145MB bundle is the difference between instant
    /// and visibly slow.
    pub fn downloads_dir(&self) -> Result<String, CoreError> {
        let dir = self.layout.downloads_dir();
        fs::create_dir_all(&dir).map_err(|e| CoreError::Sync(e.to_string()))?;
        Ok(dir.display().to_string())
    }

    /// Verify a downloaded bundle and make it the active cycle. The open
    /// connection is dropped first so the swap never races a read, and
    /// reopening on the next query picks up the new file.
    pub fn apply_cycle(
        &self,
        cycle_id: String,
        downloaded_path: String,
        sha256: String,
    ) -> Result<(), CoreError> {
        let mut state = self.lock()?;
        state.db = None;
        state.charts.forget_all();
        apply_downloaded_bundle(
            &self.layout,
            &cycle_id,
            Path::new(&downloaded_path),
            &sha256,
        )?;
        Ok(())
    }

    /// Delete superseded cycles, and any chart archive the active cycle no
    /// longer refers to; returns the bytes reclaimed.
    ///
    /// Charts shared with the active cycle survive — that is the point of
    /// addressing them by content. Only genuinely orphaned archives go.
    pub fn prune_old_cycles(&self) -> Result<u64, CoreError> {
        let mut state = self.lock()?;
        state.charts.forget_all();
        let mut freed = prune_superseded_cycles(&self.layout)?;
        // Read the surviving catalogue *after* the cycle prune, so the
        // keep-set reflects what is actually still installed.
        let keep: std::collections::HashSet<String> =
            query::catalogued_chart_hashes(self.connection(&mut state)?)?
                .into_iter()
                .collect();
        freed += ff_sync::prune_chart_blobs(&self.layout, &keep)?;
        Ok(freed)
    }

    // ---- charts ----------------------------------------------------------

    /// Every chart this cycle catalogues, each marked with whether its
    /// archive is already on the device.
    ///
    /// "Already on the device" is answered by content hash, so a chart
    /// carried over unchanged from the previous cycle reports as installed
    /// the moment the new cycle is applied — nothing to re-download.
    pub fn charts(&self) -> Result<Vec<Chart>, CoreError> {
        self.require_cycle()?;
        let mut state = self.lock()?;
        let conn = self.connection(&mut state)?;
        let mut charts = query::charts(conn)?;
        for chart in &mut charts {
            let (hash, _) = query::chart_hash(conn, &chart.id)?;
            let Some(path) = self.layout.chart_blob_path(&hash) else {
                continue;
            };
            if let Ok(meta) = fs::metadata(&path) {
                chart.installed = true;
                chart.installed_bytes = meta.len();
                if let Some(range) = charts::zoom_range(&path) {
                    chart.min_zoom = range.min;
                    chart.max_zoom = range.max;
                }
            }
        }
        Ok(charts)
    }

    /// Verify a downloaded chart archive and move it into the blob store.
    ///
    /// The checksum comes from `chart_catalog.sha256`, so unlike before,
    /// a truncated or corrupted chart download is now caught here rather
    /// than surfacing later as missing tiles. Bundles published before that
    /// column existed have nothing to compare against; those install
    /// unverified (see `query::chart_blob_key`).
    pub fn install_chart(
        &self,
        chart_id: String,
        downloaded_path: String,
    ) -> Result<(), CoreError> {
        self.require_cycle()?;
        let downloaded = Path::new(&downloaded_path);
        let mut state = self.lock()?;
        let (expected, verifiable) = query::chart_hash(self.connection(&mut state)?, &chart_id)?;

        if verifiable {
            let actual = ff_sync::sha256_file_hex(downloaded)
                .map_err(|e| CoreError::Chart(e.to_string()))?;
            if !actual.eq_ignore_ascii_case(&expected) {
                // Left where it is: the downloader resumes from a partial
                // file, and deleting it here would turn a corrupted tail
                // into a full re-download.
                return Err(CoreError::Chart(format!(
                    "{chart_id} failed checksum verification                      (expected {expected}, got {actual})"
                )));
            }
        }

        let target = self
            .layout
            .chart_blob_path(&expected)
            .ok_or_else(|| CoreError::Chart(format!("unusable content hash for {chart_id}")))?;
        fs::create_dir_all(self.layout.chart_blobs_dir())
            .map_err(|e| CoreError::Chart(e.to_string()))?;
        state.charts.forget(&expected);
        fs::rename(downloaded, &target)
            .map_err(|e| CoreError::Chart(format!("installing {chart_id}: {e}")))?;
        Ok(())
    }

    /// Delete this chart's archive.
    ///
    /// Removes the blob, which is shared: if another chart in this cycle
    /// resolves to the same content it goes too. That only happens when the
    /// archives are byte-identical, so it is the same file either way.
    pub fn remove_chart(&self, chart_id: String) -> Result<(), CoreError> {
        self.require_cycle()?;
        let mut state = self.lock()?;
        let (hash, _) = query::chart_hash(self.connection(&mut state)?, &chart_id)?;
        let Some(target) = self.layout.chart_blob_path(&hash) else {
            return Ok(());
        };
        state.charts.forget(&hash);
        match fs::remove_file(&target) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(CoreError::Chart(format!("removing {chart_id}: {e}"))),
        }
    }

    /// PNG bytes for one raster tile, or `null` where this chart has no
    /// tile there — which is the common case at the edges of a sectional
    /// and must not read as an error.
    pub fn chart_tile(
        &self,
        chart_id: String,
        z: u8,
        x: u32,
        y: u32,
    ) -> Result<Option<Vec<u8>>, CoreError> {
        self.require_cycle()?;
        let mut state = self.lock()?;
        let (hash, _) = query::chart_hash(self.connection(&mut state)?, &chart_id)?;
        let Some(path) = self.layout.chart_blob_path(&hash) else {
            return Ok(None);
        };
        if !path.is_file() {
            return Ok(None);
        }
        // Keyed on the content hash, not the chart id: two cycles naming
        // the same archive share one open handle.
        state.charts.tile(&hash, &path, z, x as u64, y as u64)
    }

    // ---- cycle queries ---------------------------------------------------

    pub fn airports_in_bbox(
        &self,
        bbox: BoundingBox,
        limit: u32,
    ) -> Result<Vec<Airport>, CoreError> {
        self.with_db(|conn| query::airports_in_bbox(conn, bbox, limit))
    }

    pub fn search(&self, query_text: String, limit: u32) -> Result<Vec<SearchHit>, CoreError> {
        self.with_db(|conn| query::search(conn, &query_text, limit))
    }

    pub fn airport(&self, icao: String) -> Result<AirportDetail, CoreError> {
        self.with_db(|conn| query::airport_detail(conn, &icao))
    }

    pub fn airport_procedures(&self, icao: String) -> Result<Vec<Procedure>, CoreError> {
        self.with_db(|conn| query::airport_procedures(conn, &icao))
    }

    pub fn procedure(&self, id: String) -> Result<ProcedureDetail, CoreError> {
        self.with_db(|conn| query::procedure_detail(conn, &id))
    }

    pub fn airspace_in_bbox(&self, bbox: BoundingBox) -> Result<Vec<Airspace>, CoreError> {
        self.with_db(|conn| query::airspace_in_bbox(conn, bbox))
    }

    pub fn attributions(&self) -> Result<Vec<DataSourceCredit>, CoreError> {
        self.with_db(query::attributions)
    }

    /// Every plate this airport publishes, with `installed` reflecting
    /// what is on disk right now.
    pub fn airport_plates(&self, icao: String) -> Result<Vec<Plate>, CoreError> {
        let mut plates = self.with_db(|conn| query::airport_plates(conn, &icao))?;
        for plate in &mut plates {
            plate.installed = self.layout.plate_path(&plate.pdf_url).is_file();
        }
        Ok(plates)
    }

    /// Absolute path to this plate's PDF, or `null` when it isn't here.
    ///
    /// The viewer asks this first and only falls back to the network when
    /// it comes back null, so a plate taken along on the ground opens with
    /// the radios off — which is the whole point of the Android client
    /// (DESIGN.md §8).
    pub fn plate_path(&self, pdf_url: String) -> Option<String> {
        let path = self.layout.plate_path(&pdf_url);
        path.is_file().then(|| path.to_string_lossy().into_owned())
    }

    /// Where a plate download should be written before [`Self::install_plate`].
    pub fn plate_target_path(&self, pdf_url: String) -> Result<String, CoreError> {
        let path = self.layout.plate_path(&pdf_url);
        fs::create_dir_all(self.layout.plates_dir())
            .map_err(|e| CoreError::Chart(format!("preparing the plate store: {e}")))?;
        Ok(path.to_string_lossy().into_owned())
    }

    /// Move a finished download into the plate store.
    ///
    /// No checksum to verify against — the bundle publishes none for
    /// plates — so this checks the file is a PDF rather than installing
    /// whatever arrived. A captive-portal login page saved as an approach
    /// plate would otherwise sit there looking downloaded until a pilot
    /// opened it in the air.
    pub fn install_plate(&self, pdf_url: String, downloaded_path: String) -> Result<(), CoreError> {
        let downloaded = Path::new(&downloaded_path);
        let mut header = [0u8; 5];
        let read = fs::File::open(downloaded)
            .and_then(|mut f| {
                use std::io::Read;
                f.read(&mut header)
            })
            .map_err(|e| CoreError::Chart(format!("reading the downloaded plate: {e}")))?;
        if &header[..read] != b"%PDF-" {
            let _ = fs::remove_file(downloaded);
            return Err(CoreError::Chart(
                "the downloaded plate is not a PDF — check the network connection".to_string(),
            ));
        }

        let target = self.layout.plate_path(&pdf_url);
        fs::create_dir_all(self.layout.plates_dir())
            .map_err(|e| CoreError::Chart(e.to_string()))?;
        if downloaded != target {
            fs::rename(downloaded, &target)
                .map_err(|e| CoreError::Chart(format!("installing the plate: {e}")))?;
        }
        Ok(())
    }

    /// Delete every plate on disk, and report how many bytes that freed.
    pub fn clear_plates(&self) -> Result<u64, CoreError> {
        let Ok(entries) = fs::read_dir(self.layout.plates_dir()) else {
            return Ok(0);
        };
        let mut freed = 0u64;
        for entry in entries.flatten() {
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            if fs::remove_file(entry.path()).is_ok() {
                freed += size;
            }
        }
        Ok(freed)
    }

    /// Bytes the plate store currently occupies.
    pub fn plates_bytes(&self) -> u64 {
        let Ok(entries) = fs::read_dir(self.layout.plates_dir()) else {
            return 0;
        };
        entries
            .flatten()
            .filter_map(|e| e.metadata().ok().map(|m| m.len()))
            .sum()
    }

    // ---- internals -------------------------------------------------------
}

impl Freeflight {
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, State>, CoreError> {
        self.state
            .lock()
            .map_err(|_| CoreError::Database("core state lock was poisoned".to_string()))
    }

    fn require_cycle(&self) -> Result<String, CoreError> {
        self.layout.current_cycle_id().ok_or(CoreError::NoCycle)
    }

    fn with_db<T>(
        &self,
        run: impl FnOnce(&Connection) -> Result<T, CoreError>,
    ) -> Result<T, CoreError> {
        let mut state = self.lock()?;
        let conn = self.connection(&mut state)?;
        run(conn)
    }

    /// The open bundle, opening (or reopening) it if the active cycle has
    /// changed under us. Re-checking the marker per call is one small file
    /// read, and it is what lets a sync finishing in the background become
    /// visible to the next query without restarting the app.
    fn connection<'a>(&self, state: &'a mut State) -> Result<&'a Connection, CoreError> {
        let cycle_id = self.require_cycle()?;
        let stale = !matches!(&state.db, Some((open_id, _)) if *open_id == cycle_id);
        if stale {
            let path = self.layout.bundle_path(&cycle_id);
            // Through `ff-storage` rather than rusqlite directly, so the
            // client-local tables (§6's aircraft/route/track tables, below
            // the bundle marker) exist even in a bundle published before
            // the migration that added them.
            let conn = ff_storage::open(&path.display().to_string())
                .map_err(|e| CoreError::Database(e.to_string()))?;
            state.db = Some((cycle_id, conn));
        }
        Ok(&state.db.as_ref().expect("opened just above").1)
    }

    /// Charts this cycle catalogues whose archive is on disk. Derived from
    /// the catalogue rather than from the blob directory, because a blob is
    /// just a hash — only the catalogue knows which chart it is.
    fn installed_chart_ids(&self, conn: &Connection) -> Vec<String> {
        let Ok(charts) = query::charts(conn) else {
            return Vec::new();
        };
        let mut ids: Vec<String> = charts
            .into_iter()
            .filter(|chart| {
                query::chart_hash(conn, &chart.id)
                    .ok()
                    .and_then(|(hash, _)| self.layout.chart_blob_path(&hash))
                    .map(|path| path.is_file())
                    .unwrap_or(false)
            })
            .map(|chart| chart.id)
            .collect();
        ids.sort();
        ids
    }
}

// ---- shared planning math (mirrors `ff-wasm`'s surface) ------------------

#[uniffi::export]
pub fn distance_nm(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    core_distance_nm((lat1, lon1), (lat2, lon2))
}

#[uniffi::export]
pub fn initial_bearing_deg(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    core_initial_bearing_deg((lat1, lon1), (lat2, lon2))
}

/// Plan a route from JSON, same wire shape as `ff-wasm::plan_route_json`:
/// `points` is a JSON array of `{"lat":.., "lon":..}`, `profile` is a JSON
/// `AircraftProfile`, `winds` is a JSON array of `{"direction_true_deg":..,
/// "speed_kt":..} | null` with one entry per leg (`points.len() - 1`) — a
/// `null` entry means no wind data for that leg (heading = course,
/// groundspeed = TAS). `decimal_year` (e.g. 2026.5) dates the WMM magnetic
/// model for the magnetic course/heading columns. Returns a JSON
/// `RoutePlanSummary`.
#[uniffi::export]
pub fn plan_route_json(
    points_json: String,
    profile_json: String,
    winds_json: String,
    decimal_year: f64,
) -> Result<String, CoreError> {
    let points: Vec<RoutePoint> =
        serde_json::from_str(&points_json).map_err(|e| CoreError::InvalidPoints(e.to_string()))?;
    let profile: AircraftProfile = serde_json::from_str(&profile_json)
        .map_err(|e| CoreError::InvalidProfile(e.to_string()))?;
    let winds: Vec<Option<Wind>> =
        serde_json::from_str(&winds_json).map_err(|e| CoreError::InvalidWinds(e.to_string()))?;
    let summary = plan_route(&points, &profile, Some(&winds), decimal_year);
    serde_json::to_string(&summary).map_err(|e| CoreError::Serialize(e.to_string()))
}

// ---- postflight analysis & export (mirrors ff-postflight surface) ---------

/// Analyze a recorded GPS track from JSON.
/// `track_points_json` is a JSON array of `{"ts": "2026-09-16T10:00:00Z", "lat": .., "lon": .., "alt_ft": ..}`.
/// Returns a JSON string of `AnalyzedTrack`.
#[uniffi::export]
pub fn analyze_track_json(track_points_json: String) -> Result<String, CoreError> {
    let points: Vec<ff_postflight::TrackPoint> = serde_json::from_str(&track_points_json)
        .map_err(|e| CoreError::InvalidTrack(e.to_string()))?;
    let analyzed = ff_postflight::analyze_track(&points);
    serde_json::to_string(&analyzed).map_err(|e| CoreError::Serialize(e.to_string()))
}

/// Export a track JSON array to GPX 1.1 XML string.
#[uniffi::export]
pub fn export_track_gpx(track_points_json: String, flight_name: String) -> Result<String, CoreError> {
    let points: Vec<ff_postflight::TrackPoint> = serde_json::from_str(&track_points_json)
        .map_err(|e| CoreError::InvalidTrack(e.to_string()))?;
    Ok(ff_postflight::export_gpx(&points, &flight_name))
}

/// Export an AnalyzedTrack JSON to CSV logbook string.
#[uniffi::export]
pub fn export_flight_csv(analyzed_track_json: String, flight_name: String) -> Result<String, CoreError> {
    let analyzed: ff_postflight::AnalyzedTrack = serde_json::from_str(&analyzed_track_json)
        .map_err(|e| CoreError::InvalidTrack(e.to_string()))?;
    Ok(ff_postflight::export_csv(&analyzed, &flight_name))
}

#[cfg(test)]
mod tests;
