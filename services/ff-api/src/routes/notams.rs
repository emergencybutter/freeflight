use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

/// The FAA NOTAM Search API requires a registered `client_id`/
/// `client_secret` (DESIGN.md §9.2, §12) that isn't wired into
/// configuration yet, so this route is a placeholder until that's set up.
pub async fn get_notams() -> Response {
    (
        StatusCode::NOT_IMPLEMENTED,
        "NOTAM proxy is not configured yet (needs FAA API credentials, see DESIGN.md §9.2)",
    )
        .into_response()
}
