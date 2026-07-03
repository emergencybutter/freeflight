use crate::state::AppState;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct LocationQuery {
    pub location: String,
}

/// Proxies the FAA NOTAM Management Service (DESIGN.md §9.2, §12) when
/// `FF_NOTAM_CLIENT_ID`/`FF_NOTAM_CLIENT_SECRET` are configured — see
/// `ff-notam`'s crate docs for why that's an email request to
/// NOTAMS@faa.gov rather than self-service signup, and why the response
/// is passed through as raw JSON rather than typed structs.
pub async fn get_notams(
    State(state): State<AppState>,
    Query(query): Query<LocationQuery>,
) -> Response {
    let Some(notam) = &state.notam else {
        return (
            StatusCode::NOT_IMPLEMENTED,
            "NOTAM proxy is not configured (set FF_NOTAM_CLIENT_ID/FF_NOTAM_CLIENT_SECRET; \
             see ff-notam's crate docs for how to request credentials)",
        )
            .into_response();
    };
    match notam.fetch_notams_raw(&query.location).await {
        Ok(value) => Json(value).into_response(),
        Err(err) => (StatusCode::BAD_GATEWAY, err.to_string()).into_response(),
    }
}
