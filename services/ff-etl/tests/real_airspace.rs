//! Validates `airspace::fetch_class_airspace`/`fetch_special_use_airspace`
//! against FAA's real, live ArcGIS feature services (a different source
//! from the 28-day NASR CSV subscription — see `airspace` module docs),
//! and `bundle::add_airspace` against a real `ff-storage` schema. Not run
//! by default (hits the network):
//!
//! ```sh
//! cargo test -p ff-etl --test real_airspace -- --ignored --nocapture
//! ```
use ff_core::{AirspaceClass, AltitudeLimit, SpecialUseKind};
use ff_etl::airspace::{fetch_class_airspace, fetch_special_use_airspace};
use ff_etl::bundle::add_airspace;

#[test]
#[ignore]
fn fetches_and_stores_real_class_airspace() {
    let volumes = fetch_class_airspace().expect("fetch real Class_Airspace data");
    // Confirmed live at the time this was written: 369 Class B + 340
    // Class C + 577 Class D shelves/sectors (1286 total). FAA amends
    // airspace over time, so this asserts "plausible ballpark", not an
    // exact count that would need updating every cycle.
    assert!(
        volumes.len() > 1000,
        "expected on the order of 1200+ Class B/C/D shelves, got {}",
        volumes.len()
    );
    assert!(volumes.iter().all(|v| matches!(
        v.class,
        AirspaceClass::B | AirspaceClass::C | AirspaceClass::D
    )));
    assert!(volumes.iter().all(|v| !v.boundary.points.is_empty()));

    let boston = volumes
        .iter()
        .find(|v| v.name.contains("BOSTON") && v.class == AirspaceClass::B)
        .expect("Boston Class B present");
    assert_eq!(boston.floor, AltitudeLimit::Surface);

    let tmpfile = tempfile::NamedTempFile::new().unwrap();
    let path_str = tmpfile.path().to_str().unwrap();
    drop(ff_storage::open(path_str).expect("migrate a fresh bundle schema"));
    add_airspace(tmpfile.path(), &volumes).expect("insert real Class Airspace volumes");

    let conn = rusqlite::Connection::open(tmpfile.path()).unwrap();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM airspace", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, volumes.len() as i64);
}

#[test]
#[ignore]
fn fetches_and_stores_real_special_use_airspace() {
    let volumes = fetch_special_use_airspace().expect("fetch real Special_Use_Airspace data");
    // Confirmed live at the time this was written: 1533 total rows
    // across MOA/Restricted/Prohibited/Warning/Alert.
    assert!(
        volumes.len() > 1000,
        "expected on the order of 1500+ special-use areas, got {}",
        volumes.len()
    );
    assert!(volumes
        .iter()
        .all(|v| matches!(v.class, AirspaceClass::SpecialUse(_))));

    // R-2202D (Nevada Test and Training Range) is a real, long-standing
    // restricted area with an unlimited top reported as a flight level
    // floor — exercises both the UNLTD sentinel and the STD/flight-level
    // code path against live data, not just the unit-test fixture.
    if let Some(r2202d) = volumes.iter().find(|v| v.name == "R-2202D") {
        assert_eq!(
            r2202d.class,
            AirspaceClass::SpecialUse(SpecialUseKind::Restricted)
        );
        assert_eq!(r2202d.ceiling, AltitudeLimit::Unlimited);
        assert!(matches!(r2202d.floor, AltitudeLimit::FlightLevel(_)));
    }

    let tmpfile = tempfile::NamedTempFile::new().unwrap();
    let path_str = tmpfile.path().to_str().unwrap();
    drop(ff_storage::open(path_str).expect("migrate a fresh bundle schema"));
    add_airspace(tmpfile.path(), &volumes).expect("insert real Special Use Airspace volumes");

    let conn = rusqlite::Connection::open(tmpfile.path()).unwrap();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM airspace", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, volumes.len() as i64);
}
