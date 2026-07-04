//! wasm-bindgen bindings over `ff-planning`/`ff-core` for the web client
//! (DESIGN.md §4, §5). One binding function per capability the web UI
//! needs from the shared Rust core; route/leg data crosses the JS/Rust
//! boundary as JSON rather than hand-mapped `wasm-bindgen` structs, to
//! keep this crate's surface small as `ff-planning` grows.
use ff_planning::{check_weight_balance, WeightAtStation, WeightBalanceEnvelope};
use ff_planning::{
    distance_nm as core_distance_nm, initial_bearing_deg as core_initial_bearing_deg,
};
use ff_planning::{plan_route, AircraftProfile, RoutePoint, Wind};
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
/// "lon":..}`, `profile` is a JSON `AircraftProfile`, `winds` is a JSON
/// array of `{"direction_true_deg":.., "speed_kt":..} | null`, one entry
/// per leg (`points.len() - 1`) — a `null` entry means no wind data for
/// that leg (heading = course, groundspeed = TAS), matching
/// `plan_route`'s own per-leg fallback. Picking which real winds-aloft
/// station/altitude applies to each leg happens client-side (see
/// apps/web/src/planning/windsAloft.ts) — this binding just forwards
/// whatever was resolved. Returns a JSON `RoutePlanSummary`, or throws a
/// JS exception on malformed input.
#[wasm_bindgen]
pub fn plan_route_json(
    points_json: &str,
    profile_json: &str,
    winds_json: &str,
) -> Result<String, JsValue> {
    let points: Vec<RoutePoint> = serde_json::from_str(points_json)
        .map_err(|e| JsValue::from_str(&format!("invalid points JSON: {e}")))?;
    let profile: AircraftProfile = serde_json::from_str(profile_json)
        .map_err(|e| JsValue::from_str(&format!("invalid profile JSON: {e}")))?;
    let winds: Vec<Option<Wind>> = serde_json::from_str(winds_json)
        .map_err(|e| JsValue::from_str(&format!("invalid winds JSON: {e}")))?;
    let summary = plan_route(&points, &profile, Some(&winds));
    serde_json::to_string(&summary)
        .map_err(|e| JsValue::from_str(&format!("failed to serialize result: {e}")))
}

/// Check a loading against a weight & balance envelope from JSON:
/// `items_json` is a JSON array of `{"weight_lb":.., "arm_in":..}`
/// (empty weight, pax, fuel, baggage — one entry per station),
/// `envelope_json` is a JSON `WeightBalanceEnvelope`. Returns a JSON
/// `WeightBalanceResult`, or throws a JS exception on malformed input.
#[wasm_bindgen]
pub fn check_weight_balance_json(items_json: &str, envelope_json: &str) -> Result<String, JsValue> {
    let items: Vec<WeightAtStation> = serde_json::from_str(items_json)
        .map_err(|e| JsValue::from_str(&format!("invalid items JSON: {e}")))?;
    let envelope: WeightBalanceEnvelope = serde_json::from_str(envelope_json)
        .map_err(|e| JsValue::from_str(&format!("invalid envelope JSON: {e}")))?;
    let result = check_weight_balance(&items, &envelope);
    serde_json::to_string(&result)
        .map_err(|e| JsValue::from_str(&format!("failed to serialize result: {e}")))
}
