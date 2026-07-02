use crate::records::{Metar, Taf};
use thiserror::Error;

pub const DEFAULT_BASE_URL: &str = "https://aviationweather.gov/api/data";

#[derive(Debug, Error)]
pub enum WeatherError {
    #[error("request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("no station ids provided")]
    NoStations,
}

/// Client for the free, unauthenticated aviationweather.gov Data API
/// (DESIGN.md §3, §9.2). No API key required; be a good citizen and
/// batch station ids into one request rather than one request per
/// station.
pub struct WeatherClient {
    http: reqwest::Client,
    base_url: String,
}

impl Default for WeatherClient {
    fn default() -> Self {
        Self::new()
    }
}

impl WeatherClient {
    pub fn new() -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url: DEFAULT_BASE_URL.to_string(),
        }
    }

    /// Point at an alternate base URL — used to route through `ff-api`'s
    /// proxy/cache (DESIGN.md §4) instead of hitting aviationweather.gov
    /// directly from a client.
    pub fn with_base_url(base_url: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url: base_url.into(),
        }
    }

    pub async fn fetch_metars(&self, station_ids: &[&str]) -> Result<Vec<Metar>, WeatherError> {
        if station_ids.is_empty() {
            return Err(WeatherError::NoStations);
        }
        let url = format!("{}/metar", self.base_url);
        let resp = self
            .http
            .get(&url)
            .query(&[
                ("ids", station_ids.join(",")),
                ("format", "json".to_string()),
            ])
            .send()
            .await?
            .error_for_status()?;
        Ok(resp.json::<Vec<Metar>>().await?)
    }

    pub async fn fetch_tafs(&self, station_ids: &[&str]) -> Result<Vec<Taf>, WeatherError> {
        if station_ids.is_empty() {
            return Err(WeatherError::NoStations);
        }
        let url = format!("{}/taf", self.base_url);
        let resp = self
            .http
            .get(&url)
            .query(&[
                ("ids", station_ids.join(",")),
                ("format", "json".to_string()),
            ])
            .send()
            .await?
            .error_for_status()?;
        Ok(resp.json::<Vec<Taf>>().await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn rejects_empty_station_list_without_a_request() {
        let client = WeatherClient::new();
        assert!(matches!(
            client.fetch_metars(&[]).await,
            Err(WeatherError::NoStations)
        ));
        assert!(matches!(
            client.fetch_tafs(&[]).await,
            Err(WeatherError::NoStations)
        ));
    }
}
