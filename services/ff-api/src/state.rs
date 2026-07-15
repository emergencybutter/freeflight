use crate::routes::auth::AuthState;
use ff_notam::{NotamClient, DEFAULT_API_BASE_URL, DEFAULT_AUTH_URL};
use ff_weather::WeatherClient;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::RwLock;

/// In-memory snapshot of the bulk METAR-cache flight categories, kept
/// current by a background refresh task (see `main::spawn_flight_category_refresh`)
/// and served at `/weather/flightcat`. Empty until the first successful
/// load; a failed refresh leaves the previous snapshot in place rather
/// than blanking it.
#[derive(Default)]
pub struct FlightCategoryCache {
    /// `station_id` (ICAO) -> `"VFR"`/`"MVFR"`/`"IFR"`/`"LIFR"`.
    pub categories: HashMap<String, String>,
    /// When `categories` was last successfully replaced — `None` before
    /// the first load.
    pub updated: Option<SystemTime>,
}

#[derive(Clone)]
pub struct AppState {
    pub weather: Arc<WeatherClient>,
    /// Shared, periodically-refreshed METAR flight categories used to
    /// color airport markers without a per-view upstream query.
    pub flight_categories: Arc<RwLock<FlightCategoryCache>>,
    /// `None` unless `FF_NOTAM_CLIENT_ID`/`FF_NOTAM_CLIENT_SECRET` are set
    /// — see `routes::notams` (DESIGN.md §9.2, §12; `ff-notam`'s crate
    /// docs cover why credentials aren't self-service anymore).
    pub notam: Option<Arc<NotamClient>>,
    /// Same directory `ff-etl` publishes cycle bundles under
    /// (`FF_ETL_DATA_DIR`, default `data/`) — see `routes::cycles`.
    pub data_dir: PathBuf,
    /// Shared client for `routes::dtpp`'s d-TPP plate PDF proxy — a plain
    /// `reqwest::Client` (not one of `WeatherClient`/`NotamClient`'s
    /// wrapped ones) since this route doesn't decode/cache a structured
    /// response, just re-serves the upstream bytes. Built once and cloned
    /// (cheap — internally `Arc`-backed) rather than per-request, so
    /// connections to aeronav.faa.gov can be reused.
    pub http: reqwest::Client,
    /// OAuth sign-in config + in-memory session store (see `routes::auth`).
    /// Sign-in is off unless provider credentials are set in the
    /// environment; `AuthState` is always present so the routes can report
    /// "no providers configured" cleanly.
    pub auth: Arc<AuthState>,
}

impl Default for AppState {
    fn default() -> Self {
        let notam = match (
            std::env::var("FF_NOTAM_CLIENT_ID"),
            std::env::var("FF_NOTAM_CLIENT_SECRET"),
        ) {
            (Ok(id), Ok(secret)) => {
                // Default to the production NMS host, but allow overriding
                // the auth/API base URLs so the same build can point at
                // FAA's staging/SIT (cgifederal-aim.com) environments —
                // whichever the issued client_id/secret belong to.
                let auth_url =
                    std::env::var("FF_NOTAM_AUTH_URL").unwrap_or_else(|_| DEFAULT_AUTH_URL.to_string());
                let api_base_url = std::env::var("FF_NOTAM_API_BASE_URL")
                    .unwrap_or_else(|_| DEFAULT_API_BASE_URL.to_string());
                Some(Arc::new(NotamClient::with_urls(id, secret, auth_url, api_base_url)))
            }
            _ => None,
        };
        let data_dir =
            PathBuf::from(std::env::var("FF_ETL_DATA_DIR").unwrap_or_else(|_| "data".to_string()));
        Self {
            weather: Arc::new(WeatherClient::new()),
            flight_categories: Arc::new(RwLock::new(FlightCategoryCache::default())),
            notam,
            data_dir,
            http: reqwest::Client::new(),
            auth: Arc::new(AuthState::from_env()),
        }
    }
}
