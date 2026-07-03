use crate::bundle::{add_chart, build_bundle, bundle_airport_bbox, BundleSource, ChartSource};
use crate::chart_prep::crop_sectional_to_bbox;
use crate::fetch::{fetch_cifp, fetch_nasr, fetch_sectional_chart};
use crate::publish::{latest_bundle_path, publish_bundle};
use crate::validate::validate_bundle;
use std::collections::HashSet;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EtlError {
    #[error(transparent)]
    Fetch(#[from] crate::fetch::FetchError),
    #[error(transparent)]
    Bundle(#[from] crate::bundle::BundleError),
    #[error(transparent)]
    ChartPrep(#[from] crate::chart_prep::ChartPrepError),
    #[error(transparent)]
    Validate(#[from] crate::validate::ValidateError),
    #[error(transparent)]
    Publish(#[from] crate::publish::PublishError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Bay Area demo scope (matching the original web demo bundle) —
/// DESIGN.md §13 says "one FAA region, e.g. a single ARTCC" for Phase 0;
/// this reuses the same 5 airports already validated end to end rather
/// than a full ARTCC boundary. Expanding to a real ARTCC selection is a
/// follow-up (see TODO.md).
const REGION_ICAOS: &[&str] = &["KSFO", "KOAK", "KSJC", "KPAO", "KHWD"];

/// The FAA sectional covering the region — like `REGION_ICAOS`, part of
/// the hardcoded region definition until real region selection exists.
const REGION_SECTIONAL: &str = "San_Francisco";

/// Margin (degrees) added around the region's airport bounding box when
/// cropping the sectional, so the chart extends past the outermost
/// airports rather than clipping right at their markers.
const CHART_BBOX_MARGIN_DEG: f64 = 0.35;

/// Runs the DESIGN.md §7 data pipeline end to end: fetch the current
/// CIFP/NASR cycle and sectional chart, parse/tile them into an
/// `ff-storage`-schema SQLite bundle plus a PMTiles chart archive,
/// validate against the previously published cycle, then publish both
/// locally under `FF_ETL_DATA_DIR` (default `data/`) for `ff-api` to
/// serve.
///
/// The chart steps need GDAL's CLI tools (`gdalwarp`, `gdal_translate`,
/// `gdaladdo`) on `PATH` — the same external dependency
/// `ff-charts::geotiff_to_pmtiles` has always had, now exercised by the
/// automated pipeline too.
pub fn run() -> Result<(), EtlError> {
    let data_dir = PathBuf::from(std::env::var("FF_ETL_DATA_DIR").unwrap_or_else(|_| "data".to_string()));
    let workdir = tempfile::tempdir()?;

    tracing::info!("fetching current CIFP cycle from aeronav.faa.gov");
    let cifp = fetch_cifp(workdir.path())?;
    tracing::info!(cycle = %cifp.cycle_date, "fetched CIFP");

    tracing::info!(cycle = %cifp.cycle_date, "fetching matching NASR 28-day subscription from nfdc.faa.gov");
    let nasr_dir = fetch_nasr(workdir.path(), &cifp.cycle_date)?;
    tracing::info!("fetched NASR");

    let icaos: HashSet<String> = REGION_ICAOS.iter().map(|s| s.to_string()).collect();
    let bundle_path = workdir.path().join("cycle.sqlite");
    let stats = build_bundle(
        &BundleSource {
            cifp_path: cifp.cifp_path,
            nasr_dir: Some(nasr_dir),
            // The chart is added after the build: its crop bbox comes
            // from the built bundle's own airports (below).
            chart: None,
            icaos,
        },
        &bundle_path,
    )?;
    tracing::info!(?stats, "built cycle bundle");

    tracing::info!(sectional = REGION_SECTIONAL, "fetching sectional chart");
    let chart_tif = fetch_sectional_chart(workdir.path(), REGION_SECTIONAL)?;
    let (min_lat, min_lon, max_lat, max_lon) = bundle_airport_bbox(&bundle_path)?;
    let bbox = (
        min_lat - CHART_BBOX_MARGIN_DEG,
        min_lon - CHART_BBOX_MARGIN_DEG,
        max_lat + CHART_BBOX_MARGIN_DEG,
        max_lon + CHART_BBOX_MARGIN_DEG,
    );
    tracing::info!(?bbox, "cropping sectional to region bbox");
    let cropped = crop_sectional_to_bbox(&chart_tif, workdir.path(), bbox)?;

    let pmtiles_path = workdir.path().join("chart.pmtiles");
    add_chart(
        &bundle_path,
        &ChartSource {
            geotiff_path: cropped,
            pmtiles_out: pmtiles_path.clone(),
            cycle_id: cifp.cycle_date.clone(),
            name: format!("{} Sectional", REGION_SECTIONAL.replace('_', " ")),
            // Where ff-api serves published chart files (see
            // ff-api's /bundles route and publish.rs's layout).
            tile_url: format!("/bundles/{}/chart.pmtiles", cifp.cycle_date),
        },
    )?;
    tracing::info!("tiled sectional into PMTiles and added chart_catalog entry");

    let previous_bundle_path = latest_bundle_path(&data_dir)?;
    validate_bundle(&bundle_path, &stats, previous_bundle_path.as_deref())?;
    tracing::info!("validated cycle bundle");

    let published_path = publish_bundle(&bundle_path, Some(&pmtiles_path), &cifp.cycle_date, &data_dir)?;
    tracing::info!(path = %published_path.display(), "published cycle bundle + chart");

    Ok(())
}
