//! Blocking variant of [`crate::client`], behind the `blocking` feature.
//!
//! `ff-etl` is entirely synchronous — it uses `reqwest::blocking`
//! throughout — and pulling an async runtime into it for one data source
//! would be the tail wagging the dog. This mirrors how `reqwest` itself
//! offers both, and keeps the knowledge of openAIP's paging in the crate
//! that knows about openAIP rather than in the ETL.
//!
//! The decoding is shared: both clients hand the same wire types to
//! [`crate::convert`].

use crate::model::{Airport, Airspace, Navaid, Page};
use crate::OpenAipError;
use serde::de::DeserializeOwned;

/// Same limits as the async client — see [`crate::client`].
const DEFAULT_BASE_URL: &str = "https://api.core.openaip.net/api";
const PAGE_LIMIT: u32 = 1000;
const MAX_PAGES: u32 = 100;

pub struct BlockingOpenAipClient {
    http: reqwest::blocking::Client,
    api_key: String,
    base_url: String,
}

impl BlockingOpenAipClient {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            http: reqwest::blocking::Client::new(),
            api_key: api_key.into(),
            base_url: DEFAULT_BASE_URL.to_string(),
        }
    }

    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    pub fn airports(&self, country: &str) -> Result<Vec<Airport>, OpenAipError> {
        self.fetch_all("airports", country)
    }

    pub fn navaids(&self, country: &str) -> Result<Vec<Navaid>, OpenAipError> {
        self.fetch_all("navaids", country)
    }

    pub fn airspaces(&self, country: &str) -> Result<Vec<Airspace>, OpenAipError> {
        self.fetch_all("airspaces", country)
    }

    fn fetch_all<T: DeserializeOwned>(
        &self,
        endpoint: &str,
        country: &str,
    ) -> Result<Vec<T>, OpenAipError> {
        let mut all = Vec::new();
        let mut page = 1;
        loop {
            let response = self
                .http
                .get(format!("{}/{endpoint}", self.base_url))
                // Header rather than the query parameter the API also
                // accepts: a key in a URL ends up in logs and history.
                .header("x-openaip-api-key", &self.api_key)
                .query(&[
                    ("country", country),
                    ("limit", &PAGE_LIMIT.to_string()),
                    ("page", &page.to_string()),
                ])
                .send()?;

            let status = response.status();
            if !status.is_success() {
                let body = response.text().unwrap_or_default();
                return Err(OpenAipError::Http {
                    endpoint: endpoint.to_string(),
                    status: status.as_u16(),
                    body: body.chars().take(200).collect(),
                });
            }

            let decoded: Page<T> = response.json()?;
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
