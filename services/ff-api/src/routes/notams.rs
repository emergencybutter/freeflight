use crate::state::AppState;
use axum::extract::{Query, State};
use axum::http::{header, StatusCode};
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
        Err(err) => {
            tracing::warn!("NOTAM fetch for {} failed: {err}", query.location);
            // 503, not 502: Cloudflare replaces an origin's 502 body with
            // its own HTML error page, so the client would never see why.
            if err.is_rate_limited() {
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    [(header::RETRY_AFTER, "5")],
                    "FAA NOTAM service is busy, try again in a few seconds",
                )
                    .into_response()
            } else {
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    "FAA NOTAM service request failed",
                )
                    .into_response()
            }
        }
    }
}
