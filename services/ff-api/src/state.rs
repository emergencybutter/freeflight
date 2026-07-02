use ff_weather::WeatherClient;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub weather: Arc<WeatherClient>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            weather: Arc::new(WeatherClient::new()),
        }
    }
}
