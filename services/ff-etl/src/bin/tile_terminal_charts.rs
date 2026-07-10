//! One-off surgical tiler: fetches FAA Terminal Area Charts (and the VFR
//! Flyway chart bundled in each TAC zip) plus Helicopter route charts, and
//! adds them to an already-published cycle bundle **in place** — the
//! PMTiles into the cycle's directory, one `chart_catalog` row each into
//! its `cycle.sqlite`. This is the `pipeline.rs` terminal/heli loop lifted
//! out so the new chart kinds can be backfilled into an existing cycle
//! without regenerating the whole 19 GB bundle.
//!
//! Requires GDAL's CLI tools on `PATH` (see `ff-charts::ingest`). Config
//! via env:
//!   FF_CYCLE_SQLITE  path to the cycle's `cycle.sqlite`
//!   FF_CYCLE_DIR     directory to write `chart-*.pmtiles` into
//!   FF_CYCLE_DATE    cycle id, e.g. "2026-07-09"

use ff_charts::ChartKind;
use ff_etl::bundle::{add_chart, ChartSource};
use ff_etl::chart_prep::expand_palette_to_rgb;
use ff_etl::fetch::{discover_chart_cycle, fetch_terminal_chart_zip};
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

    // Idempotent: drop any terminal/flyway/heli rows from a previous run so
    // re-running doesn't trip add_chart's plain INSERT on a duplicate id.
    // (The PMTiles files themselves are overwritten in place.)
    let conn = rusqlite::Connection::open(&sqlite)?;
    let deleted = conn.execute(
        "DELETE FROM chart_catalog WHERE kind IN ('TerminalAreaChart','VfrFlyway','HelicopterRoute')",
        [],
    )?;
    drop(conn);
    tracing::info!(deleted, "cleared any existing terminal/flyway/heli catalog rows");

    let cycle = discover_chart_cycle()?;
    tracing::info!(
        tac = cycle.tac_names.len(),
        heli = cycle.heli_names.len(),
        "discovered terminal/heli chart lists"
    );

    let sqlite_path = Path::new(&sqlite);
    let mut added = 0usize;
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
                let (slug_suffix, name_suffix) = match part.kind {
                    ChartKind::TerminalAreaChart => ("tac", "TAC"),
                    ChartKind::VfrFlyway => ("fly", "VFR Flyway"),
                    ChartKind::HelicopterRoute => ("heli", "Helicopter"),
                    _ => ("chart", "Chart"),
                };
                let slug = format!("{}-{slug_suffix}", part.label.to_lowercase());
                let pmtiles_filename = format!("chart-{slug}.pmtiles");
                let pmtiles_out = out_dir.join(&pmtiles_filename);
                add_chart(
                    sqlite_path,
                    &ChartSource {
                        id: format!("{cycle_date}-{slug}"),
                        geotiff_path: rgb,
                        pmtiles_out,
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
    tracing::info!(added, "done adding terminal/flyway/heli charts");
    Ok(())
}
