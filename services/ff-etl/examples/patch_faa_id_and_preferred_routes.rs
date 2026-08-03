//! Standalone: backfill `airport.faa_id` and add preferred/coded-
//! departure routes to an *already-built* bundle in place, without a
//! full pipeline run (which would also re-fetch CIFP and re-tile every
//! FAA chart nationwide — unnecessary just to add the FAA-LID mapping
//! and route suggestions to a bundle that already exists at the right
//! AIRAC cycle). Mirrors `add_openaip_to_bundle.rs`'s shape.
//!
//! `faa_id` has to come first: `preferred_routes::fetch_and_parse`
//! resolves the NFDC Preferred Routes file's 3-letter domestic idents
//! (e.g. "ABE") against exactly that column, so a bundle with it still
//! empty would resolve nothing but the CDR rows (already full ICAO) and
//! the handful of PFR rows whose Orig/Dest happen to already be 4-letter
//! ICAO (Canada, mostly).
//!
//! ```sh
//! FF_TARGET_BUNDLE=/path/to/cycle.sqlite \
//! FF_NASR_CYCLE_DATE=2026-08-06 \
//!   cargo run -p ff-etl --example patch_faa_id_and_preferred_routes
//! ```
use ff_etl::bundle::add_preferred_routes;
use ff_etl::preferred_routes::fetch_and_parse;
use std::path::Path;

fn main() {
    tracing_subscriber::fmt::init();
    let target = std::env::var("FF_TARGET_BUNDLE").expect("set FF_TARGET_BUNDLE");
    let cycle_date = std::env::var("FF_NASR_CYCLE_DATE").expect("set FF_NASR_CYCLE_DATE");
    let bundle_path = Path::new(&target);
    assert!(bundle_path.exists(), "target bundle {target} does not exist");

    // Bring the schema up to date first (adds `preferred_route` if this
    // bundle predates that migration) — safe to call on an
    // already-populated bundle, `ff_storage::open` only ever runs
    // migrations it hasn't applied yet.
    ff_storage::open(&target).expect("apply schema migrations");

    println!("fetching NASR {cycle_date} to backfill faa_id...");
    let workdir = tempfile::tempdir().expect("tempdir");
    let nasr_dir = ff_etl::fetch::fetch_nasr(workdir.path(), &cycle_date).expect("fetch_nasr");
    let apt_base = ff_nasr::parse_apt_base(
        &std::fs::read(nasr_dir.join("APT_BASE.csv")).expect("read APT_BASE.csv"),
    )
    .expect("parse APT_BASE.csv");

    let conn = rusqlite::Connection::open(bundle_path).expect("open bundle");
    let mut updated = 0usize;
    for row in &apt_base {
        let icao = row.icao_id.clone().unwrap_or_else(|| row.arpt_id.clone());
        updated += conn
            .execute(
                "UPDATE airport SET faa_id = ?1 WHERE icao = ?2",
                rusqlite::params![row.arpt_id, icao],
            )
            .expect("update faa_id");
    }
    println!("backfilled faa_id on {updated} airport rows (of {} NASR entries)", apt_base.len());
    drop(conn);

    println!("fetching preferred/coded-departure routes...");
    let routes = fetch_and_parse(bundle_path).expect("fetch_and_parse");
    let mut by_source: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for r in &routes {
        *by_source.entry(r.source).or_default() += 1;
    }
    println!("resolved {} routes: {:?}", routes.len(), by_source);
    let inserted = add_preferred_routes(bundle_path, &routes).expect("add_preferred_routes");
    println!("inserted {inserted} preferred_route rows");

    let conn = rusqlite::Connection::open(bundle_path).expect("open bundle");
    let faa_id_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM airport WHERE faa_id IS NOT NULL AND faa_id <> ''",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let route_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM preferred_route", [], |r| r.get(0))
        .unwrap();
    println!("final: {faa_id_count} airports with faa_id, {route_count} preferred_route rows");

    for (from, to) in [("KABE", "KACY"), ("KJFK", "KBOS"), ("KLAX", "KSFO")] {
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM preferred_route WHERE orig_icao=?1 AND dest_icao=?2",
                rusqlite::params![from, to],
                |r| r.get(0),
            )
            .unwrap();
        println!("  {from} -> {to}: {n} suggested routes");
    }
}
