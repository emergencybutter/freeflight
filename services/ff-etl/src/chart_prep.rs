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

/// A pixel is "dark" for neatline detection if even its brightest channel
/// is dim — colored chart body (yellow terrain, blue water) keeps a bright
/// max channel, while the black frame line stays dark even after `-r
/// average` downsampling blurs it against white paper.
const NEATLINE_DARK_MAX_CHANNEL: u8 = 160;
/// Looser darkness for the row-detection retry: a real Los Angeles VFR
/// Flyway chart draws its *top* frame line thin enough to blur to light
/// gray (its sides and bottom stay black), so it never reads as dark at
/// the strict threshold. Only used in the fallback path — see
/// [`detect_neatline`].
const NEATLINE_FAINT_MAX_CHANNEL: u8 = 210;
/// A column is a frame candidate if a continuous dark run covers this
/// fraction of the preview height (the neatline's verticals span nearly
/// the whole body; legend text/table rules are far shorter).
const NEATLINE_COL_RUN_FRACTION: f64 = 0.55;
/// Rows are matched within the detected left/right frame columns, and the
/// frame's horizontals span that window end to end — a high threshold
/// rejects scale bars and title rules in the margins, which are long but
/// not *that* long.
const NEATLINE_ROW_RUN_FRACTION: f64 = 0.85;
/// Reject a "detection" whose body would be implausibly small — that's
/// noise, not a chart frame.
const NEATLINE_MIN_BODY_FRACTION: f64 = 0.4;
/// Rows demand a much taller body than the generic bar: every real chart
/// measured (TAC/Flyway/ENR) uses ≥93% of the sheet height for the body,
/// and a lower bar let the strict row pass accept an (interior parallel,
/// bottom frame) pair instead of falling through to the loose retry that
/// finds the real (faint) top line.
const NEATLINE_ROW_MIN_BODY_FRACTION: f64 = 0.8;
/// Preview width for neatline detection. Much wider than the white-band
/// heuristic's 5% preview: the neatline is only a few source pixels
/// thick, and at coarser scales `-r average` washes it out past the dark
/// threshold — measured on a real Los Angeles TAC, whose frame reads
/// cleanly at 4000px but vanishes at 2000px.
const NEATLINE_PREVIEW_WIDTH: u32 = 4000;
/// Candidate line coordinates within this many preview pixels merge into
/// one cluster (a blurred frame line spans a few preview columns).
const NEATLINE_CLUSTER_MERGE_PX: u32 = 5;

/// Longest run of consecutive `true`s from `is_set` over `0..len`.
fn longest_run(len: u32, is_set: impl Fn(u32) -> bool) -> u32 {
    let mut best = 0;
    let mut current = 0;
    for i in 0..len {
        if is_set(i) {
            current += 1;
            best = best.max(current);
        } else {
            current = 0;
        }
    }
    best
}

/// Groups sorted candidate coordinates into `(start, end)` clusters,
/// merging neighbors within [`NEATLINE_CLUSTER_MERGE_PX`].
fn cluster_lines(candidates: &[u32]) -> Vec<(u32, u32)> {
    let mut clusters: Vec<(u32, u32)> = Vec::new();
    for &c in candidates {
        match clusters.last_mut() {
            Some((_, end)) if c <= *end + NEATLINE_CLUSTER_MERGE_PX => *end = c,
            _ => clusters.push((c, c)),
        }
    }
    clusters
}

/// The pair of adjacent clusters with the largest gap between them — the
/// chart body is the widest framed cell, which is what distinguishes the
/// body's frame from the sheet border and the legend panel's own box (a
/// real ENR panel has all three; taking plain extremes grabbed the
/// legend). Returns the crop edges: the left cluster's start and the
/// right cluster's end, so the frame lines themselves stay in the crop.
fn widest_cell(clusters: &[(u32, u32)]) -> Option<(u32, u32)> {
    let mut best: Option<(u32, u32)> = None;
    let mut best_gap = 0;
    for pair in clusters.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let gap = b.0.saturating_sub(a.1);
        if gap >= best_gap {
            best_gap = gap;
            best = Some((a.0, b.1));
        }
    }
    best
}

/// Test-only re-export of [`detect_neatline`] so the ignored
/// `real_neatline_probe` integration test can exercise detection against
/// staged real-chart previews without GDAL.
pub fn detect_neatline_for_probe(rgb: &image::RgbImage) -> Option<(u32, u32, u32, u32)> {
    detect_neatline(rgb)
}

/// Finds the chart's neatline — the black frame around the georeferenced
/// body — in a preview image, as inclusive `(left, top, right, bottom)`
/// pixel coordinates.
///
/// Columns: collect those whose longest continuous dark run spans most of
/// the height (frame verticals), cluster them, and take the widest cell
/// between adjacent clusters (see [`widest_cell`]) — the body, not the
/// legend panel's own box.
///
/// Rows (within those columns): same widest-cell pass first; when it
/// can't find two lines — a real LA Flyway draws its top frame line too
/// faint for the strict threshold — retry at the looser
/// [`NEATLINE_FAINT_MAX_CHANNEL`] taking the outermost clusters instead:
/// interior graticule parallels also match when loosened, but they sit
/// *between* the frame lines, so extremes stay correct, and this path
/// only runs for charts the strict pass already failed.
///
/// `None` when no plausible frame is found.
fn detect_neatline(rgb: &image::RgbImage) -> Option<(u32, u32, u32, u32)> {
    let (w, h) = rgb.dimensions();
    let dark_at = |max_channel: u8| {
        move |x: u32, y: u32| {
            let p = rgb.get_pixel(x, y);
            p[0].max(p[1]).max(p[2]) < max_channel
        }
    };

    let is_dark = dark_at(NEATLINE_DARK_MAX_CHANNEL);
    let col_run_min = (NEATLINE_COL_RUN_FRACTION * h as f64) as u32;
    let frame_cols: Vec<u32> = (0..w)
        .filter(|&x| longest_run(h, |y| is_dark(x, y)) >= col_run_min)
        .collect();
    let (left, right) = widest_cell(&cluster_lines(&frame_cols))?;
    if (right - left) < (NEATLINE_MIN_BODY_FRACTION * w as f64) as u32 {
        return None;
    }

    let row_window = right - left + 1;
    let row_run_min = (NEATLINE_ROW_RUN_FRACTION * row_window as f64) as u32;
    let row_clusters = |max_channel: u8| {
        let is_dark = dark_at(max_channel);
        let rows: Vec<u32> = (0..h)
            .filter(|&y| longest_run(row_window, |i| is_dark(left + i, y)) >= row_run_min)
            .collect();
        cluster_lines(&rows)
    };
    let min_body_h = (NEATLINE_ROW_MIN_BODY_FRACTION * h as f64) as u32;
    let plausible = |&(top, bottom): &(u32, u32)| (bottom - top) >= min_body_h;
    // Rows take cluster extremes, not the widest cell: an interior
    // graticule parallel is itself a frame-spanning dark line, and
    // widest-cell could pair it with one frame edge and crop mid-chart —
    // while nothing in the margins is long enough to pass the row-run
    // bar (a TAC's scale bar measures ~60% of the body width), so the
    // outermost matches are reliably the frame.
    let row_extremes = |max_channel: u8| {
        let clusters = row_clusters(max_channel);
        let (first, last) = (clusters.first()?, clusters.last()?);
        Some((first.0, last.1)).filter(plausible)
    };
    let (top, bottom) = row_extremes(NEATLINE_DARK_MAX_CHANNEL)
        .or_else(|| row_extremes(NEATLINE_FAINT_MAX_CHANNEL))?;

    Some((left, top, right, bottom))
}

/// Crops `source_tif` to its neatline (see [`detect_neatline`]) — the
/// crop strategy for chart styles the white-band heuristic of
/// [`crop_legend_and_collar`] can't read: IFR enroute panels (whose
/// *body* is mostly white, so white-fraction scanning ate the chart —
/// measured keeping only 4.5% of a real panel), and TAC/Flyway/Heli
/// charts (whose legend side carries colored inset panels that stop the
/// white-band scan early). All of them frame the georeferenced body in a
/// continuous black neatline, with the legend/collar outside it —
/// confirmed against real Los Angeles TAC and ENR_L02 rasters.
///
/// Returns `Ok(None)` when no plausible frame is detected (an odd layout,
/// a style change) — callers fall back to tiling uncropped, the previous
/// behavior for these chart kinds.
///
/// `source_tif` must already be RGB (see [`expand_palette_to_rgb`]): the
/// detection preview is downsampled with `-r average`, and averaging
/// palette *indices* corrupts colors the same way the module doc
/// describes for the tiling warp.
pub fn crop_to_neatline(
    source_tif: &Path,
    workdir: &Path,
) -> Result<Option<PathBuf>, ChartPrepError> {
    let preview_path = workdir.join("neatline_preview.png");
    run_tool(
        "gdal_translate",
        &[
            "-outsize".as_ref(),
            NEATLINE_PREVIEW_WIDTH.to_string().as_ref(),
            "0".as_ref(),
            "-r".as_ref(),
            "average".as_ref(),
            "-of".as_ref(),
            "PNG".as_ref(),
            source_tif.as_os_str(),
            preview_path.as_os_str(),
        ],
    )?;

    let preview = image::open(&preview_path)?.into_rgb8();
    let (pw, ph) = preview.dimensions();
    let Some((left, top, right, bottom)) = detect_neatline(&preview) else {
        tracing::warn!(tif = %source_tif.display(), "no neatline detected; tiling uncropped");
        return Ok(None);
    };

    let (full_w, full_h) = source_dimensions(source_tif)?;
    let sx = full_w as f64 / pw as f64;
    let sy = full_h as f64 / ph as f64;
    let src_left = (left as f64 * sx).floor() as u32;
    let src_top = (top as f64 * sy).floor() as u32;
    let src_right = (((right + 1) as f64) * sx).ceil().min(full_w as f64) as u32;
    let src_bottom = (((bottom + 1) as f64) * sy).ceil().min(full_h as f64) as u32;
    tracing::debug!(
        src_left,
        src_top,
        src_right,
        src_bottom,
        full_w,
        full_h,
        "cropping to detected neatline"
    );

    let cropped = workdir.join("chart_neatline_cropped.tif");
    run_tool(
        "gdal_translate",
        &[
            "-srcwin".as_ref(),
            src_left.to_string().as_ref(),
            src_top.to_string().as_ref(),
            (src_right - src_left).to_string().as_ref(),
            (src_bottom - src_top).to_string().as_ref(),
            source_tif.as_os_str(),
            cropped.as_os_str(),
        ],
    )?;
    Ok(Some(cropped))
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

    /// White canvas with a black rectangle frame (the neatline), short
    /// dark "legend text" strokes left of it, and a partial-width "scale
    /// bar" below it — the shapes detection must accept and reject.
    fn synthetic_framed_chart() -> image::RgbImage {
        let (w, h) = (400u32, 300u32);
        let mut img = image::RgbImage::from_pixel(w, h, image::Rgb([255, 255, 255]));
        let black = image::Rgb([0, 0, 0]);
        // Frame: verticals x=40..41 and x=360..361, horizontals y=20..21
        // and y=280..281 (2px thick, like a blurred real neatline).
        for y in 20..=281 {
            for x in [40, 41, 360, 361] {
                img.put_pixel(x, y, black);
            }
        }
        for x in 40..=361 {
            for y in [20, 21, 280, 281] {
                img.put_pixel(x, y, black);
            }
        }
        // "Legend text": short vertical strokes in the left margin — long
        // enough to be visible, far too short to be a frame line.
        for y in 60..100 {
            img.put_pixel(10, y, black);
            img.put_pixel(20, y, black);
        }
        // "Scale bar" in the bottom margin: long but not frame-spanning.
        for x in 100..300 {
            img.put_pixel(x, 295, black);
        }
        img
    }

    #[test]
    fn detects_the_neatline_frame_and_ignores_margin_marks() {
        let img = synthetic_framed_chart();
        assert_eq!(detect_neatline(&img), Some((40, 20, 361, 281)));
    }

    #[test]
    fn detects_nothing_on_a_frameless_image() {
        let img = image::RgbImage::from_pixel(200, 150, image::Rgb([255, 255, 255]));
        assert_eq!(detect_neatline(&img), None);
    }

    #[test]
    fn colored_chart_body_is_not_mistaken_for_frame_lines() {
        // A column of saturated terrain color (bright max channel) must
        // not read as "dark" — only genuinely dark ink counts.
        let mut img = synthetic_framed_chart();
        for y in 22..280 {
            img.put_pixel(200, y, image::Rgb([255, 230, 0])); // yellow
        }
        assert_eq!(detect_neatline(&img), Some((40, 20, 361, 281)));
    }

    #[test]
    fn faint_top_frame_line_is_found_by_the_loose_row_retry() {
        // Real LA Flyway: sides/bottom black, top frame line light gray —
        // the strict row pass sees one line, the loose retry must find
        // both, and an interior black graticule parallel must not shrink
        // the crop (extremes, not widest-cell, on the loose pass).
        let mut img = synthetic_framed_chart();
        let gray = image::Rgb([185, 185, 185]);
        for x in 40..=361 {
            for y in [20, 21] {
                img.put_pixel(x, y, gray); // repaint top line faint
            }
        }
        for x in 42..360 {
            img.put_pixel(x, 150, image::Rgb([0, 0, 0])); // interior parallel
        }
        assert_eq!(detect_neatline(&img), Some((40, 20, 361, 281)));
    }

    #[test]
    fn picks_the_body_frame_not_the_legend_panels_own_box() {
        // Real ENR panels wrap the legend column in its own full-height
        // box, next to the body frame — plain leftmost/rightmost line
        // picking grabbed the legend; the widest-cell rule must not.
        let (w, h) = (400u32, 300u32);
        let mut img = image::RgbImage::from_pixel(w, h, image::Rgb([255, 255, 255]));
        let black = image::Rgb([0, 0, 0]);
        // Legend panel box: x=4..36, full-height verticals at both edges.
        for y in 10..=290 {
            for x in [4, 36] {
                img.put_pixel(x, y, black);
            }
        }
        // Body frame: x=60..380, y=10..290.
        for y in 10..=290 {
            for x in [60, 380] {
                img.put_pixel(x, y, black);
            }
        }
        for x in 60..=380 {
            for y in [10, 290] {
                img.put_pixel(x, y, black);
            }
        }
        assert_eq!(detect_neatline(&img), Some((60, 10, 380, 290)));
    }
}
