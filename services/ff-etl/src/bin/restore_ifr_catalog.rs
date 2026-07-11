//! Emergency recovery tool: restores IfrEnrouteLow/IfrEnrouteHigh
//! `chart_catalog` rows from their still-on-disk PMTiles files.
//!
//! Context: a re-crop attempt (`tile_terminal_charts` with `FF_TILE_IFR=1`)
//! deletes existing rows for the kinds it's about to re-tile *before*
//! re-tiling, so it can safely re-run without duplicating rows. That
//! attempt got stuck on the very first IFR panel (GDAL/memory pressure)
//! and was killed — the delete had already run, but no new IFR rows were
//! ever inserted, while the original `.pmtiles` files were never
//! overwritten (tiling never got far enough to reach the final-output
//! write). So the files are intact; only the catalog rows pointing at
//! them are missing.
//!
//! Rather than re-tiling (the expensive, GDAL-dependent, currently-
//! failing path), this reads each file's own PMTiles header — `min_pos`/
//! `max_pos` give the exact bbox for free, no GDAL needed — and
//! reconstructs the id/name/tile_url using the same convention
//! `pipeline.rs`'s IFR loop already uses, deriving them from the
//! filename alone (`chart-<slug>.pmtiles`; the slug is exactly what
//! `pipeline.rs` would have used as `part.label.to_lowercase()`).
//! Idempotent: skips a file if a matching row already exists.
//!
//! Config via env:
//!   FF_CYCLE_DIR     directory holding chart-*.pmtiles and cycle.sqlite
//!   FF_CYCLE_SQLITE  path to the cycle's cycle.sqlite
//!   FF_CYCLE_DATE    cycle id, e.g. "2026-07-09"

use pmtiles2::Header;
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;

fn main() {
    tracing_subscriber::fmt::init();
    if let Err(err) = run() {
        tracing::error!("IFR catalog restore stopped: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cycle_dir = PathBuf::from(std::env::var("FF_CYCLE_DIR")?);
    let sqlite = std::env::var("FF_CYCLE_SQLITE")?;
    let cycle_date = std::env::var("FF_CYCLE_DATE")?;

    let conn = rusqlite::Connection::open(&sqlite)?;
    let mut restored = 0usize;
    let mut skipped = 0usize;

    let mut entries: Vec<_> = std::fs::read_dir(&cycle_dir)?.collect::<Result<_, _>>()?;
    entries.sort_by_key(std::fs::DirEntry::file_name);

    for entry in entries {
        let filename = entry.file_name().to_string_lossy().into_owned();
        let Some(slug) = filename
            .strip_prefix("chart-")
            .and_then(|s| s.strip_suffix(".pmtiles"))
        else {
            continue;
        };
        let (kind, series_label) = if slug.starts_with("enr_l") {
            ("IfrEnrouteLow", "IFR Low Altitude Enroute")
        } else if slug.starts_with("enr_h") {
            ("IfrEnrouteHigh", "IFR High Altitude Enroute")
        } else {
            continue;
        };

        let tile_url = format!("/bundles/{cycle_date}/{filename}");
        let exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM chart_catalog WHERE kind = ?1 AND tile_url = ?2)",
            rusqlite::params![kind, tile_url],
            |row| row.get(0),
        )?;
        if exists {
            skipped += 1;
            continue;
        }

        let file = File::open(entry.path())?;
        let mut reader = BufReader::new(file);
        let header = Header::from_reader(&mut reader)?;

        let id = format!("{cycle_date}-{slug}");
        let name = format!("{series_label} {slug}");

        conn.execute(
            "INSERT INTO chart_catalog (id, name, kind, cycle_id, min_lat, min_lon, max_lat, max_lon, tile_url)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            rusqlite::params![
                id,
                name,
                kind,
                cycle_date,
                header.min_pos.latitude,
                header.min_pos.longitude,
                header.max_pos.latitude,
                header.max_pos.longitude,
                tile_url,
            ],
        )?;
        restored += 1;
        tracing::info!(slug, kind, "restored chart_catalog row");
    }

    tracing::info!(restored, skipped, "done restoring IFR chart_catalog rows");
    Ok(())
}
