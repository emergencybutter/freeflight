use crate::state::AppState;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct StationQuery {
    /// Comma-separated ICAO station ids, e.g. "KSFO,KOAK".
    pub ids: String,
}

fn parse_ids(raw: &str) -> Vec<&str> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect()
}

/// Proxies aviationweather.gov METAR (DESIGN.md §4, §9.2) so clients don't
/// need to embed/refresh an upstream base URL and get a consistent place
/// to add caching later.
pub async fn get_metars(
    State(state): State<AppState>,
    Query(query): Query<StationQuery>,
) -> Response {
    let ids = parse_ids(&query.ids);
    match state.weather.fetch_metars(&ids).await {
        Ok(metars) => Json(metars).into_response(),
        Err(err) => (StatusCode::BAD_GATEWAY, err.to_string()).into_response(),
    }
}

pub async fn get_tafs(
    State(state): State<AppState>,
    Query(query): Query<StationQuery>,
) -> Response {
    let ids = parse_ids(&query.ids);
    match state.weather.fetch_tafs(&ids).await {
        Ok(tafs) => Json(tafs).into_response(),
        Err(err) => (StatusCode::BAD_GATEWAY, err.to_string()).into_response(),
    }
}

/// Proxies aviationweather.gov's Graphical AIRMET — no station filter,
/// same as upstream (all current CONUS records).
pub async fn get_gairmets(State(state): State<AppState>) -> Response {
    match state.weather.fetch_gairmets().await {
        Ok(records) => Json(records).into_response(),
        Err(err) => (StatusCode::BAD_GATEWAY, err.to_string()).into_response(),
    }
}

/// Proxies aviationweather.gov's US domestic/convective SIGMET.
pub async fn get_sigmets(State(state): State<AppState>) -> Response {
    match state.weather.fetch_sigmets().await {
        Ok(records) => Json(records).into_response(),
        Err(err) => (StatusCode::BAD_GATEWAY, err.to_string()).into_response(),
    }
}

/// Proxies aviationweather.gov's international/oceanic SIGMET.
pub async fn get_intl_sigmets(State(state): State<AppState>) -> Response {
    match state.weather.fetch_intl_sigmets().await {
        Ok(records) => Json(records).into_response(),
        Err(err) => (StatusCode::BAD_GATEWAY, err.to_string()).into_response(),
    }
}

#[derive(Debug, Deserialize)]
pub struct WindsAloftQuery {
    #[serde(default = "default_level")]
    pub level: String,
    #[serde(default = "default_fcst")]
    pub fcst: String,
    #[serde(default = "default_region")]
    pub region: String,
}

fn default_level() -> String {
    "low".to_string()
}

fn default_fcst() -> String {
    "06".to_string()
}

fn default_region() -> String {
    "all".to_string()
}

/// Proxies aviationweather.gov's winds/temps aloft forecast — the one
/// product `ff-weather` gets as fixed-width text rather than JSON (see
/// `ff_weather::winds_aloft`), reserialized here as JSON like everything
/// else this server proxies.
pub async fn get_winds_aloft(
    State(state): State<AppState>,
    Query(query): Query<WindsAloftQuery>,
) -> Response {
    match state
        .weather
        .fetch_winds_aloft(&query.level, &query.fcst, &query.region)
        .await
    {
        Ok(bulletin) => Json(bulletin).into_response(),
        Err(err) => (StatusCode::BAD_GATEWAY, err.to_string()).into_response(),
    }
}
