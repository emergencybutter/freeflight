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
