//! Validates `Metar`/`Taf` deserialization against real
//! `aviationweather.gov` Data API responses.
//!
//! The fixture-based tests run by default against real responses captured
//! and checked in below (small, a few KB) — no network needed. They exist
//! because the API's actual shape differs from what the hand-written unit
//! test fixture in `records.rs` assumed: `Taf::issue_time` is an ISO 8601
//! string, not an epoch integer like the other TAF timestamp fields, and
//! `TafForecastPeriod::wdir` can be the string `"VRB"` (confirmed on live
//! KATL/KDEN/KMIA TAFs with PROB/TEMPO groups), same as `Metar::wdir`.
//!
//! `fetches_live_metar_and_taf` additionally hits the real API to catch
//! future upstream schema changes. Not run by default:
//!
//! ```sh
//! cargo test -p ff-weather --test real_weather -- --ignored --nocapture
//! ```
use ff_weather::{Metar, Taf, WeatherClient};

#[test]
fn parses_real_metar_response() {
    let json = include_str!("fixtures_real_metar.json");
    let metars: Vec<Metar> = serde_json::from_str(json).expect("deserialize real METAR response");
    assert_eq!(metars.len(), 5);
    let khwd = metars
        .iter()
        .find(|m| m.icao_id == "KHWD")
        .expect("KHWD metar present");
    assert_eq!(
        khwd.wdir,
        Some(serde_json::Value::String("VRB".to_string()))
    );
}

#[test]
fn parses_real_taf_response_with_prob_tempo_and_variable_wind() {
    let json = include_str!("fixtures_real_taf.json");
    let tafs: Vec<Taf> = serde_json::from_str(json).expect("deserialize real TAF response");
    let katl = tafs
        .iter()
        .find(|t| t.icao_id == "KATL")
        .expect("KATL taf present");
    assert!(katl.issue_time.starts_with("2026-"));

    let prob_period = katl
        .fcsts
        .iter()
        .find(|f| f.fcst_change.as_deref() == Some("PROB"))
        .expect("KATL has a PROB forecast period");
    assert_eq!(
        prob_period.wdir,
        Some(serde_json::Value::String("VRB".to_string()))
    );

    let kmia = tafs
        .iter()
        .find(|t| t.icao_id == "KMIA")
        .expect("KMIA taf present");
    assert!(kmia
        .fcsts
        .iter()
        .any(|f| f.fcst_change.as_deref() == Some("TEMPO")));
}

#[tokio::test]
#[ignore]
async fn fetches_live_metar_and_taf() {
    let client = WeatherClient::new();
    let metars = client
        .fetch_metars(&["KSFO", "KOAK", "KSJC", "KPAO", "KHWD"])
        .await
        .expect("live METAR fetch");
    assert_eq!(metars.len(), 5);

    let tafs = client
        .fetch_tafs(&["KSFO", "KOAK"])
        .await
        .expect("live TAF fetch");
    assert!(!tafs.is_empty());
}
