use serde::Deserialize;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use thiserror::Error;

pub const DEFAULT_AUTH_URL: &str = "https://api-nms.aim.faa.gov/v1/auth/token";
pub const DEFAULT_API_BASE_URL: &str = "https://api-nms.aim.faa.gov/nmsapi";

#[derive(Debug, Error)]
pub enum NotamError {
    #[error("request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("no ICAO location provided")]
    NoLocation,
    #[error("NMS token request failed ({status}): {body}")]
    TokenRequestFailed {
        status: reqwest::StatusCode,
        body: String,
    },
    #[error("NMS API request failed ({status}): {body}")]
    ApiRequestFailed {
        status: reqwest::StatusCode,
        body: String,
    },
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    // FAA's production API returns this as a JSON number, but the CGI
    // staging/SIT gateways return it as a quoted string (e.g. "1799") —
    // accept either so the same client works against both.
    #[serde(deserialize_with = "de_u64_or_string")]
    expires_in: u64,
}

fn de_u64_or_string<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum U64OrString {
        U64(u64),
        String(String),
    }
    match U64OrString::deserialize(deserializer)? {
        U64OrString::U64(n) => Ok(n),
        U64OrString::String(s) => s.parse().map_err(serde::de::Error::custom),
    }
}

struct CachedToken {
    access_token: String,
    expires_at: Instant,
}

/// Client for the FAA NOTAM Management Service (NMS) API
/// (<https://nms.aim.faa.gov/>, DESIGN.md §3, §9.2, §12).
///
/// This replaces the old FAA NOTAM Search API
/// (`external-api.faa.gov/notamapi/v1/notams`), which DESIGN.md §12
/// flagged as the flakiest upstream dependency — it has since been
/// retired outright (confirmed live: that endpoint now 404s with "No
/// context-path matches the request URI" on FAA's gateway). The
/// replacement also changed how you get credentials: no more
/// self-service portal signup, `client_id`/`client_secret` must be
/// requested by emailing NOTAMS@faa.gov. Base URLs and the OAuth2
/// `client_credentials` flow below are reverse-engineered from a
/// third-party client (`faa-nms-api` on npm, itself derived from FAA's
/// OpenAPI spec) and confirmed reachable (the token endpoint and
/// `/nmsapi/notams` both respond live, the latter with a real "Access
/// Token is Invalid or Expired" 401) — but not yet exercised with a real
/// `client_id`/`client_secret`, so treat this as unvalidated until it is.
pub struct NotamClient {
    http: reqwest::Client,
    auth_url: String,
    api_base_url: String,
    client_id: String,
    client_secret: String,
    token: Arc<Mutex<Option<CachedToken>>>,
}

impl NotamClient {
    pub fn new(client_id: impl Into<String>, client_secret: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            auth_url: DEFAULT_AUTH_URL.to_string(),
            api_base_url: DEFAULT_API_BASE_URL.to_string(),
            client_id: client_id.into(),
            client_secret: client_secret.into(),
            token: Arc::new(Mutex::new(None)),
        }
    }

    /// Point at alternate auth/API base URLs — used to target FAA's
    /// `staging`/`fit` environments instead of production.
    pub fn with_urls(
        client_id: impl Into<String>,
        client_secret: impl Into<String>,
        auth_url: impl Into<String>,
        api_base_url: impl Into<String>,
    ) -> Self {
        Self {
            http: reqwest::Client::new(),
            auth_url: auth_url.into(),
            api_base_url: api_base_url.into(),
            client_id: client_id.into(),
            client_secret: client_secret.into(),
            token: Arc::new(Mutex::new(None)),
        }
    }

    /// Lazily fetches and caches a bearer token, refreshing ~60s before
    /// expiry. No lock is held across an `.await` — the cache is only
    /// consulted/updated in non-async critical sections.
    async fn get_token(&self) -> Result<String, NotamError> {
        if let Some(cached) = self.token.lock().unwrap().as_ref() {
            if cached.expires_at > Instant::now() {
                return Ok(cached.access_token.clone());
            }
        }

        let resp = self
            .http
            .post(&self.auth_url)
            .basic_auth(&self.client_id, Some(&self.client_secret))
            .form(&[("grant_type", "client_credentials")])
            .send()
            .await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(NotamError::TokenRequestFailed { status, body });
        }
        let token: TokenResponse = resp.json().await?;
        let expires_at = Instant::now() + Duration::from_secs(token.expires_in.saturating_sub(60));
        let access_token = token.access_token;
        *self.token.lock().unwrap() = Some(CachedToken {
            access_token: access_token.clone(),
            expires_at,
        });
        Ok(access_token)
    }

    /// Fetch NOTAMs for a single ICAO/FAA location as GeoJSON.
    ///
    /// The individual NOTAM record shape (`data.geojson[]`) is not
    /// modeled as typed structs yet — deliberately, since getting it
    /// wrong silently could hide a runway closure, and this crate has no
    /// real credentials to validate a live response against yet (see the
    /// struct docs above). This returns the raw parsed JSON so a caller
    /// can inspect the current live shape and typed structs can be added
    /// once that's verified.
    pub async fn fetch_notams_raw(
        &self,
        icao_location: &str,
    ) -> Result<serde_json::Value, NotamError> {
        if icao_location.is_empty() {
            return Err(NotamError::NoLocation);
        }
        let token = self.get_token().await?;
        let url = format!("{}/v1/notams", self.api_base_url);
        let resp = self
            .http
            .get(&url)
            .bearer_auth(token)
            .header("nmsResponseFormat", "GEOJSON")
            .header("Accept", "application/json")
            .query(&[("location", icao_location)])
            .send()
            .await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(NotamError::ApiRequestFailed { status, body });
        }
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

    #[test]
    fn token_response_accepts_string_or_numeric_expires_in() {
        // CGI staging returns expires_in as a quoted string...
        let staging: TokenResponse =
            serde_json::from_str(r#"{"access_token":"abc","expires_in":"1799"}"#).unwrap();
        assert_eq!(staging.expires_in, 1799);
        assert_eq!(staging.access_token, "abc");
        // ...production returns it as a JSON number.
        let prod: TokenResponse =
            serde_json::from_str(r#"{"access_token":"xyz","expires_in":1799}"#).unwrap();
        assert_eq!(prod.expires_in, 1799);
    }
}
