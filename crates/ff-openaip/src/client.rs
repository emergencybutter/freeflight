//! Paged REST client for openAIP.
//!
//! Kept deliberately small: fetch pages, hand the wire types to
//! [`crate::convert`]. Everything that decides *meaning* lives there, so
//! this module can be skipped entirely by a caller that already has the
//! JSON (a cached export, a fixture, a test).

use crate::model::{Airport, Airspace, Navaid, Page};
use crate::OpenAipError;
use serde::de::DeserializeOwned;

const DEFAULT_BASE_URL: &str = "https://api.core.openaip.net/api";

/// openAIP caps page size; 1000 is what the API accepts and keeps the
/// number of round trips low (Germany is 1364 airports = 2 pages).
const PAGE_LIMIT: u32 = 1000;

/// A guard against a paging bug turning into an unbounded fetch loop
/// against someone else's API. Germany's largest collection is under two
/// pages; 100 is far beyond any national dataset.
const MAX_PAGES: u32 = 100;

/// Authenticated openAIP API client.
///
/// The key is a secret: it comes from the environment (`ff-etl` reads
/// `FF_OPENAIP_API_KEY`) and is sent as the `x-openaip-api-key` header
/// rather than the query parameter the API also accepts — a key in a URL
/// ends up in logs, proxies and shell history.
pub struct OpenAipClient {
    http: reqwest::Client,
    api_key: String,
    base_url: String,
}

impl OpenAipClient {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            api_key: api_key.into(),
            base_url: DEFAULT_BASE_URL.to_string(),
        }
    }

    /// Point at a different base URL (a mock server in tests).
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    /// Every airport for a country, following pagination. `country` is
    /// openAIP's two-letter code (`"DE"`), not an ICAO region.
    pub async fn airports(&self, country: &str) -> Result<Vec<Airport>, OpenAipError> {
        self.fetch_all("airports", country).await
    }

    pub async fn navaids(&self, country: &str) -> Result<Vec<Navaid>, OpenAipError> {
        self.fetch_all("navaids", country).await
    }

    pub async fn airspaces(&self, country: &str) -> Result<Vec<Airspace>, OpenAipError> {
        self.fetch_all("airspaces", country).await
    }

    async fn fetch_all<T: DeserializeOwned>(
        &self,
        endpoint: &str,
        country: &str,
    ) -> Result<Vec<T>, OpenAipError> {
        let mut all = Vec::new();
        let mut page = 1;
        loop {
            let url = format!("{}/{endpoint}", self.base_url);
            let response = self
                .http
                .get(&url)
                .header("x-openaip-api-key", &self.api_key)
                .query(&[
                    ("country", country),
                    ("limit", &PAGE_LIMIT.to_string()),
                    ("page", &page.to_string()),
                ])
                .send()
                .await?;

            let status = response.status();
            if !status.is_success() {
                // Never echo the key, and don't assume a body exists.
                let body = response.text().await.unwrap_or_default();
                return Err(OpenAipError::Http {
                    endpoint: endpoint.to_string(),
                    status: status.as_u16(),
                    body: body.chars().take(200).collect(),
                });
            }

            let decoded: Page<T> = response.json().await?;
            let total_pages = decoded.total_pages;
            all.extend(decoded.items);

            if page >= total_pages || page >= MAX_PAGES {
                break;
            }
            page += 1;
        }
        Ok(all)
    }
}
