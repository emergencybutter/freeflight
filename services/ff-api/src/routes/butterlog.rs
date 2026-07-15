use crate::state::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

/// Proxies the Butterlog current flight endpoint to avoid client-side CORS issues.
pub async fn get_current(
    State(state): State<AppState>,
    Path(user_id): Path<String>,
) -> Response {
    let url = format!("https://butterlog.flyvoyager.net/api/v0/user/{user_id}/current");
    match state.http.get(&url).send().await {
        Ok(res) => {
            let status = res.status();
            if !status.is_success() {
                return (status, format!("Upstream returned status {}", status)).into_response();
            }
            match res.bytes().await {
                Ok(bytes) => (
                    [(axum::http::header::CONTENT_TYPE, "application/json")],
                    bytes,
                )
                    .into_response(),
                Err(err) => (StatusCode::BAD_GATEWAY, err.to_string()).into_response(),
            }
        }
        Err(err) => (StatusCode::BAD_GATEWAY, err.to_string()).into_response(),
    }
}
