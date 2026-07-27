mod routes;
mod state;

use state::AppState;
use std::time::{Duration, SystemTime};

/// How often the background task re-pulls the bulk METAR cache. The
/// upstream file updates ~once a minute, but 5-minute staleness is fine
/// for map-marker coloring and keeps us a good citizen; overridable via
/// `FF_WEATHER_METAR_CACHE_REFRESH_SECS`.
const DEFAULT_METAR_CACHE_REFRESH_SECS: u64 = 300;

/// How often expired sessions are cleared out (see `spawn_session_sweep`).
const SESSION_SWEEP_INTERVAL: Duration = Duration::from_secs(60 * 60);

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8080);
    let addr = format!("0.0.0.0:{port}");

    let mut state = AppState::default();
    // Optional (FF_DATABASE_URL); logs and carries on if absent or
    // unreachable — see AppState::connect_accounts.
    state.connect_accounts().await;
    spawn_flight_category_refresh(state.clone());
    spawn_session_sweep(state.clone());
    let app = routes::router(state);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| panic!("failed to bind {addr}: {e}"));
    tracing::info!("ff-api listening on {addr}");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("server error");
}

/// Periodically pulls aviationweather.gov's bulk METAR cache into
/// `state.flight_categories` so `/weather/flightcat` (and thus airport
/// marker coloring) is served from memory — one upstream download every
/// few minutes, shared across all clients, instead of a station-list
/// query per map pan. A failed refresh logs and keeps the last good
/// snapshot; it never blanks the map.
fn spawn_flight_category_refresh(state: AppState) {
    let refresh = Duration::from_secs(
        std::env::var("FF_WEATHER_METAR_CACHE_REFRESH_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_METAR_CACHE_REFRESH_SECS),
    );
    tokio::spawn(async move {
        loop {
            match state.weather.fetch_metar_flight_categories().await {
                Ok(categories) => {
                    let count = categories.len();
                    let mut cache = state.flight_categories.write().await;
                    cache.categories = categories;
                    cache.updated = Some(SystemTime::now());
                    tracing::info!("loaded {count} METAR flight categories");
                }
                Err(err) => {
                    tracing::warn!("failed to refresh METAR flight categories: {err}");
                }
            }
            tokio::time::sleep(refresh).await;
        }
    });
}

/// Drops expired sessions periodically. The old in-memory map swept
/// itself on every write and lost everything on restart anyway; rows
/// persist, so something has to clear them out. Hourly is plenty for a
/// 30-day TTL — this is housekeeping, not a security boundary, since
/// expiry is enforced in the lookup query regardless of whether the row
/// has been collected yet.
fn spawn_session_sweep(state: AppState) {
    tokio::spawn(async move {
        loop {
            routes::auth::sweep_expired_sessions(&state).await;
            tokio::time::sleep(SESSION_SWEEP_INTERVAL).await;
        }
    });
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to listen for ctrl-c");
}
