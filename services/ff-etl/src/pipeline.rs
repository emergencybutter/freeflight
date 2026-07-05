use crate::airspace::{fetch_class_airspace, fetch_special_use_airspace};
use crate::bundle::{add_airspace, add_chart, build_bundle, BundleSource, ChartSource};
use crate::chart_prep::{crop_legend_and_collar, expand_palette_to_rgb};
use crate::fetch::{
    discover_chart_cycle, discover_ifr_enroute_cycle, fetch_cifp, fetch_ifr_enroute_panel,
    fetch_nasr, fetch_sectional_chart, ChartCycle, IfrEnrouteCycle,
};
use crate::publish::{latest_bundle_path, publish_bundle};
use crate::validate::validate_bundle;
use ff_charts::ChartKind;
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
    Airspace(#[from] crate::airspace::AirspaceError),
    #[error(transparent)]
    Validate(#[from] crate::validate::ValidateError),
    #[error(transparent)]
    Publish(#[from] crate::publish::PublishError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Runs the DESIGN.md §7 data pipeline end to end: fetch the current
/// CIFP/NASR cycle, every current FAA sectional and IFR Enroute Low/High
/// Altitude chart, and current Class B/C/D + Special Use Airspace
/// boundaries; parse/tile them into an `ff-storage`-schema SQLite bundle
/// plus one PMTiles archive per chart, validate against the previously
/// published cycle, then publish everything locally under
/// `FF_ETL_DATA_DIR` (default `data/`) for `ff-api` to serve.
///
/// The chart steps need GDAL's CLI tools (`gdal_translate`, `gdalwarp`,
/// `gdaladdo`) on `PATH` — the same external dependency
/// `ff-charts::geotiff_to_pmtiles` has always had, now exercised by the
/// automated pipeline too. Chart coverage is nationwide (every sectional
/// FAA currently publishes — CONUS + Alaska + Hawaii + a few Canadian
/// border charts — plus every CONUS IFR Enroute Low/High panel, all
/// discovered live rather than a hardcoded list), each tiled at its own
/// full native extent rather than cropped to a region: with a nationwide
/// airport/procedure bundle there's no single "region"
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

    tracing::info!("fetching Class B/C/D and Special Use Airspace boundaries");
    let mut airspace_volumes = fetch_class_airspace()?;
    airspace_volumes.extend(fetch_special_use_airspace()?);
    add_airspace(&bundle_path, &airspace_volumes)?;
    tracing::info!(count = airspace_volumes.len(), "added airspace boundaries");

    let chart_cycle: ChartCycle = discover_chart_cycle()?;
    tracing::info!(
        count = chart_cycle.sectional_names.len(),
        "discovered current sectional chart list"
    );

    let mut published_pmtiles = Vec::with_capacity(chart_cycle.sectional_names.len());
    for sectional_name in &chart_cycle.sectional_names {
        tracing::info!(sectional = %sectional_name, "fetching sectional chart");
        let chart_workdir = tempfile::tempdir()?;
        // Usually one part; a handful of sectionals (e.g. Western
        // Aleutian Islands' East/West split) ship more than one
        // separately-georeferenced .tif per zip — see fetch_sectional_chart.
        let parts = fetch_sectional_chart(chart_workdir.path(), sectional_name, &chart_cycle)?;
        for part in parts {
            let cropped_tif = crop_legend_and_collar(&part.tif_path, chart_workdir.path())?;
            let rgb_tif = expand_palette_to_rgb(&cropped_tif, chart_workdir.path())?;

            let slug = part.label.to_lowercase();
            let pmtiles_filename = format!("chart-{slug}.pmtiles");
            let pmtiles_path = chart_workdir.path().join(&pmtiles_filename);
            add_chart(
                &bundle_path,
                &ChartSource {
                    id: format!("{}-{slug}", cifp.cycle_date),
                    geotiff_path: rgb_tif,
                    pmtiles_out: pmtiles_path.clone(),
                    cycle_id: cifp.cycle_date.clone(),
                    name: format!("{} Sectional", part.label.replace('_', " ")),
                    // Where ff-api serves published chart files (see
                    // ff-api's /bundles route and publish.rs's layout).
                    tile_url: format!("/bundles/{}/{pmtiles_filename}", cifp.cycle_date),
                    kind: ChartKind::Sectional,
                },
            )?;
            tracing::info!(sectional = %part.label, "tiled sectional into PMTiles and added chart_catalog entry");

            // Copy out of chart_workdir before it's dropped (and cleaned
            // up) once every part of this sectional is done, so the next
            // sectional doesn't pile its own multi-hundred-MB
            // intermediates on top.
            let published_copy = workdir.path().join(&pmtiles_filename);
            std::fs::copy(&pmtiles_path, &published_copy)?;
            published_pmtiles.push((pmtiles_filename, published_copy));
        }
    }

    // IFR Enroute Low/High Altitude panels, same shape as the sectional
    // loop above but no legend crop: unlike sectionals (colored terrain
    // body, white legend/collar), these charts' *body* is itself mostly
    // white background with just line symbology, so the white-fraction
    // crop heuristic can't tell chart content from legend — confirmed by
    // running it against a real downloaded panel, where it kept only
    // 4.5% of the image. Tiling the panel uncropped leaves a persistent
    // legend column visible on one edge, a cosmetic compromise rather
    // than a correctness bug (real paper IFR charts have the same
    // margin). These GeoTIFFs are already 3-band RGB (confirmed via
    // gdalinfo on real samples) rather than palette-indexed like
    // sectionals — expand_palette_to_rgb detects this and skips the
    // conversion (calling `gdal_translate -expand rgb` on an
    // already-RGB source doesn't just do nothing, it errors: caught
    // live while validating this against a real downloaded panel).
    // Still called for one shared code path in case some panel (e.g. a
    // future Caribbean/oceanic addition) is palette-indexed after all.
    let ifr_cycle: IfrEnrouteCycle = discover_ifr_enroute_cycle()?;
    tracing::info!(
        low = ifr_cycle.low_panel_names.len(),
        high = ifr_cycle.high_panel_names.len(),
        "discovered current IFR enroute chart panel list"
    );
    let ifr_series: [(&[String], ChartKind, &str); 2] = [
        (
            &ifr_cycle.low_panel_names,
            ChartKind::IfrEnrouteLow,
            "IFR Low Altitude Enroute",
        ),
        (
            &ifr_cycle.high_panel_names,
            ChartKind::IfrEnrouteHigh,
            "IFR High Altitude Enroute",
        ),
    ];
    for (panel_names, kind, series_label) in ifr_series {
        for panel_name in panel_names {
            tracing::info!(panel = %panel_name, "fetching IFR enroute chart panel");
            let chart_workdir = tempfile::tempdir()?;
            let parts = fetch_ifr_enroute_panel(chart_workdir.path(), panel_name, &ifr_cycle)?;
            for part in parts {
                let rgb_tif = expand_palette_to_rgb(&part.tif_path, chart_workdir.path())?;

                let slug = part.label.to_lowercase();
                let pmtiles_filename = format!("chart-{slug}.pmtiles");
                let pmtiles_path = chart_workdir.path().join(&pmtiles_filename);
                add_chart(
                    &bundle_path,
                    &ChartSource {
                        id: format!("{}-{slug}", cifp.cycle_date),
                        geotiff_path: rgb_tif,
                        pmtiles_out: pmtiles_path.clone(),
                        cycle_id: cifp.cycle_date.clone(),
                        name: format!("{series_label} {panel_name}"),
                        tile_url: format!("/bundles/{}/{pmtiles_filename}", cifp.cycle_date),
                        kind,
                    },
                )?;
                tracing::info!(panel = %part.label, "tiled IFR enroute panel into PMTiles and added chart_catalog entry");

                let published_copy = workdir.path().join(&pmtiles_filename);
                std::fs::copy(&pmtiles_path, &published_copy)?;
                published_pmtiles.push((pmtiles_filename, published_copy));
            }
        }
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
