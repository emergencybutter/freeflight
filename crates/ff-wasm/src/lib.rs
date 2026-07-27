//! wasm-bindgen bindings over `ff-planning`/`ff-core` for the web client
//! (DESIGN.md §4, §5). One binding function per capability the web UI
//! needs from the shared Rust core; route/leg data crosses the JS/Rust
//! boundary as JSON rather than hand-mapped `wasm-bindgen` structs, to
//! keep this crate's surface small as `ff-planning` grows.
use ff_planning::{check_weight_balance, WeightAtStation, WeightBalanceEnvelope};
use ff_planning::{
    distance_nm as core_distance_nm, initial_bearing_deg as core_initial_bearing_deg,
};
use ff_planning::{plan_flight, plan_route, plan_vertical, AircraftProfile, RoutePoint, Wind};
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
/// whatever was resolved. `decimal_year` (e.g. 2026.5) dates the WMM
/// magnetic model for the magnetic course/heading columns. Returns a JSON
/// `RoutePlanSummary`, or throws a JS exception on malformed input.
#[wasm_bindgen]
pub fn plan_route_json(
    points_json: &str,
    profile_json: &str,
    winds_json: &str,
    decimal_year: f64,
) -> Result<String, JsValue> {
    let points: Vec<RoutePoint> = serde_json::from_str(points_json)
        .map_err(|e| JsValue::from_str(&format!("invalid points JSON: {e}")))?;
    let profile: AircraftProfile = serde_json::from_str(profile_json)
        .map_err(|e| JsValue::from_str(&format!("invalid profile JSON: {e}")))?;
    let winds: Vec<Option<Wind>> = serde_json::from_str(winds_json)
        .map_err(|e| JsValue::from_str(&format!("invalid winds JSON: {e}")))?;
    let summary = plan_route(&points, &profile, Some(&winds), decimal_year);
    serde_json::to_string(&summary)
        .map_err(|e| JsValue::from_str(&format!("failed to serialize result: {e}")))
}

/// Compute the route's vertical profile — top of climb and top of
/// descent — from JSON. `points`/`profile`/`winds` are exactly as
/// [`plan_route_json`] takes them; the profile's `cruise_altitude_ft` and
/// climb/descent performance drive this one. Field elevations are passed
/// separately (rather than being read off the route points) because the
/// client knows them only for the departure/arrival *airports*, and
/// either may be unknown — pass `f64::NAN` for one that is, and that end
/// is simply left out of the result.
///
/// Returns a JSON `VerticalProfile`, or the JSON literal `null` when
/// there's nothing to compute (no cruise altitude, no usable field
/// elevation/rate pair, fewer than two points). Throws only on malformed
/// input.
#[wasm_bindgen]
pub fn plan_vertical_json(
    points_json: &str,
    profile_json: &str,
    winds_json: &str,
    departure_elevation_ft: f64,
    arrival_elevation_ft: f64,
) -> Result<String, JsValue> {
    let points: Vec<RoutePoint> = serde_json::from_str(points_json)
        .map_err(|e| JsValue::from_str(&format!("invalid points JSON: {e}")))?;
    let profile: AircraftProfile = serde_json::from_str(profile_json)
        .map_err(|e| JsValue::from_str(&format!("invalid profile JSON: {e}")))?;
    let winds: Vec<Option<Wind>> = serde_json::from_str(winds_json)
        .map_err(|e| JsValue::from_str(&format!("invalid winds JSON: {e}")))?;
    // NaN is the "unknown" carrier across the boundary — JS `null` can't
    // ride in an `f64` parameter, and a real elevation is never NaN.
    let elevation = |ft: f64| if ft.is_nan() { None } else { Some(ft) };
    let profile = plan_vertical(
        &points,
        &profile,
        Some(&winds),
        elevation(departure_elevation_ft),
        elevation(arrival_elevation_ft),
    );
    serde_json::to_string(&profile)
        .map_err(|e| JsValue::from_str(&format!("failed to serialize result: {e}")))
}

/// Plan a whole flight in one pass: nav log, vertical profile, and a
/// phase-aware fuel total (DESIGN.md §9.5.6).
///
/// Supersedes calling [`plan_route_json`] and [`plan_vertical_json`]
/// separately — fuel can only be decomposed by phase if one pass knows
/// both the legs and the climb/descent times, and cruise TAS comes out of
/// the profile's performance tables here rather than its scalar field.
/// Those two are kept for callers that only want one half.
///
/// Arguments are the union of the two: `points`/`profile`/`winds` as
/// [`plan_route_json`], the elevations as [`plan_vertical_json`]
/// (`f64::NAN` for unknown), and `decimal_year` dating the WMM model.
/// Returns a JSON `FlightPlanSummary`.
#[wasm_bindgen]
pub fn plan_flight_json(
    points_json: &str,
    profile_json: &str,
    winds_json: &str,
    departure_elevation_ft: f64,
    arrival_elevation_ft: f64,
    decimal_year: f64,
) -> Result<String, JsValue> {
    let points: Vec<RoutePoint> = serde_json::from_str(points_json)
        .map_err(|e| JsValue::from_str(&format!("invalid points JSON: {e}")))?;
    let profile: AircraftProfile = serde_json::from_str(profile_json)
        .map_err(|e| JsValue::from_str(&format!("invalid profile JSON: {e}")))?;
    let winds: Vec<Option<Wind>> = serde_json::from_str(winds_json)
        .map_err(|e| JsValue::from_str(&format!("invalid winds JSON: {e}")))?;
    let elevation = |ft: f64| if ft.is_nan() { None } else { Some(ft) };
    let summary = plan_flight(
        &points,
        &profile,
        Some(&winds),
        elevation(departure_elevation_ft),
        elevation(arrival_elevation_ft),
        decimal_year,
    );
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
