//! Crops a full FAA sectional GeoTIFF down to a region bounding box and
//! expands its color palette to RGB, producing the input
//! `ff-charts::geotiff_to_pmtiles` expects.
//!
//! Both steps were worked out against a real San Francisco sectional
//! (see TODO.md's "Chart imagery"): the crop must use nearest-neighbor
//! resampling because the source GeoTIFF is palette-indexed — bilinear
//! would blend palette *indices* and corrupt every color — and the
//! palette must then be expanded to RGB so the pipeline's own later
//! bilinear warp (to Web Mercator) is operating on real color values.
use std::path::{Path, PathBuf};
use std::process::Command;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ChartPrepError {
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
}

fn run_tool(tool: &'static str, args: &[&std::ffi::OsStr]) -> Result<(), ChartPrepError> {
    let output = Command::new(tool)
        .args(args)
        .output()
        .map_err(|source| ChartPrepError::ToolNotFound { tool, source })?;
    if !output.status.success() {
        return Err(ChartPrepError::ToolFailed {
            tool,
            code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    Ok(())
}

/// Crop `source_tif` to the WGS84 bbox (`min_lat, min_lon, max_lat,
/// max_lon`) and expand its palette to RGB, writing intermediates into
/// `workdir`. Returns the cropped RGB GeoTIFF's path. Requires
/// `gdalwarp`/`gdal_translate` on `PATH` (same GDAL CLI dependency as
/// `ff-charts::geotiff_to_pmtiles`, which runs downstream of this).
pub fn crop_sectional_to_bbox(
    source_tif: &Path,
    workdir: &Path,
    bbox: (f64, f64, f64, f64),
) -> Result<PathBuf, ChartPrepError> {
    let (min_lat, min_lon, max_lat, max_lon) = bbox;
    let cropped = workdir.join("chart_cropped.tif");
    let cropped_rgb = workdir.join("chart_cropped_rgb.tif");

    run_tool(
        "gdalwarp",
        &[
            "-t_srs".as_ref(),
            "EPSG:4326".as_ref(),
            "-te".as_ref(),
            format!("{min_lon}").as_ref(),
            format!("{min_lat}").as_ref(),
            format!("{max_lon}").as_ref(),
            format!("{max_lat}").as_ref(),
            "-r".as_ref(),
            "near".as_ref(),
            "-overwrite".as_ref(),
            source_tif.as_os_str(),
            cropped.as_os_str(),
        ],
    )?;

    run_tool(
        "gdal_translate",
        &[
            "-expand".as_ref(),
            "rgb".as_ref(),
            cropped.as_os_str(),
            cropped_rgb.as_os_str(),
        ],
    )?;

    Ok(cropped_rgb)
}
