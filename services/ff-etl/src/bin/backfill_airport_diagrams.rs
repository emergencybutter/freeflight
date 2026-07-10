//! One-off backfill: adds FAA d-TPP airport diagram links to an
//! already-published cycle bundle **in place** — pure metadata (a PDF
//! URL string), no raster processing, so unlike `tile_terminal_charts`
//! this needs no GDAL.
//!
//! `ff_etl::dtpp::fetch_and_match_dtpp_charts` matches *every* wanted
//! chart code (SID/STAR/Approach/Airport Diagram) against the bundle's
//! procedures each time it runs, so this filters its result down to just
//! the newly-supported `AIRPORT_DIAGRAM_IDENT` rows before inserting —
//! otherwise re-running it would duplicate the SID/STAR/Approach chart
//! links the original ETL pass already added. Idempotent: clears any
//! prior airport-diagram rows first, same pattern as
//! `tile_terminal_charts`.
//!
//! Config via env:
//!   FF_CYCLE_SQLITE  path to the cycle's `cycle.sqlite`

use ff_etl::bundle::add_dtpp_charts;
use ff_etl::dtpp::{discover_dtpp_cycle, fetch_and_match_dtpp_charts, AIRPORT_DIAGRAM_IDENT};
use std::path::Path;

fn main() {
    tracing_subscriber::fmt::init();
    if let Err(err) = run() {
        tracing::error!("airport-diagram backfill stopped: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let sqlite = std::env::var("FF_CYCLE_SQLITE")?;
    let sqlite_path = Path::new(&sqlite);

    let conn = rusqlite::Connection::open(&sqlite)?;
    let deleted = conn.execute(
        "DELETE FROM dtpp_chart WHERE procedure_ident = ?1",
        [AIRPORT_DIAGRAM_IDENT],
    )?;
    drop(conn);
    tracing::info!(deleted, "cleared any existing airport-diagram rows");

    let cycle = discover_dtpp_cycle()?;
    tracing::info!(cycle = %cycle, "discovered current d-TPP cycle");

    let matched = fetch_and_match_dtpp_charts(sqlite_path, &cycle)?;
    let diagrams: Vec<_> = matched
        .into_iter()
        .filter(|c| c.procedure_ident == AIRPORT_DIAGRAM_IDENT)
        .collect();
    tracing::info!(count = diagrams.len(), "matched airport diagrams");

    add_dtpp_charts(sqlite_path, &diagrams)?;
    tracing::info!(added = diagrams.len(), "done adding airport diagrams");
    Ok(())
}
