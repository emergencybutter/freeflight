//! Expands a FAA sectional GeoTIFF's color palette to RGB, producing the
//! input `ff-charts::geotiff_to_pmtiles` expects.
//!
//! Worked out against a real San Francisco sectional: the source GeoTIFF
//! is palette-indexed, and
//! `geotiff_to_pmtiles`'s own later bilinear warp (to Web Mercator) would
//! blend palette *indices* rather than colors if it ran on the raw
//! source — corrupting every color. Expanding to RGB first (nearest by
//! definition — `gdal_translate -expand rgb` is a direct palette lookup,
//! no resampling involved) fixes that.
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

/// Expand `source_tif`'s indexed palette to RGB, writing the result into
/// `workdir`. Returns the RGB GeoTIFF's path, at `source_tif`'s full
/// native extent (nationwide chart coverage tiles each sectional whole,
/// rather than cropping to a region — see pipeline.rs). Requires
/// `gdal_translate` on `PATH` (same GDAL CLI dependency as
/// `ff-charts::geotiff_to_pmtiles`, which runs downstream of this).
pub fn expand_palette_to_rgb(source_tif: &Path, workdir: &Path) -> Result<PathBuf, ChartPrepError> {
    let expanded_rgb = workdir.join("chart_rgb.tif");

    run_tool(
        "gdal_translate",
        &[
            "-expand".as_ref(),
            "rgb".as_ref(),
            source_tif.as_os_str(),
            expanded_rgb.as_os_str(),
        ],
    )?;

    Ok(expanded_rgb)
}
