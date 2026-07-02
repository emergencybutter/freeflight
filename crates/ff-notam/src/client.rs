use thiserror::Error;

pub const DEFAULT_BASE_URL: &str = "https://external-api.faa.gov/notamapi/v1/notams";

#[derive(Debug, Error)]
pub enum NotamError {
    #[error("request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("no ICAO location provided")]
    NoLocation,
}

/// Client for the FAA NOTAM Search API (DESIGN.md §3, §9.2, §12 — flagged
/// there as the flakiest upstream dependency). Unlike weather, this API
/// requires a free `client_id`/`client_secret` pair registered at
/// https://api.faa.gov before any request will succeed.
pub struct NotamClient {
    http: reqwest::Client,
    base_url: String,
    client_id: String,
    client_secret: String,
}

impl NotamClient {
    pub fn new(client_id: impl Into<String>, client_secret: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url: DEFAULT_BASE_URL.to_string(),
            client_id: client_id.into(),
            client_secret: client_secret.into(),
        }
    }

    /// Fetch NOTAMs for a single ICAO location.
    ///
    /// The response schema (`items[].properties.coreNOTAMData...`) is not
    /// modeled as typed structs yet — deliberately, since getting it wrong
    /// silently could hide a runway closure. This returns the raw parsed
    /// JSON so a caller can inspect the current live shape and typed
    /// structs can be added once that's verified against a real response.
    pub async fn fetch_notams_raw(
        &self,
        icao_location: &str,
    ) -> Result<serde_json::Value, NotamError> {
        if icao_location.is_empty() {
            return Err(NotamError::NoLocation);
        }
        let resp = self
            .http
            .get(&self.base_url)
            .header("client_id", &self.client_id)
            .header("client_secret", &self.client_secret)
            .query(&[("icaoLocation", icao_location)])
            .send()
            .await?
            .error_for_status()?;
        Ok(resp.json::<serde_json::Value>().await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn rejects_empty_location_without_a_request() {
        let client = NotamClient::new("id", "secret");
        assert!(matches!(
            client.fetch_notams_raw("").await,
            Err(NotamError::NoLocation)
        ));
    }
}
