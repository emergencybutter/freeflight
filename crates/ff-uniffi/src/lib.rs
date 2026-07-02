//! UniFFI bindings over `ff-planning`/`ff-core` for the Android client
//! (DESIGN.md §4, §5), mirroring `ff-wasm`'s surface for the web client so
//! both bindings expose the same operations from one Rust core.
//!
//! Uses UniFFI's proc-macro export mode (no `.udl` file). Kotlin bindings
//! are generated from this crate with `uniffi-bindgen`.
use ff_planning::{
    distance_nm as core_distance_nm, initial_bearing_deg as core_initial_bearing_deg,
};
use ff_planning::{plan_route, AircraftProfile, RoutePoint};

uniffi::setup_scaffolding!();

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum PlanningError {
    #[error("invalid points JSON: {0}")]
    InvalidPoints(String),
    #[error("invalid profile JSON: {0}")]
    InvalidProfile(String),
    #[error("failed to serialize result: {0}")]
    Serialize(String),
}

#[uniffi::export]
pub fn distance_nm(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    core_distance_nm((lat1, lon1), (lat2, lon2))
}

#[uniffi::export]
pub fn initial_bearing_deg(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    core_initial_bearing_deg((lat1, lon1), (lat2, lon2))
}

/// Plan a route from JSON, same wire shape as `ff-wasm::plan_route_json`
/// (a JSON array of `{"lat":.., "lon":..}` points and a JSON
/// `AircraftProfile`), returning a JSON `RoutePlanSummary`.
#[uniffi::export]
pub fn plan_route_json(points_json: String, profile_json: String) -> Result<String, PlanningError> {
    let points: Vec<RoutePoint> = serde_json::from_str(&points_json)
        .map_err(|e| PlanningError::InvalidPoints(e.to_string()))?;
    let profile: AircraftProfile = serde_json::from_str(&profile_json)
        .map_err(|e| PlanningError::InvalidProfile(e.to_string()))?;
    let summary = plan_route(&points, &profile, None);
    serde_json::to_string(&summary).map_err(|e| PlanningError::Serialize(e.to_string()))
}
