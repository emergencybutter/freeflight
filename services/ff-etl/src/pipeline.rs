use crate::bundle::{add_chart, build_bundle, BundleSource, ChartSource};
use crate::chart_prep::expand_palette_to_rgb;
use crate::fetch::{
    discover_chart_cycle, fetch_cifp, fetch_nasr, fetch_sectional_chart, ChartCycle,
};
use crate::publish::{latest_bundle_path, publish_bundle};
use crate::validate::validate_bundle;
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

/// Runs the DESIGN.md §7 data pipeline end to end: fetch the current
/// CIFP/NASR cycle plus every current FAA sectional chart, parse/tile
/// them into an `ff-storage`-schema SQLite bundle plus one PMTiles
/// archive per sectional, validate against the previously published
/// cycle, then publish everything locally under `FF_ETL_DATA_DIR`
/// (default `data/`) for `ff-api` to serve.
///
/// The chart steps need GDAL's CLI tools (`gdal_translate`, `gdalwarp`,
/// `gdaladdo`) on `PATH` — the same external dependency
/// `ff-charts::geotiff_to_pmtiles` has always had, now exercised by the
/// automated pipeline too. Chart coverage is nationwide (every sectional
/// FAA currently publishes — CONUS + Alaska + Hawaii + a few Canadian
/// border charts — discovered live rather than a hardcoded list), each
/// tiled at its own full native extent rather than cropped to a region:
/// with a nationwide airport/procedure bundle there's no single "region"
/// left to crop to.
pub fn run() -> Result<(), EtlError> {
    let data_dir =
        PathBuf::from(std::env::var("FF_ETL_DATA_DIR").unwrap_or_else(|_| "data".to_string()));
    let workdir = tempfile::tempdir()?;

    tracing::info!("fetching current CIFP cycle from aeronav.faa.gov");
    let cifp = fetch_cifp(workdir.path())?;
    tracing::info!(cycle = %cifp.cycle_date, "fetched CIFP");

    tracing::info!(cycle = %cifp.cycle_date, "fetching matching NASR 28-day subscription from nfdc.faa.gov");
    let nasr_dir = fetch_nasr(workdir.path(), &cifp.cycle_date)?;
    tracing::info!("fetched NASR");

    let bundle_path = workdir.path().join("cycle.sqlite");
    let stats = build_bundle(
        &BundleSource {
            cifp_path: cifp.cifp_path,
            nasr_dir: Some(nasr_dir),
            // Charts are added after the build, one per sectional (below).
            chart: None,
            // Nationwide — no ICAO filter.
            icaos: None,
        },
        &bundle_path,
    )?;
    tracing::info!(?stats, "built cycle bundle");

    let chart_cycle: ChartCycle = discover_chart_cycle()?;
    tracing::info!(
        count = chart_cycle.sectional_names.len(),
        "discovered current sectional chart list"
    );

    let mut published_pmtiles = Vec::with_capacity(chart_cycle.sectional_names.len());
    for sectional_name in &chart_cycle.sectional_names {
        tracing::info!(sectional = %sectional_name, "fetching sectional chart");
        let chart_workdir = tempfile::tempdir()?;
        let chart_tif = fetch_sectional_chart(chart_workdir.path(), sectional_name, &chart_cycle)?;
        let rgb_tif = expand_palette_to_rgb(&chart_tif, chart_workdir.path())?;

        let slug = sectional_name.to_lowercase();
        let pmtiles_filename = format!("chart-{slug}.pmtiles");
        let pmtiles_path = chart_workdir.path().join(&pmtiles_filename);
        add_chart(
            &bundle_path,
            &ChartSource {
                id: format!("{}-{slug}", cifp.cycle_date),
                geotiff_path: rgb_tif,
                pmtiles_out: pmtiles_path.clone(),
                cycle_id: cifp.cycle_date.clone(),
                name: format!("{} Sectional", sectional_name.replace('_', " ")),
                // Where ff-api serves published chart files (see
                // ff-api's /bundles route and publish.rs's layout).
                tile_url: format!("/bundles/{}/{pmtiles_filename}", cifp.cycle_date),
            },
        )?;
        tracing::info!(sectional = %sectional_name, "tiled sectional into PMTiles and added chart_catalog entry");

        // Copy out of chart_workdir before it's dropped (and cleaned up)
        // at the end of this loop iteration, so the next chart doesn't
        // pile its own multi-hundred-MB intermediates on top.
        let published_copy = workdir.path().join(&pmtiles_filename);
        std::fs::copy(&pmtiles_path, &published_copy)?;
        published_pmtiles.push((pmtiles_filename, published_copy));
    }

    let previous_bundle_path = latest_bundle_path(&data_dir)?;
    validate_bundle(&bundle_path, &stats, previous_bundle_path.as_deref())?;
    tracing::info!("validated cycle bundle");

    let published_path = publish_bundle(
        &bundle_path,
        &published_pmtiles,
        &cifp.cycle_date,
        &data_dir,
    )?;
    tracing::info!(path = %published_path.display(), charts = published_pmtiles.len(), "published cycle bundle + charts");

    Ok(())
}
