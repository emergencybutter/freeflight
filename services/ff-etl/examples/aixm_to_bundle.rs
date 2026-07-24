//! Standalone: build a minimal bundle from just a national AIXM export
//! (no FAA data) and print the row counts — an end-to-end check of the
//! non-US load→persist path (DESIGN.md §3.1) without needing the full
//! FAA-fetching pipeline.
//!
//! ```sh
//! FF_AIXM_FR_PATH=/path/to/export_xml_bd_SIA*.zip \
//!   cargo run -p ff-etl --example aixm_to_bundle
//! ```
use ff_etl::aixm;
use ff_etl::bundle::{add_aixm, add_airspace};
use std::path::Path;

fn main() {
    let path = aixm::configured_source().expect("set FF_AIXM_FR_PATH to the SIA export");
    let data = aixm::load(&path).expect("load AIXM export");

    let out = std::env::var("FF_AIXM_OUT").unwrap_or_else(|_| "aixm_cycle.sqlite".to_string());
    let _ = std::fs::remove_file(&out);
    ff_storage::open(&out).expect("create ff-storage schema");

    let stats = add_aixm(Path::new(&out), &data).expect("add_aixm");
    add_airspace(Path::new(&out), &data.airspaces).expect("add_airspace");
    println!("add_aixm stats: {stats:#?}");
    println!("airspaces: {}", data.airspaces.len());

    let conn = rusqlite::Connection::open(&out).unwrap();
    for t in ["airport", "runway", "navaid", "waypoint", "airway", "airway_leg", "airspace"] {
        let n: i64 = conn
            .query_row(&format!("SELECT COUNT(*) FROM {t}"), [], |r| r.get(0))
            .unwrap();
        println!("  {t:<11}{n}");
    }
    // Attribution row (Licence Ouverte): name / effective date / credit.
    let ds: Result<(String, Option<String>, String), _> = conn.query_row(
        "SELECT name, effective_date, attribution FROM data_source",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    );
    println!("data_source: {ds:?}");
}
