use crate::state::AppState;
use axum::extract::{Path as CycleIdPath, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use ff_sync::{sha256_hex, CycleManifest};

/// Manifest for the most recently published cycle bundle (DESIGN.md §7),
/// as published by `ff-etl` (`cargo run -p ff-etl`) into `FF_ETL_DATA_DIR`
/// (default `data/`, shared with this server via the same env var).
///
/// Returns `ff_sync::CycleManifest` directly rather than a hand-rolled
/// JSON object, so this route and `ff-sync`'s client-side deserializer
/// can't drift out of sync the way they did before either side had
/// actually been run against the other (see manifest.rs's doc comment).
pub async fn latest(State(state): State<AppState>) -> Response {
    match ff_etl::publish::latest_bundle_path(&state.data_dir) {
        Ok(Some(bundle_path)) => {
            let cycle_id = bundle_path
                .parent()
                .and_then(|p| p.file_name())
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let bytes = match tokio::fs::read(&bundle_path).await {
                Ok(bytes) => bytes,
                Err(err) => return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
            };
            Json(CycleManifest {
                sqlite_url: format!("/cycles/{cycle_id}/bundle.sqlite"),
                sqlite_sha256: sha256_hex(&bytes),
                // ff-etl's real pipeline doesn't fetch/tile chart imagery
                // yet (TODO.md) — no chart bundle to point clients at.
                pmtiles_url: None,
                pmtiles_sha256: None,
                cycle_id,
            })
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
