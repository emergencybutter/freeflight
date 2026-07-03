use crate::state::AppState;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use ff_sync::{sha256_hex, CycleManifest};

/// Manifest for the most recently published cycle bundle (DESIGN.md §7),
/// as published by `ff-etl` (`cargo run -p ff-etl`) into `FF_ETL_DATA_DIR`
/// (default `data/`, shared with this server via the same env var). The
/// files themselves are served under `/bundles/…` (a static-file service
/// with HTTP Range support — the pmtiles format is fetched by range
/// requests, so plain read-whole-file handlers wouldn't do).
///
/// Returns `ff_sync::CycleManifest` directly rather than a hand-rolled
/// JSON object, so this route and `ff-sync`'s client-side deserializer
/// can't drift out of sync the way they did before either side had
/// actually been run against the other (see manifest.rs's doc comment).
pub async fn latest(State(state): State<AppState>) -> Response {
    let bundle_path = match ff_etl::publish::latest_bundle_path(&state.data_dir) {
        Ok(Some(path)) => path,
        Ok(None) => {
            return (
                StatusCode::NOT_IMPLEMENTED,
                "no cycle bundle has been published yet; run `cargo run -p ff-etl` first",
            )
                .into_response()
        }
        Err(err) => return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
    };
    let cycle_id = bundle_path
        .parent()
        .and_then(|p| p.file_name())
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();

    let bytes = match tokio::fs::read(&bundle_path).await {
        Ok(bytes) => bytes,
        Err(err) => return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
    };

    // `CycleManifest` (an `ff-sync` type, shared with the not-yet-built
    // Android client) only has room for a single chart file — a leftover
    // from when the pipeline tiled exactly one regional sectional per
    // cycle. A nationwide cycle now publishes dozens of per-sectional
    // PMTiles files (see `/data/charts` and `chart_catalog` for the real,
    // complete list), which that single-file shape can't represent, so
    // it's left `None` here rather than arbitrarily picking one chart and
    // implying it's "the" chart. Revisit once Android sync needs real
    // multi-chart offline support.
    let (pmtiles_url, pmtiles_sha256) = (None, None);

    Json(CycleManifest {
        sqlite_url: format!("/bundles/{cycle_id}/cycle.sqlite"),
        sqlite_sha256: sha256_hex(&bytes),
        pmtiles_url,
        pmtiles_sha256,
        cycle_id,
    })
    .into_response()
}
