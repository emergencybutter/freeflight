//! Standalone: add openAIP-sourced airports/navaids/airspace to an
//! *already-built* bundle in place, without going through the full
//! `pipeline::run()` (which also re-fetches CIFP/NASR and re-tiles every
//! FAA chart nationwide — unnecessary just to add a fallback-tier
//! country's vector data to a bundle that already exists and already
//! carries the right AIRAC cycle).
//!
//! `FF_OPENAIP_CYCLE_ID` should match the target bundle's actual cycle
//! (e.g. the id already live in its `chart_catalog`/`dtpp_chart` rows) —
//! it's recorded as the attribution's effective date, not validated
//! against the bundle's own content.
//!
//! ```sh
//! FF_OPENAIP_API_KEY=... \
//! FF_OPENAIP_TARGET_BUNDLE=/path/to/cycle.sqlite \
//! FF_OPENAIP_CYCLE_ID=2026-08-06 \
//!   cargo run -p ff-etl --example add_openaip_to_bundle
//! ```
use ff_etl::bundle::{add_airspace, add_openaip};
use ff_etl::openaip;
use std::path::Path;

fn main() {
    tracing_subscriber::fmt::init();
    let api_key = openaip::configured_key().expect("set FF_OPENAIP_API_KEY");
    let target =
        std::env::var("FF_OPENAIP_TARGET_BUNDLE").expect("set FF_OPENAIP_TARGET_BUNDLE");
    let cycle_id = std::env::var("FF_OPENAIP_CYCLE_ID").expect("set FF_OPENAIP_CYCLE_ID");
    let bundle_path = Path::new(&target);
    assert!(bundle_path.exists(), "target bundle {target} does not exist");

    let loaded = openaip::load_configured(&api_key).expect("load_configured");
    for s in &loaded.per_state {
        println!(
            "{} ({}): airports={} navaids={} airspaces={} skipped_airspaces={}",
            s.country, s.region, s.airports, s.navaids, s.airspaces, s.skipped_airspaces
        );
    }

    let added = add_openaip(bundle_path, &loaded.airports, &loaded.navaids, &cycle_id)
        .expect("add_openaip");
    add_airspace(bundle_path, &loaded.airspaces).expect("add_airspace");
    println!("inserted: {added:#?}");
    println!("airspaces inserted: {}", loaded.airspaces.len());

    let conn = rusqlite::Connection::open(bundle_path).unwrap();
    for t in ["airport", "navaid", "airspace"] {
        let n: i64 = conn
            .query_row(&format!("SELECT COUNT(*) FROM {t}"), [], |r| r.get(0))
            .unwrap();
        println!("  {t:<8}{n} total rows now in bundle");
    }
    let ds: Vec<(String, Option<String>, String)> = {
        let mut stmt = conn
            .prepare("SELECT name, effective_date, attribution FROM data_source ORDER BY name")
            .unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    };
    println!("data_source rows: {ds:#?}");

    let gander: Option<(String, String, f64, f64)> = conn
        .query_row(
            "SELECT icao, name, lat, lon FROM airport WHERE icao = 'CYQX'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .ok();
    println!("CYQX: {gander:?}");
}
