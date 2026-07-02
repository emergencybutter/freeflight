//! wasm-bindgen bindings over `ff-planning`/`ff-core` for the web client
//! (DESIGN.md §4, §5). One binding function per capability the web UI
//! needs from the shared Rust core; route/leg data crosses the JS/Rust
//! boundary as JSON rather than hand-mapped `wasm-bindgen` structs, to
//! keep this crate's surface small as `ff-planning` grows.
use ff_planning::{
    distance_nm as core_distance_nm, initial_bearing_deg as core_initial_bearing_deg,
};
use ff_planning::{plan_route, AircraftProfile, RoutePoint};
use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn init() {
    console_error_panic_hook::set_once();
}

#[wasm_bindgen]
pub fn distance_nm(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    core_distance_nm((lat1, lon1), (lat2, lon2))
}

#[wasm_bindgen]
pub fn initial_bearing_deg(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    core_initial_bearing_deg((lat1, lon1), (lat2, lon2))
}

/// Plan a route from JSON: `points` is a JSON array of `{"lat":..,
/// "lon":..}`, `profile` is a JSON `AircraftProfile`. Returns a JSON
/// `RoutePlanSummary`, or throws a JS exception on malformed input.
#[wasm_bindgen]
pub fn plan_route_json(points_json: &str, profile_json: &str) -> Result<String, JsValue> {
    let points: Vec<RoutePoint> = serde_json::from_str(points_json)
        .map_err(|e| JsValue::from_str(&format!("invalid points JSON: {e}")))?;
    let profile: AircraftProfile = serde_json::from_str(profile_json)
        .map_err(|e| JsValue::from_str(&format!("invalid profile JSON: {e}")))?;
    let summary = plan_route(&points, &profile, None);
    serde_json::to_string(&summary)
        .map_err(|e| JsValue::from_str(&format!("failed to serialize result: {e}")))
}
