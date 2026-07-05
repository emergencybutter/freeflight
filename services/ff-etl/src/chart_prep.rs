//! Expands a FAA sectional GeoTIFF's color palette to RGB, and crops out
//! the legend/collar margin FAA bakes into the same raster, producing
//! the input `ff-charts::geotiff_to_pmtiles` expects.
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
    #[error("failed to parse '{tool}' JSON output: {source}")]
    BadJson {
        tool: &'static str,
        #[source]
        source: serde_json::Error,
    },
    #[error("failed to decode crop-detection preview image: {0}")]
    Image(#[from] image::ImageError),
}

fn run_tool(tool: &'static str, args: &[&std::ffi::OsStr]) -> Result<(), ChartPrepError> {
    run_tool_capturing(tool, args).map(|_| ())
}

fn run_tool_capturing(
    tool: &'static str,
    args: &[&std::ffi::OsStr],
) -> Result<Vec<u8>, ChartPrepError> {
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
    Ok(output.stdout)
}

/// `true` if `source_tif`'s first band is palette-indexed (`gdalinfo`'s
/// `colorInterpretation` reports `"Palette"`), as opposed to already
/// being plain RGB. Confirmed against two real chart families this was
/// built against: FAA sectionals are palette-indexed; IFR Enroute
/// Low/High panels are already 3-band RGB. This distinction matters
/// because `gdal_translate -expand rgb` isn't a harmless no-op on an
/// already-RGB source the way its name might suggest — it errors
/// (`"band 1 has no color table"`), confirmed live while validating the
/// IFR enroute pipeline against a real downloaded panel.
fn is_palette_indexed(source_tif: &Path) -> Result<bool, ChartPrepError> {
    let stdout = run_tool_capturing(
        "gdalinfo",
        &["-json".as_ref(), "-nomd".as_ref(), source_tif.as_os_str()],
    )?;
    let info: serde_json::Value =
        serde_json::from_slice(&stdout).map_err(|source| ChartPrepError::BadJson {
            tool: "gdalinfo",
            source,
        })?;
    let bands = info["bands"]
        .as_array()
        .expect("gdalinfo -json always includes a bands array");
    Ok(bands
        .first()
        .and_then(|b| b["colorInterpretation"].as_str())
        == Some("Palette"))
}

/// Expand `source_tif`'s indexed palette to RGB, writing the result into
/// `workdir` — a no-op that returns `source_tif` unchanged if it's
/// already RGB (see [`is_palette_indexed`]). Returns the RGB GeoTIFF's
/// path, at `source_tif`'s full native extent (nationwide chart coverage
/// tiles each chart whole, rather than cropping to a region — see
/// pipeline.rs). Requires `gdal_translate`/`gdalinfo` on `PATH` (same
/// GDAL CLI dependency as `ff-charts::geotiff_to_pmtiles`, which runs
/// downstream of this).
pub fn expand_palette_to_rgb(source_tif: &Path, workdir: &Path) -> Result<PathBuf, ChartPrepError> {
    if !is_palette_indexed(source_tif)? {
        return Ok(source_tif.to_path_buf());
    }

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

/// Fraction of a preview image's pixels, per row/column, that are near-
/// white (all channels > 235) — the discriminator this module uses to
/// tell the legend/collar's paper-white background apart from the
/// chart body, which is almost entirely covered in terrain color.
fn white_fractions(rgb: &image::RgbImage) -> (Vec<f64>, Vec<f64>) {
    let (w, h) = rgb.dimensions();
    let is_white = |x: u32, y: u32| {
        let p = rgb.get_pixel(x, y);
        p[0] > 235 && p[1] > 235 && p[2] > 235
    };
    let col_frac = (0..w)
        .map(|x| (0..h).filter(|&y| is_white(x, y)).count() as f64 / h as f64)
        .collect();
    let row_frac = (0..h)
        .map(|y| (0..w).filter(|&x| is_white(x, y)).count() as f64 / w as f64)
        .collect();
    (col_frac, row_frac)
}

/// Fraction of `fracs`' length before real chart content reliably
/// starts, scanning from the front: the first index where at least
/// `RUN` consecutive entries all stay under `WHITE_THRESHOLD`. `0.0` if
/// content starts immediately (no legend/collar on this edge).
const WHITE_THRESHOLD: f64 = 0.3;
const RUN: usize = 6;

fn edge_cut_fraction(fracs: &[f64]) -> f64 {
    let len = fracs.len();
    for i in 0..len {
        let window_end = (i + RUN).min(len);
        if fracs[i..window_end].iter().all(|&f| f < WHITE_THRESHOLD) {
            return i as f64 / len as f64;
        }
    }
    0.0
}

fn source_dimensions(source_tif: &Path) -> Result<(u32, u32), ChartPrepError> {
    let stdout = run_tool_capturing(
        "gdalinfo",
        &["-json".as_ref(), "-nomd".as_ref(), source_tif.as_os_str()],
    )?;
    let info: serde_json::Value =
        serde_json::from_slice(&stdout).map_err(|source| ChartPrepError::BadJson {
            tool: "gdalinfo",
            source,
        })?;
    let size = info["size"]
        .as_array()
        .expect("gdalinfo -json always includes a 2-element size array");
    let width = size[0].as_u64().unwrap() as u32;
    let height = size[1].as_u64().unwrap() as u32;
    Ok((width, height))
}

/// Crops out the legend/collar margin FAA bakes into the same raster as
/// the actual chart imagery. Confirmed against FAA's own product
/// metadata (each sectional's accompanying `.htm` metadata file): "The
/// image inside the neat line is georeferenced to the surface of the
/// earth. Only the main body of the chart is accurately georeferenced,"
/// while "the area of coverage... includ[es] the chart border
/// (tabulations, legend, notes, etc.)" — i.e. the whole raster shares
/// one geotransform, so left uncropped, the legend/collar gets warped
/// and tiled as if it were real chart imagery, appearing as garbled
/// content at a bogus location next to the real chart once nationwide
/// full-extent tiling (see pipeline.rs) stopped incidentally cropping
/// it away the way the old region-bbox crop used to.
///
/// Detected per chart rather than assumed at a fixed position/size —
/// confirmed live that legend placement/size varies (a real Wichita
/// sectional has a left-column legend plus a bottom margin; a real
/// Western Aleutian Islands sectional has next to none). Renders a
/// small preview, scans each of the 4 edges inward for a mostly-white
/// band (the legend/collar's paper background — the chart body itself
/// is almost entirely covered in terrain color, confirmed against both
/// real charts above), and crops the full-resolution source to what's
/// left. A chart with no legend on a given edge degenerates to cutting
/// nothing there.
pub fn crop_legend_and_collar(
    source_tif: &Path,
    workdir: &Path,
) -> Result<PathBuf, ChartPrepError> {
    let preview_path = workdir.join("crop_preview.png");
    run_tool(
        "gdal_translate",
        &[
            "-outsize".as_ref(),
            "5%".as_ref(),
            "5%".as_ref(),
            "-of".as_ref(),
            "PNG".as_ref(),
            source_tif.as_os_str(),
            preview_path.as_os_str(),
        ],
    )?;

    let preview = image::open(&preview_path)?.into_rgb8();
    let (col_frac, row_frac) = white_fractions(&preview);
    let left_frac = edge_cut_fraction(&col_frac);
    let right_frac = edge_cut_fraction(&col_frac.iter().rev().copied().collect::<Vec<_>>());
    let top_frac = edge_cut_fraction(&row_frac);
    let bottom_frac = edge_cut_fraction(&row_frac.iter().rev().copied().collect::<Vec<_>>());

    let (full_w, full_h) = source_dimensions(source_tif)?;
    let left = (left_frac * full_w as f64).round() as u32;
    let right = full_w - (right_frac * full_w as f64).round() as u32;
    let top = (top_frac * full_h as f64).round() as u32;
    let bottom = full_h - (bottom_frac * full_h as f64).round() as u32;
    tracing::debug!(
        left,
        top,
        right,
        bottom,
        full_w,
        full_h,
        "cropping legend/collar"
    );

    let cropped = workdir.join("chart_cropped.tif");
    run_tool(
        "gdal_translate",
        &[
            "-srcwin".as_ref(),
            left.to_string().as_ref(),
            top.to_string().as_ref(),
            (right - left).to_string().as_ref(),
            (bottom - top).to_string().as_ref(),
            source_tif.as_os_str(),
            cropped.as_os_str(),
        ],
    )?;

    Ok(cropped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_no_cut_when_content_starts_immediately() {
        assert_eq!(edge_cut_fraction(&[0.05; 20]), 0.0);
    }

    #[test]
    fn finds_the_cut_past_a_white_legend_band() {
        let mut fracs = vec![0.9; 10];
        fracs.extend(vec![0.05; 20]);
        assert_eq!(edge_cut_fraction(&fracs), 10.0 / 30.0);
    }

    #[test]
    fn ignores_a_brief_light_patch_that_does_not_hold_for_a_full_run() {
        // A single bright column/row (e.g. sun glare on water, a thin
        // white road) shouldn't be mistaken for the start of real chart
        // content if it's not sustained for RUN consecutive entries.
        let mut fracs = vec![0.9; 5];
        fracs.push(0.1); // one bright-adjacent dip, shorter than RUN
        fracs.extend(vec![0.9; 4]);
        fracs.extend(vec![0.05; 20]);
        let cut = edge_cut_fraction(&fracs);
        assert_eq!(cut, 10.0 / 30.0);
    }
}
