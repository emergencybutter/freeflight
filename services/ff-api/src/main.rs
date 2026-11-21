mod cors;
mod ratelimit;
mod routes;
mod state;
mod version;

use state::AppState;
use std::time::{Duration, SystemTime};

/// How often the background task re-pulls the bulk METAR cache. The
/// upstream file updates ~once a minute, but 5-minute staleness is fine
/// for map-marker coloring and keeps us a good citizen; overridable via
/// `FF_WEATHER_METAR_CACHE_REFRESH_SECS`.
const DEFAULT_METAR_CACHE_REFRESH_SECS: u64 = 300;

/// How often the background task re-pulls GFS winds aloft. GFS runs
/// every 6 h; 3 h keeps us at ~8 full refreshes (~6,800 weighted
/// Open-Meteo calls) a day, under the free tier's 10,000. Overridable
/// via `FF_WINDS_ALOFT_REFRESH_SECS`.
const DEFAULT_WINDS_ALOFT_REFRESH_SECS: u64 = 3 * 60 * 60;
/// Retry delay after a failed winds-aloft refresh.
const WINDS_ALOFT_RETRY: Duration = Duration::from_secs(10 * 60);
/// Stations per Open-Meteo request, and the pause between requests:
/// 41 stations is ~213 weighted calls, so one chunk per 30 s stays well
/// under the free tier's 600/minute.
const WINDS_ALOFT_CHUNK: usize = 41;
const WINDS_ALOFT_CHUNK_PAUSE: Duration = Duration::from_secs(30);
/// Hours of forecast kept per refresh: covers the furthest offset served
/// (FD "24" -> now + 18 h) plus the refresh interval.
const WINDS_ALOFT_FORECAST_HOURS: u32 = 24;

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
    spawn_winds_aloft_refresh(state.clone());
    spawn_session_sweep(state.clone());
    let app = routes::router(state);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| panic!("failed to bind {addr}: {e}"));
    tracing::info!("ff-api listening on {addr}");

    // `into_make_service_with_connect_info` so the rate limiter can see the
    // socket peer address; without it there is no address to key on when
    // no trusted proxy header is configured.
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
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

/// Periodically pulls GFS winds/temps aloft for every FD station into
/// `state.winds_aloft` (see `routes::weather::get_winds_aloft`). Fetched
/// in spaced chunks to respect Open-Meteo's per-minute limit; a refresh
/// only replaces the snapshot if every chunk succeeded, so the map never
/// shows a partial station set. A failure keeps the last good snapshot
/// and retries sooner.
fn spawn_winds_aloft_refresh(state: AppState) {
    let refresh = Duration::from_secs(
        std::env::var("FF_WINDS_ALOFT_REFRESH_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_WINDS_ALOFT_REFRESH_SECS),
    );
    tokio::spawn(async move {
        loop {
            let fetched_at = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs() as i64);
            let mut times = Vec::new();
            let mut stations = Vec::with_capacity(ff_weather::FD_STATIONS.len());
            let mut failure = None;
            for (i, chunk) in ff_weather::FD_STATIONS.chunks(WINDS_ALOFT_CHUNK).enumerate() {
                if i > 0 {
                    tokio::time::sleep(WINDS_ALOFT_CHUNK_PAUSE).await;
                }
                match state
                    .weather
                    .fetch_gfs_profiles(chunk, WINDS_ALOFT_FORECAST_HOURS)
                    .await
                {
                    Ok((t, profiles)) => {
                        times = t;
                        stations.extend(profiles);
                    }
                    Err(err) => {
                        failure = Some(err);
                        break;
                    }
                }
            }
            let wait = match failure {
                None => {
                    let count = stations.len();
                    *state.winds_aloft.write().await = Some(ff_weather::GfsWindsForecast {
                        fetched_at,
                        times,
                        stations,
                    });
                    tracing::info!("loaded GFS winds aloft for {count} stations");
                    refresh
                }
                Some(err) => {
                    tracing::warn!("failed to refresh GFS winds aloft: {err}");
                    WINDS_ALOFT_RETRY
                }
            };
            tokio::time::sleep(wait).await;
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
