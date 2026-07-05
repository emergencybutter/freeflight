//! Turns FAA GeoTIFF chart releases into offline-servable PMTiles archives.
//!
//! `ff-charts` deliberately does not reimplement raster reprojection/tiling
//! in Rust — that's a solved problem in GDAL. This module shells out to
//! GDAL's CLI tools (`gdalwarp`, `gdal_translate`, `gdaladdo`) to reproject
//! the source GeoTIFF to Web Mercator and slice it into an MBTiles tile
//! set, then hands that off to [`crate::mbtiles::mbtiles_to_pmtiles`] —
//! pure Rust, no extra tool — to repack it as the final PMTiles archive.
use crate::catalog::{BoundingBox, ChartKind};
use crate::mbtiles::{mbtiles_to_pmtiles, MbtilesError};
use std::path::Path;
use std::process::Command;
use thiserror::Error;

/// Overview (reduced-resolution) zoom-out factors passed to `gdaladdo`.
/// These control which lower zoom levels get pre-rendered tiles; GDAL's
/// MBTiles driver only emits tiles for zoom levels that have either the
/// full-resolution image or an overview.
const OVERVIEW_FACTORS: &[&str] = &["2", "4", "8", "16", "32"];

/// Overrides where `geotiff_to_pmtiles` puts its `warped.tif`/
/// `tiles.mbtiles` intermediates — see that function's doc comment for
/// why this deliberately isn't just `TMPDIR`. Defaults to `/tmp`
/// (conventionally tmpfs on Linux); set this if `/tmp` is too small for
/// the largest source panels (e.g. nationwide IFR enroute charts can
/// produce a 700MB+ warped intermediate) or isn't tmpfs on a given host
/// — point it at any non-CoW filesystem (ext4/xfs are fine; just not
/// btrfs/ZFS) with enough free space.
const INGEST_TMPDIR_ENV: &str = "FF_CHARTS_INGEST_TMPDIR";
const DEFAULT_INGEST_TMPDIR: &str = "/tmp";

#[derive(Debug, Error)]
pub enum ChartIngestError {
    #[error("required external tool '{tool}' was not found on PATH: {source}")]
    ToolNotFound {
        tool: &'static str,
        #[source]
        source: std::io::Error,
    },
    #[error("external tool '{tool}' failed (exit code {code:?}): {stderr}")]
    ToolFailed {
        tool: &'static str,
        code: Option<i32>,
        stderr: String,
    },
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Mbtiles(#[from] MbtilesError),
}

/// A GeoTIFF chart release ready to be converted into a tiled, offline
/// servable chart.
#[derive(Debug, Clone)]
pub struct GeoTiffSource {
    pub path: std::path::PathBuf,
    pub kind: ChartKind,
    pub cycle_id: String,
}

fn run_tool(tool: &'static str, args: &[&std::ffi::OsStr]) -> Result<(), ChartIngestError> {
    let output = Command::new(tool)
        .args(args)
        .output()
        .map_err(|source| ChartIngestError::ToolNotFound { tool, source })?;
    if !output.status.success() {
        return Err(ChartIngestError::ToolFailed {
            tool,
            code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    Ok(())
}

/// Reproject + tile a [`GeoTiffSource`] into a PMTiles archive at
/// `output_path`, returning the resulting bounding box for the catalog
/// entry.
///
/// Requires `gdalwarp`, `gdal_translate`, and `gdaladdo` (GDAL's CLI
/// tools, not the Python bindings) to be present on `PATH`.
///
/// `warped.tif`/`tiles.mbtiles` are always created under
/// [`INGEST_TMPDIR_ENV`] (default `/tmp`) rather than through
/// `tempfile::tempdir()`'s usual `TMPDIR`-driven default — deliberately
/// bypassing whatever `TMPDIR` the rest of the pipeline might be using
/// for large downloads. MBTiles is SQLite under the hood, and
/// `gdal_translate`/`gdaladdo` hit it with many small writes; measured
/// live on a CoW filesystem (btrfs) this produced 40+ GB of actual disk
/// I/O (with a large fraction immediately overwritten/cancelled) to
/// produce a ~30MB result — a stuck-for-hours pipeline run traced back
/// to exactly this. `/tmp` is conventionally tmpfs (RAM-backed, no CoW)
/// on Linux, which sidesteps the pathology entirely; `output_path`
/// itself is unaffected and can still point anywhere (including a CoW
/// volume) since writing the final, much smaller PMTiles file is one
/// sequential write, not a problem for CoW.
pub fn geotiff_to_pmtiles(
    source: &GeoTiffSource,
    output_path: &Path,
) -> Result<BoundingBox, ChartIngestError> {
    let tmp_root =
        std::env::var(INGEST_TMPDIR_ENV).unwrap_or_else(|_| DEFAULT_INGEST_TMPDIR.to_string());
    let workdir = tempfile::Builder::new()
        .prefix("ff-charts-ingest-")
        .tempdir_in(&tmp_root)?;
    let warped_path = workdir.path().join("warped.tif");
    let mbtiles_path = workdir.path().join("tiles.mbtiles");

    run_tool(
        "gdalwarp",
        &[
            "-t_srs".as_ref(),
            "EPSG:3857".as_ref(),
            "-dstalpha".as_ref(),
            "-r".as_ref(),
            "bilinear".as_ref(),
            "-overwrite".as_ref(),
            source.path.as_os_str(),
            warped_path.as_os_str(),
        ],
    )?;

    run_tool(
        "gdal_translate",
        &[
            "-of".as_ref(),
            "MBTILES".as_ref(),
            "-co".as_ref(),
            "TILE_FORMAT=PNG".as_ref(),
            warped_path.as_os_str(),
            mbtiles_path.as_os_str(),
        ],
    )?;

    let mut gdaladdo_args: Vec<&std::ffi::OsStr> =
        vec!["-r".as_ref(), "average".as_ref(), mbtiles_path.as_os_str()];
    gdaladdo_args.extend(OVERVIEW_FACTORS.iter().map(std::ffi::OsStr::new));
    run_tool("gdaladdo", &gdaladdo_args)?;

    let bbox = mbtiles_to_pmtiles(&mbtiles_path, output_path)?;
    Ok(bbox)
}
