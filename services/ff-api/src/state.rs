use ff_notam::NotamClient;
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
            (Ok(id), Ok(secret)) => Some(Arc::new(NotamClient::new(id, secret))),
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
