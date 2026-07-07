//! Re-syncs `dtpp_chart` rows against an already-published bundle,
//! without re-running the whole multi-hour pipeline — useful if the
//! matching heuristic (`ff_etl::dtpp`) improves later and existing
//! cycles should pick up better matches, or after a fresh publish that
//! predates this feature. Safe to re-run: clears this bundle's existing
//! `dtpp_chart` rows first, so re-running never accumulates duplicates.
//!
//! Usage: `cargo run --release --example sync_dtpp_charts -- <bundle_path>`
use ff_etl::bundle::add_dtpp_charts;
use ff_etl::dtpp::{discover_dtpp_cycle, fetch_and_match_dtpp_charts};
use std::path::Path;

fn main() {
    tracing_subscriber::fmt::init();
    let bundle_path_str = std::env::args()
        .nth(1)
        .expect("usage: sync_dtpp_charts <bundle_path>");
    let bundle_path = Path::new(&bundle_path_str);

    // CREATE TABLE IF NOT EXISTS -- safe against an already-published
    // bundle that predates this migration.
    let conn = ff_storage::open(bundle_path.to_str().unwrap()).expect("apply migrations");
    conn.execute("DELETE FROM dtpp_chart", [])
        .expect("clear existing dtpp_chart rows");
    drop(conn);

    let cycle = discover_dtpp_cycle().expect("discover cycle");
    tracing::info!(cycle = %cycle, "discovered d-TPP cycle");

    let matched = fetch_and_match_dtpp_charts(bundle_path, &cycle).expect("fetch+match");
    let mut by_airport: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for c in &matched {
        by_airport.insert(&c.airport_icao);
    }
    tracing::info!(
        charts = matched.len(),
        airports = by_airport.len(),
        "matched d-TPP charts against this bundle's procedures"
    );

    add_dtpp_charts(bundle_path, &matched).expect("insert dtpp_chart rows");
    tracing::info!(count = matched.len(), "synced dtpp_chart rows");
}
