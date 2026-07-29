use crate::airspace::{fetch_class_airspace, fetch_special_use_airspace};
use crate::bundle::{
    add_airspace, add_aixm, add_chart, add_dtpp_charts, add_openaip, build_bundle, BundleSource,
    ChartSource,
};
use crate::chart_prep::{crop_legend_and_collar, crop_to_neatline, expand_palette_to_rgb};
use crate::dtpp::{discover_dtpp_cycle, fetch_and_match_dtpp_charts};
use crate::fetch::{
    discover_chart_cycle, discover_ifr_enroute_cycle, fetch_cifp, fetch_ifr_enroute_panel,
    fetch_nasr, fetch_sectional_chart, fetch_terminal_chart_zip, ChartCycle, IfrEnrouteCycle,
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
    Aixm(#[from] crate::aixm::AixmLoadError),
    #[error(transparent)]
    OpenAip(#[from] crate::openaip::OpenAipLoadError),
    #[error("AIXM effective date {sia} does not match the CIFP cycle {cifp}; use the matching-AIRAC SIA export, or set FF_AIXM_ALLOW_CYCLE_MISMATCH=1 to build anyway")]
    AixmCycleMismatch { sia: String, cifp: String },
    #[error(transparent)]
    Dtpp(#[from] crate::dtpp::DtppError),
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
    let mut stats = build_bundle(
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

    // Non-US data from a national AIXM export (DESIGN.md §3.1), first
    // target France/SIA. Gated on FF_AIXM_FR_PATH (a locally-downloaded
    // SIA export — the file is cart-gated, not fetchable unattended);
    // unset means a US-only cycle, unaffected. Best-effort: a bad/absent
    // non-US file must not sink the whole US cycle, so a load error is
    // logged and skipped (same policy as the d-TPP step below).
    //
    // ATTRIBUTION: SIA data is Licence Ouverte — a published cycle that
    // includes it must display "Service de l'Information Aéronautique
    // (SIA)" + the export's effective date. `add_aixm` records that in the
    // `data_source` table; the web About page renders it (§3.1).
    if let Some(aixm_path) = crate::aixm::configured_source() {
        match crate::aixm::load(&aixm_path) {
            Ok(data) => {
                // Guardrail: the SIA export must be the same AIRAC cycle as
                // the FAA data, or the bundle would carry stale non-US data
                // under the FAA cycle id (a silent, easy mistake). Refuse
                // unless explicitly overridden. A file without an effective
                // date can't be checked, so it's allowed through.
                if let Some(eff) = data.effective.as_deref() {
                    if eff != cifp.cycle_date {
                        if std::env::var("FF_AIXM_ALLOW_CYCLE_MISMATCH").is_ok() {
                            tracing::warn!(sia_effective = %eff, cifp_cycle = %cifp.cycle_date, "AIXM effective date != CIFP cycle — proceeding because FF_AIXM_ALLOW_CYCLE_MISMATCH is set");
                        } else {
                            return Err(EtlError::AixmCycleMismatch {
                                sia: eff.to_string(),
                                cifp: cifp.cycle_date.clone(),
                            });
                        }
                    }
                }
                let added = add_aixm(&bundle_path, &data)?;
                // Airspace volumes go through the same inserter the FAA
                // airspace uses (bbox-indexed).
                add_airspace(&bundle_path, &data.airspaces)?;
                // Fold the non-US airport count into `stats` so validation's
                // cycle-to-cycle airport-count check compares like with like.
                stats.airports += added.airports;
                tracing::info!(
                    source = %aixm_path.display(),
                    airports = added.airports,
                    runways = added.runways,
                    navaids = added.navaids,
                    waypoints = added.waypoints,
                    airways = added.airways,
                    airway_legs = added.airway_legs,
                    airspaces = data.airspaces.len(),
                    "added France/SIA AIXM data to bundle (Licence Ouverte — attribution required in clients)"
                );
            }
            Err(err) => {
                tracing::warn!(error = %err, source = %aixm_path.display(), "couldn't load AIXM data this cycle — skipping")
            }
        }
    }

    // The openAIP fallback tier (DESIGN.md §3.1.2): states whose official
    // AIS may not be re-hosted (Germany, UK, Canada) or publishes no
    // dataset at all (Greenland). Runs *after* the AIXM step so that
    // `INSERT OR IGNORE` gives official data priority on any ICAO both
    // could supply — the tier rule holding at the database level, not
    // just in configuration. Gated on FF_OPENAIP_API_KEY; unset leaves
    // the cycle exactly as it was.
    //
    // ATTRIBUTION: bundling openAIP obliges crediting it in the clients,
    // alongside the SIA attribution above.
    if let Some(api_key) = crate::openaip::configured_key() {
        // A tier conflict is a config error that would corrupt the
        // bundle, so it propagates; individual state fetch failures are
        // logged and skipped inside `load_configured`.
        let loaded = crate::openaip::load_configured(&api_key)?;
        for s in &loaded.per_state {
            tracing::info!(
                country = %s.country,
                region = %s.region,
                airports = s.airports,
                navaids = s.navaids,
                airspaces = s.airspaces,
                skipped_airspaces = s.skipped_airspaces,
                "openAIP state loaded"
            );
        }
        let added = add_openaip(&bundle_path, &loaded.airports, &loaded.navaids, &cifp.cycle_date)?;
        add_airspace(&bundle_path, &loaded.airspaces)?;
        // Fold into `stats` so validation's cycle-to-cycle airport-count
        // check compares like with like, exactly as the AIXM step does.
        stats.airports += added.airports;
        tracing::info!(
            states = loaded.per_state.len(),
            airports = added.airports,
            navaids = added.navaids,
            airspaces = loaded.airspaces.len(),
            "added openAIP data to bundle (attribution required in clients)"
        );
    }

    // d-TPP SID/STAR/Approach chart links — best-effort, matching every
    // procedure that could be, not something the whole cycle publish
    // should fail over: a hiccup fetching/matching FAA's ~16MB metafile
    // (or its ~95% approach-match rate simply missing a few) still
    // leaves every other feature (procedures, airspace, charts) intact.
    match discover_dtpp_cycle().and_then(|cycle| {
        tracing::info!(cycle = %cycle, "fetching current d-TPP chart metadata");
        fetch_and_match_dtpp_charts(&bundle_path, &cycle)
    }) {
        Ok(dtpp_charts) => {
            add_dtpp_charts(&bundle_path, &dtpp_charts)?;
            tracing::info!(count = dtpp_charts.len(), "added d-TPP chart links");
        }
        Err(err) => tracing::warn!(error = %err, "couldn't add d-TPP chart links this cycle"),
    }

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
    // loop above but cropped by neatline detection rather than the
    // white-band heuristic: unlike sectionals (colored terrain body,
    // white legend/collar), these charts' *body* is itself mostly white
    // background with just line symbology, so the white-fraction crop
    // heuristic can't tell chart content from legend — confirmed by
    // running it against a real downloaded panel, where it kept only
    // 4.5% of the image. crop_to_neatline finds the black frame around
    // the body instead, and degrades to tiling the panel uncropped when
    // no frame is detected. Expansion runs before cropping since
    // neatline detection needs real RGB (see crop_to_neatline's docs);
    // these GeoTIFFs are already 3-band RGB (confirmed via gdalinfo on
    // real samples) rather than palette-indexed like sectionals —
    // expand_palette_to_rgb detects this and skips the conversion
    // (calling `gdal_translate -expand rgb` on an already-RGB source
    // doesn't just do nothing, it errors: caught live while validating
    // this against a real downloaded panel). Still called for one shared
    // code path in case some panel (e.g. a future Caribbean/oceanic
    // addition) is palette-indexed after all.
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
                let rgb_tif = crop_to_neatline(&rgb_tif, chart_workdir.path(), kind, &part.label)?
                    .unwrap_or(rgb_tif);

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

    // Terminal Area Charts — whose zips also carry each city's VFR Flyway
    // planning chart — plus Helicopter route charts, cropped by neatline
    // detection like the IFR panels above: crop_legend_and_collar's
    // white-band heuristic can't read these layouts (a TAC's legend side
    // carries colored inset panels that stop the white scan early), but
    // they all frame the georeferenced body in a black neatline. Degrades
    // to tiling uncropped when no frame is detected. A missing
    // tac-files/Heli_files listing leaves these lists empty (see
    // discover_chart_cycle), so this simply does nothing rather than
    // failing the run.
    for (subdir, names) in [
        ("tac-files", &chart_cycle.tac_names),
        ("Heli_files", &chart_cycle.heli_names),
    ] {
        for name in names {
            tracing::info!(chart = %name, subdir, "fetching terminal/heli chart");
            let chart_workdir = tempfile::tempdir()?;
            let parts = fetch_terminal_chart_zip(chart_workdir.path(), name, subdir, &chart_cycle)?;
            for part in parts {
                let rgb_tif = expand_palette_to_rgb(&part.tif_path, chart_workdir.path())?;
                let rgb_tif =
                    crop_to_neatline(&rgb_tif, chart_workdir.path(), part.kind, &part.label)?
                        .unwrap_or(rgb_tif);
                // Kind-specific slug suffix keeps these from colliding with
                // the same city's sectional (e.g. `chart-los_angeles`) or
                // each other (`-tac`/`-fly`/`-heli`).
                let (slug_suffix, name_suffix) = match part.kind {
                    ChartKind::TerminalAreaChart => ("tac", "TAC"),
                    ChartKind::VfrFlyway => ("fly", "VFR Flyway"),
                    ChartKind::HelicopterRoute => ("heli", "Helicopter"),
                    _ => ("chart", "Chart"),
                };
                let slug = format!("{}-{slug_suffix}", part.label.to_lowercase());
                let pmtiles_filename = format!("chart-{slug}.pmtiles");
                let pmtiles_path = chart_workdir.path().join(&pmtiles_filename);
                add_chart(
                    &bundle_path,
                    &ChartSource {
                        id: format!("{}-{slug}", cifp.cycle_date),
                        geotiff_path: rgb_tif,
                        pmtiles_out: pmtiles_path.clone(),
                        cycle_id: cifp.cycle_date.clone(),
                        name: format!("{} {name_suffix}", part.label.replace('_', " ")),
                        tile_url: format!("/bundles/{}/{pmtiles_filename}", cifp.cycle_date),
                        kind: part.kind,
                    },
                )?;
                tracing::info!(chart = %part.label, kind = ?part.kind, "tiled terminal/heli chart into PMTiles and added chart_catalog entry");

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
