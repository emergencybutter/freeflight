//! Validates `GAirmet`/`Sigmet`/`IntlSigmet` deserialization against
//! real `aviationweather.gov` responses (captured live, checked in as
//! fixtures below — no network needed for these tests).
//!
//! Confirmed live shape quirks these fixtures pin down:
//! - `GAirmet::coords` uses numeric-looking *strings* for lat/lon
//!   (`"35.51"`), unlike every other coordinate pair in this crate.
//! - `GAirmet` covers both `"AREA"` and `"LINE"` `geometryType`s with
//!   the same flat `coords` shape.
//! - `IntlSigmet::coords` is polymorphic: `geom: "AREA"` is a flat point
//!   list, `geom: "AREAS"` is a list of point lists (multi-polygon) —
//!   kept as raw JSON because of this, see the type's doc comment.
//! - `IntlSigmet::dir`/`spd` are strings (e.g. `"0"`, `"-"`), not numbers.
//!
//! `fetches_live_hazards` additionally hits the real API. Not run by
//! default:
//!
//! ```sh
//! cargo test -p ff-weather --test real_hazards -- --ignored --nocapture
//! ```
use ff_weather::{GAirmet, IntlSigmet, Sigmet, WeatherClient};

#[test]
fn parses_real_gairmet_response_including_area_and_line_geometry() {
    let json = include_str!("fixtures_real_gairmet.json");
    let records: Vec<GAirmet> = serde_json::from_str(json).expect("deserialize real G-AIRMET response");
    assert!(records.iter().any(|r| r.geometry_type == "AREA"));
    assert!(records.iter().any(|r| r.geometry_type == "LINE"));
    for r in &records {
        assert!(!r.coords.is_empty());
        // Confirms these really do parse as numbers despite being
        // transported as JSON strings.
        for c in &r.coords {
            c.lat.parse::<f64>().expect("lat should be a parseable number");
            c.lon.parse::<f64>().expect("lon should be a parseable number");
        }
    }
}

#[test]
fn parses_real_sigmet_response() {
    let json = include_str!("fixtures_real_sigmet.json");
    let records: Vec<Sigmet> = serde_json::from_str(json).expect("deserialize real SIGMET response");
    assert!(!records.is_empty());
    assert!(records.iter().all(|r| !r.coords.is_empty()));
}

#[test]
fn parses_real_isigmet_response_including_area_and_areas_geometry() {
    let json = include_str!("fixtures_real_isigmet.json");
    let records: Vec<IntlSigmet> = serde_json::from_str(json).expect("deserialize real international SIGMET response");

    let area = records.iter().find(|r| r.geom == "AREA").expect("an AREA record");
    assert!(area.coords.is_array());
    assert!(area.coords.as_array().unwrap()[0].is_object());

    let areas = records.iter().find(|r| r.geom == "AREAS").expect("an AREAS record");
    assert!(areas.coords.is_array());
    assert!(areas.coords.as_array().unwrap()[0].is_array());
}

#[tokio::test]
#[ignore]
async fn fetches_live_hazards() {
    let client = WeatherClient::new();
    // Not asserting non-empty: whether any of these are non-empty at a
    // given moment is real weather, not something this test controls.
    client.fetch_gairmets().await.expect("live G-AIRMET fetch");
    client.fetch_sigmets().await.expect("live SIGMET fetch");
    client.fetch_intl_sigmets().await.expect("live international SIGMET fetch");
}
