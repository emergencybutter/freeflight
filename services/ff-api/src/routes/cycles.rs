use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

/// `ff-etl` doesn't publish cycle bundles yet (DESIGN.md §7), so there is
/// nothing to serve here until that pipeline exists — return a clear
/// "not implemented" rather than fabricating a manifest.
pub async fn latest() -> Response {
    (
        StatusCode::NOT_IMPLEMENTED,
        "no cycle bundle has been published yet; run ff-etl first (see DESIGN.md §7)",
    )
        .into_response()
}
