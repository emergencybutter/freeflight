use crate::state::AppState;
use axum::extract::{Path as CycleIdPath, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

/// Manifest for the most recently published cycle bundle (DESIGN.md §7),
/// as published by `ff-etl` (`cargo run -p ff-etl`) into `FF_ETL_DATA_DIR`
/// (default `data/`, shared with this server via the same env var).
pub async fn latest(State(state): State<AppState>) -> Response {
    match ff_etl::publish::latest_bundle_path(&state.data_dir) {
        Ok(Some(bundle_path)) => {
            let cycle_id = bundle_path
                .parent()
                .and_then(|p| p.file_name())
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            Json(json!({
                "cycle_id": cycle_id,
                "bundle_url": format!("/cycles/{cycle_id}/bundle.sqlite"),
            }))
            .into_response()
        }
        Ok(None) => (
            StatusCode::NOT_IMPLEMENTED,
            "no cycle bundle has been published yet; run `cargo run -p ff-etl` first",
        )
            .into_response(),
        Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
    }
}

/// Serves the raw SQLite bytes for a published cycle.
pub async fn bundle(State(state): State<AppState>, CycleIdPath(cycle_id): CycleIdPath<String>) -> Response {
    let path = state.data_dir.join("cycles").join(&cycle_id).join("cycle.sqlite");
    match tokio::fs::read(&path).await {
        Ok(bytes) => ([(header::CONTENT_TYPE, "application/vnd.sqlite3")], bytes).into_response(),
        Err(_) => (StatusCode::NOT_FOUND, "cycle bundle not found").into_response(),
    }
}
