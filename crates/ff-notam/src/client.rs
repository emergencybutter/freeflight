use serde::Deserialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use thiserror::Error;

pub const DEFAULT_AUTH_URL: &str = "https://api-nms.aim.faa.gov/v1/auth/token";
pub const DEFAULT_API_BASE_URL: &str = "https://api-nms.aim.faa.gov/nmsapi";

/// NMS enforces a "spike arrest" of one request per second with no burst
/// allowance, per client credential and across *all* callers, token
/// requests included (confirmed live: two back-to-back calls get a 429
/// `policies.ratelimit.SpikeArrestViolation`). Every upstream call goes
/// through [`Pacer`] so they're spaced at least this far apart; the
/// margin over 1s absorbs jitter on FAA's side.
const MIN_REQUEST_SPACING: Duration = Duration::from_millis(1100);

/// Longest a request will queue for an upstream slot before giving up
/// with [`NotamError::Busy`], so a crowd of callers gets fast "try again"
/// answers instead of requests hanging for a minute.
const MAX_QUEUE_WAIT: Duration = Duration::from_secs(10);

/// How long a location's NOTAMs are reused. NOTAMs change on the scale of
/// minutes to hours, and at one upstream request per second a cache is
/// what lets more than one person use the app at once.
const CACHE_TTL: Duration = Duration::from_secs(5 * 60);

/// How many times a 429 from NMS is retried. Our own pacing should make
/// these rare; they come from something else spending the same
/// credential's budget (another deployment, a dev machine).
const MAX_RATE_LIMIT_RETRIES: u32 = 2;

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
    #[error("NMS rate limit is saturated; too many NOTAM requests queued")]
    Busy,
}

impl NotamError {
    /// True when the failure is NMS's rate limit (or our own queue in
    /// front of it) rather than a real fault, i.e. retrying shortly will
    /// likely work.
    pub fn is_rate_limited(&self) -> bool {
        match self {
            NotamError::Busy => true,
            NotamError::TokenRequestFailed { status, .. }
            | NotamError::ApiRequestFailed { status, .. } => {
                *status == reqwest::StatusCode::TOO_MANY_REQUESTS
            }
            _ => false,
        }
    }
}

/// Spaces upstream requests at least `spacing` apart. Each caller
/// reserves the next free slot under a short (non-async) lock, then
/// sleeps until it, so waiting callers are served in arrival order
/// without holding a lock across an `.await`.
struct Pacer {
    spacing: Duration,
    max_wait: Duration,
    next_slot: Mutex<Option<tokio::time::Instant>>,
}

impl Pacer {
    fn new(spacing: Duration, max_wait: Duration) -> Self {
        Self {
            spacing,
            max_wait,
            next_slot: Mutex::new(None),
        }
    }

    async fn wait_turn(&self) -> Result<(), NotamError> {
        let now = tokio::time::Instant::now();
        let slot = {
            let mut next = self.next_slot.lock().unwrap();
            let slot = next.map_or(now, |n| n.max(now));
            if slot - now > self.max_wait {
                return Err(NotamError::Busy);
            }
            *next = Some(slot + self.spacing);
            slot
        };
        tokio::time::sleep_until(slot).await;
        Ok(())
    }
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
    /// Held while fetching a new token, so concurrent callers that all
    /// find the cache empty share one token request instead of each
    /// spending an NMS rate-limit slot on their own.
    token_refresh: tokio::sync::Mutex<()>,
    pacer: Pacer,
    cache: Mutex<HashMap<String, (Instant, serde_json::Value)>>,
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
            token_refresh: tokio::sync::Mutex::new(()),
            pacer: Pacer::new(MIN_REQUEST_SPACING, MAX_QUEUE_WAIT),
            cache: Mutex::new(HashMap::new()),
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
            token_refresh: tokio::sync::Mutex::new(()),
            pacer: Pacer::new(MIN_REQUEST_SPACING, MAX_QUEUE_WAIT),
            cache: Mutex::new(HashMap::new()),
        }
    }

    fn cached_token(&self) -> Option<String> {
        self.token
            .lock()
            .unwrap()
            .as_ref()
            .filter(|cached| cached.expires_at > Instant::now())
            .map(|cached| cached.access_token.clone())
    }

    /// Lazily fetches and caches a bearer token, refreshing ~60s before
    /// expiry. The token cache itself is only touched in non-async
    /// critical sections; only `token_refresh` is held across the fetch.
    async fn get_token(&self) -> Result<String, NotamError> {
        if let Some(token) = self.cached_token() {
            return Ok(token);
        }
        let _refreshing = self.token_refresh.lock().await;
        // Someone else may have refreshed it while we waited for the lock.
        if let Some(token) = self.cached_token() {
            return Ok(token);
        }

        self.pacer.wait_turn().await?;
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
        let key = icao_location.to_ascii_uppercase();
        if let Some((fetched_at, value)) = self.cache.lock().unwrap().get(&key) {
            if fetched_at.elapsed() < CACHE_TTL {
                return Ok(value.clone());
            }
        }

        let mut retries = 0;
        let value = loop {
            match self.fetch_uncached(&key).await {
                Ok(value) => break value,
                // `Busy` means our own queue is already full; queueing
                // again would only make that worse.
                Err(err)
                    if err.is_rate_limited()
                        && !matches!(err, NotamError::Busy)
                        && retries < MAX_RATE_LIMIT_RETRIES =>
                {
                    retries += 1;
                }
                Err(err) => return Err(err),
            }
        };

        let mut cache = self.cache.lock().unwrap();
        cache.retain(|_, (fetched_at, _)| fetched_at.elapsed() < CACHE_TTL);
        cache.insert(key, (Instant::now(), value.clone()));
        Ok(value)
    }

    async fn fetch_uncached(&self, icao_location: &str) -> Result<serde_json::Value, NotamError> {
        let token = self.get_token().await?;
        self.pacer.wait_turn().await?;
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
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            // A token NMS no longer accepts shouldn't stay cached until its
            // nominal expiry; drop it so the next call re-authenticates.
            *self.token.lock().unwrap() = None;
        }
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

    #[tokio::test(start_paused = true)]
    async fn pacer_spaces_requests_and_rejects_long_queues() {
        let pacer = Pacer::new(Duration::from_secs(1), Duration::from_secs(2));
        let start = tokio::time::Instant::now();
        for i in 0..3 {
            pacer.wait_turn().await.unwrap();
            assert_eq!(start.elapsed(), Duration::from_secs(i));
        }
        // Four concurrent callers: the first two get slots 1s and 2s out;
        // the rest would wait 3s, over the 2s limit, and are turned away.
        let (a, b, c, d) = tokio::join!(
            pacer.wait_turn(),
            pacer.wait_turn(),
            pacer.wait_turn(),
            pacer.wait_turn()
        );
        assert!(a.is_ok() && b.is_ok());
        assert!(matches!(c, Err(NotamError::Busy)));
        assert!(matches!(d, Err(NotamError::Busy)));
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
