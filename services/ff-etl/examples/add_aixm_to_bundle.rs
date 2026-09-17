//! Standalone: fold a national AIXM export into an *already-built* bundle
//! in place, without re-running `pipeline::run()` (which re-fetches
//! CIFP/NASR and re-tiles every FAA chart nationwide — hours of GDAL work
//! that has nothing to do with adding a country's vector data).
//!
//! The counterpart to `add_openaip_to_bundle`, and the practical reason
//! mixed-cycle bundles are allowed at all (DESIGN.md §3.1): the FAA side
//! is fetched unattended while the SIA export is a manual cart download,
//! so the two routinely arrive a cycle apart. `add_aixm` records the
//! export's *own* effective date, so a bundle assembled this way says
//! which cycle each half came from and the clients flag the difference.
//!
//! Also writes the bundle's `airac_cycle` row when it is missing, since
//! that is what the clients compare each source's date against — a bundle
//! published before `add_airac_cycle` existed has no such row, and
//! without it the mixed-cycle notice silently finds nothing stale.
//!
//! ```sh
//! FF_AIXM_FR_PATH=/path/to/export_xml_bd_SIA<date>.zip \
//! FF_AIXM_TARGET_BUNDLE=/path/to/cycles/<id>/cycle.sqlite \
//! FF_AIXM_CYCLE_ID=2026-10-01 \
//!   cargo run --release -p ff-etl --example add_aixm_to_bundle
//! ```
use ff_etl::aixm;
use ff_etl::bundle::{add_airac_cycle, add_airspace, add_aixm};
use std::path::Path;

fn main() {
    tracing_subscriber::fmt::init();
    let source = aixm::configured_source().expect("set FF_AIXM_FR_PATH to the SIA export");
    let target = std::env::var("FF_AIXM_TARGET_BUNDLE").expect("set FF_AIXM_TARGET_BUNDLE");
    let cycle_id = std::env::var("FF_AIXM_CYCLE_ID").expect("set FF_AIXM_CYCLE_ID");
    let bundle_path = Path::new(&target);
    assert!(bundle_path.exists(), "target bundle {target} does not exist");

    let data = aixm::load(&source).expect("load AIXM export");

    // Refused for the same reason the pipeline refuses it: a source with
    // no effective date cannot be described downstream, so no client could
    // tell a pilot how current it is.
    let effective = data
        .effective
        .as_deref()
        .expect("the AIXM export declares no effective date; use one that does");
    if effective != cycle_id {
        tracing::warn!(
            aixm_effective = %effective,
            bundle_cycle = %cycle_id,
            "mixed-cycle bundle: this export is from a different AIRAC cycle than the bundle"
        );
    }

    add_airac_cycle(bundle_path, &cycle_id).expect("add_airac_cycle");
    let stats = add_aixm(bundle_path, &data).expect("add_aixm");
    add_airspace(bundle_path, &data.airspaces).expect("add_airspace");

    tracing::info!(
        source = %source.display(),
        effective,
        airports = stats.airports,
        runways = stats.runways,
        navaids = stats.navaids,
        waypoints = stats.waypoints,
        airways = stats.airways,
        airway_legs = stats.airway_legs,
        airspaces = data.airspaces.len(),
        "folded AIXM data into the bundle (Licence Ouverte — attribution required in clients)"
    );
}
