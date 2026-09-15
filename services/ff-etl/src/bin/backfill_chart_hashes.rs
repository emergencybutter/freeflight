//! One-off backfill: records each chart's content hash and published size
//! in an already-published cycle bundle, in place.
//!
//! Cycles built before migrations 0007/0008 carry neither, which leaves
//! three client behaviours dormant (DESIGN.md §8): chart downloads are
//! unverified, a chart set cannot say what it will cost, and archives are
//! not reused across cycles, so every AIRAC update re-downloads ~20GB of
//! sectionals that mostly did not change. Re-running the full ETL would
//! recover the columns only by re-tiling all that imagery through GDAL;
//! the published archives are already on disk next to the bundle, so this
//! reads the hash and size straight off them instead. Pure metadata — no
//! GDAL needed.
//!
//! Idempotent: rows that already have both are skipped, so a re-run costs
//! a catalogue scan rather than re-hashing everything.
//!
//! Config via env:
//!   FF_ETL_DATA_DIR   published data dir (default "data"); the active
//!                     cycle is resolved through its `latest.json`
//!   FF_BACKFILL_FORCE set to recompute rows that already have values —
//!                     for when published files were replaced in place

use ff_etl::chart_hashes::backfill_chart_hashes;
use std::path::PathBuf;

fn main() {
    tracing_subscriber::fmt::init();
    if let Err(err) = run() {
        tracing::error!("chart-hash backfill stopped: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let data_dir =
        PathBuf::from(std::env::var("FF_ETL_DATA_DIR").unwrap_or_else(|_| "data".to_string()));
    let force = std::env::var("FF_BACKFILL_FORCE").is_ok();

    let bundle = ff_etl::publish::latest_bundle_path(&data_dir)?
        .ok_or("no published cycle in that data dir (no latest.json)")?;
    // The published PMTiles archives sit beside the bundle, in the same
    // per-cycle directory.
    let charts_dir = bundle
        .parent()
        .ok_or("published bundle has no parent directory")?
        .to_path_buf();

    tracing::info!(bundle = %bundle.display(), force, "backfilling chart hashes");
    let stats = backfill_chart_hashes(&bundle, &charts_dir, force)?;
    tracing::info!(
        filled = stats.filled,
        skipped = stats.skipped,
        missing = stats.missing,
        "done"
    );
    Ok(())
}
