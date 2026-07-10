//! One-off surgical tiler: fetches FAA Terminal Area Charts (and the VFR
//! Flyway chart bundled in each TAC zip) plus Helicopter route charts, and
//! adds them to an already-published cycle bundle **in place** — the
//! PMTiles into the cycle's directory, one `chart_catalog` row each into
//! its `cycle.sqlite`. This is the `pipeline.rs` terminal/heli loop lifted
//! out so the new chart kinds can be backfilled into an existing cycle
//! without regenerating the whole 19 GB bundle. Charts are cropped to
//! their neatline (frame) like the pipeline does, falling back to
//! uncropped when no frame is detected.
//!
//! Requires GDAL's CLI tools on `PATH` (see `ff-charts::ingest`). Config
//! via env:
//!   FF_CYCLE_SQLITE  path to the cycle's `cycle.sqlite`
//!   FF_CYCLE_DIR     directory to write `chart-*.pmtiles` into
//!   FF_CYCLE_DATE    cycle id, e.g. "2026-07-09"
//!   FF_TILE_IFR      "1" to also re-tile the IFR Enroute Low/High panels
//!                    (e.g. to re-crop ones originally tiled uncropped)

use ff_charts::ChartKind;
use ff_etl::bundle::{add_chart, ChartSource};
use ff_etl::chart_prep::{crop_to_neatline, expand_palette_to_rgb};
use ff_etl::fetch::{
    discover_chart_cycle, discover_ifr_enroute_cycle, fetch_ifr_enroute_panel,
    fetch_terminal_chart_zip,
};
use std::path::{Path, PathBuf};

fn main() {
    tracing_subscriber::fmt::init();
    if let Err(err) = run() {
        tracing::error!("terminal-chart tiling stopped: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let sqlite = std::env::var("FF_CYCLE_SQLITE")?;
    let out_dir = PathBuf::from(std::env::var("FF_CYCLE_DIR")?);
    let cycle_date = std::env::var("FF_CYCLE_DATE")?;
    let tile_ifr = std::env::var("FF_TILE_IFR").is_ok_and(|v| v == "1");

    // Idempotent: drop the rows this run will re-create so re-running
    // doesn't trip add_chart's plain INSERT on a duplicate id. (The
    // PMTiles files themselves are overwritten in place.)
    let kinds_clause = if tile_ifr {
        "('TerminalAreaChart','VfrFlyway','HelicopterRoute','IfrEnrouteLow','IfrEnrouteHigh')"
    } else {
        "('TerminalAreaChart','VfrFlyway','HelicopterRoute')"
    };
    let conn = rusqlite::Connection::open(&sqlite)?;
    let deleted = conn.execute(
        &format!("DELETE FROM chart_catalog WHERE kind IN {kinds_clause}"),
        [],
    )?;
    drop(conn);
    tracing::info!(deleted, tile_ifr, "cleared existing catalog rows for the kinds being re-tiled");

    let sqlite_path = Path::new(&sqlite);
    let mut added = 0usize;

    let cycle = discover_chart_cycle()?;
    tracing::info!(
        tac = cycle.tac_names.len(),
        heli = cycle.heli_names.len(),
        "discovered terminal/heli chart lists"
    );
    for (subdir, names) in [
        ("tac-files", &cycle.tac_names),
        ("Heli_files", &cycle.heli_names),
    ] {
        for name in names {
            let workdir = tempfile::tempdir()?;
            let parts = match fetch_terminal_chart_zip(workdir.path(), name, subdir, &cycle) {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!(chart = %name, error = %e, "skipping chart that failed to fetch");
                    continue;
                }
            };
            for part in parts {
                let rgb = expand_palette_to_rgb(&part.tif_path, workdir.path())?;
                let rgb = crop_to_neatline(&rgb, workdir.path())?.unwrap_or(rgb);
                let (slug_suffix, name_suffix) = match part.kind {
                    ChartKind::TerminalAreaChart => ("tac", "TAC"),
                    ChartKind::VfrFlyway => ("fly", "VFR Flyway"),
                    ChartKind::HelicopterRoute => ("heli", "Helicopter"),
                    _ => ("chart", "Chart"),
                };
                let slug = format!("{}-{slug_suffix}", part.label.to_lowercase());
                let pmtiles_filename = format!("chart-{slug}.pmtiles");
                add_chart(
                    sqlite_path,
                    &ChartSource {
                        id: format!("{cycle_date}-{slug}"),
                        geotiff_path: rgb,
                        pmtiles_out: out_dir.join(&pmtiles_filename),
                        cycle_id: cycle_date.clone(),
                        name: format!("{} {name_suffix}", part.label.replace('_', " ")),
                        tile_url: format!("/bundles/{cycle_date}/{pmtiles_filename}"),
                        kind: part.kind,
                    },
                )?;
                added += 1;
                tracing::info!(chart = %part.label, kind = ?part.kind, added, "tiled + cataloged");
            }
        }
    }

    if tile_ifr {
        // Same ids/names/tile_urls as pipeline.rs's IFR loop, so a re-tile
        // is a drop-in replacement for the originally-published rows.
        let ifr_cycle = discover_ifr_enroute_cycle()?;
        tracing::info!(
            low = ifr_cycle.low_panel_names.len(),
            high = ifr_cycle.high_panel_names.len(),
            "discovered IFR enroute panel lists"
        );
        let series: [(&[String], ChartKind, &str); 2] = [
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
        for (panel_names, kind, series_label) in series {
            for panel_name in panel_names {
                let workdir = tempfile::tempdir()?;
                let parts = match fetch_ifr_enroute_panel(workdir.path(), panel_name, &ifr_cycle) {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::warn!(panel = %panel_name, error = %e, "skipping panel that failed to fetch");
                        continue;
                    }
                };
                for part in parts {
                    let rgb = expand_palette_to_rgb(&part.tif_path, workdir.path())?;
                    let rgb = crop_to_neatline(&rgb, workdir.path())?.unwrap_or(rgb);
                    let slug = part.label.to_lowercase();
                    let pmtiles_filename = format!("chart-{slug}.pmtiles");
                    add_chart(
                        sqlite_path,
                        &ChartSource {
                            id: format!("{cycle_date}-{slug}"),
                            geotiff_path: rgb,
                            pmtiles_out: out_dir.join(&pmtiles_filename),
                            cycle_id: cycle_date.clone(),
                            name: format!("{series_label} {panel_name}"),
                            tile_url: format!("/bundles/{cycle_date}/{pmtiles_filename}"),
                            kind,
                        },
                    )?;
                    added += 1;
                    tracing::info!(panel = %part.label, kind = ?kind, added, "tiled + cataloged");
                }
            }
        }
    }

    tracing::info!(added, "done adding charts");
    Ok(())
}
