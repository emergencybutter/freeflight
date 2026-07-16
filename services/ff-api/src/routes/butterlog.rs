use crate::state::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

const BUTTERLOG_BASE: &str = "https://butterlog.flyvoyager.net/api/v0";

/// Both Butterlog ids we forward — the internal numeric user id and the
/// Discord snowflake — are all-digit strings. Validate before interpolating
/// into the upstream URL so a caller can't smuggle path/query characters
/// into it (the segment rides straight from the client).
fn is_numeric_id(id: &str) -> bool {
    !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit())
}

/// Proxies a Butterlog "current flight" endpoint to sidestep browser CORS
/// (butterlog.flyvoyager.net sends no `Access-Control-Allow-Origin` for the
/// freeflight origin) and to keep the client pointed at a single API host.
async fn proxy_current(state: &AppState, url: String) -> Response {
    match state.http.get(&url).send().await {
        Ok(res) => {
            let status = res.status();
            if !status.is_success() {
                return (status, format!("Upstream returned status {status}")).into_response();
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

/// Current flight by Butterlog's internal numeric user id (the manual
/// "Butterlog User ID" a pilot can enter in Settings).
pub async fn get_current(State(state): State<AppState>, Path(user_id): Path<String>) -> Response {
    if !is_numeric_id(&user_id) {
        return (StatusCode::BAD_REQUEST, "user_id must be numeric").into_response();
    }
    proxy_current(&state, format!("{BUTTERLOG_BASE}/user/{user_id}/current")).await
}

/// Current flight by the pilot's Discord id — used to auto-link a
/// freeflight user who signed in with Discord to their Butterlog flights
/// without any manual id entry (their Discord subject *is* Butterlog's
/// `users.discord_id`).
pub async fn get_current_by_discord(
    State(state): State<AppState>,
    Path(discord_id): Path<String>,
) -> Response {
    if !is_numeric_id(&discord_id) {
        return (StatusCode::BAD_REQUEST, "discord_id must be numeric").into_response();
    }
    proxy_current(
        &state,
        format!("{BUTTERLOG_BASE}/user/by-discord/{discord_id}/current"),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::is_numeric_id;

    #[test]
    fn accepts_plain_numeric_ids() {
        assert!(is_numeric_id("12345"));
        assert!(is_numeric_id("1526759090032869396")); // a Discord snowflake
    }

    #[test]
    fn rejects_empty_and_non_numeric() {
        assert!(!is_numeric_id(""));
        assert!(!is_numeric_id("abc"));
        assert!(!is_numeric_id("12 3"));
    }

    #[test]
    fn rejects_path_and_query_injection_attempts() {
        assert!(!is_numeric_id("1/../admin"));
        assert!(!is_numeric_id("1?x=y"));
        assert!(!is_numeric_id("1/current?probe"));
    }
}
