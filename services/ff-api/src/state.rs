use ff_notam::{NotamClient, DEFAULT_API_BASE_URL, DEFAULT_AUTH_URL};
use ff_weather::WeatherClient;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub weather: Arc<WeatherClient>,
    /// `None` unless `FF_NOTAM_CLIENT_ID`/`FF_NOTAM_CLIENT_SECRET` are set
    /// — see `routes::notams` (DESIGN.md §9.2, §12; `ff-notam`'s crate
    /// docs cover why credentials aren't self-service anymore).
    pub notam: Option<Arc<NotamClient>>,
    /// Same directory `ff-etl` publishes cycle bundles under
    /// (`FF_ETL_DATA_DIR`, default `data/`) — see `routes::cycles`.
    pub data_dir: PathBuf,
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
            notam,
            data_dir,
        }
    }
}
