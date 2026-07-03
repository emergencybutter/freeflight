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
//! Separately (not a fixture — needs a live 204, see below), the API
//! returns 204 No Content rather than `[]` when no station in the
//! request has a current report, which broke `fetch_tafs` for
//! non-towered airports like KHWD until `client.rs` special-cased it.
//!
//! `fetches_live_metar_and_taf` and `fetches_live_taf_for_station_with_no_taf`
//! additionally hit the real API to catch future upstream changes. Not
//! run by default:
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
    assert_eq!(khwd.flt_cat, Some("VFR".to_string()));
    let koak = metars
        .iter()
        .find(|m| m.icao_id == "KOAK")
        .expect("KOAK metar present");
    assert_eq!(koak.flt_cat, Some("MVFR".to_string()));
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

#[tokio::test]
#[ignore]
async fn fetches_live_taf_for_station_with_no_taf() {
    // KHWD (Hayward Executive) is non-towered and has no TAF, which the
    // API signals with 204 No Content rather than an empty JSON array.
    let client = WeatherClient::new();
    let tafs = client.fetch_tafs(&["KHWD"]).await.expect("live TAF fetch");
    assert!(tafs.is_empty());
}
