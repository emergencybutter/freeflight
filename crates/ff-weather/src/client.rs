use crate::hazards::{Cwa, GAirmet, IntlSigmet, Pirep, Sigmet};
use crate::records::{Datis, Metar, Taf};
use crate::winds_aloft::{gfs_hourly_variables, profile_from_open_meteo, OpenMeteoLocation, StationProfile};
use thiserror::Error;

pub const DEFAULT_BASE_URL: &str = "https://aviationweather.gov/api/data";

/// D-ATIS isn't an aviationweather.gov product — it comes from the free,
/// unauthenticated D-ATIS API (formerly datis.clowd.io, which now 302s
/// here), so it has its own base URL rather than sharing `base_url`.
pub const DATIS_BASE_URL: &str = "https://atis.info/api";

/// Open-Meteo's GFS endpoint — source of winds/temps aloft since NWS
/// retired the FD product (see `winds_aloft` module docs).
pub const OPEN_METEO_GFS_URL: &str = "https://api.open-meteo.com/v1/gfs";

#[derive(Debug, Error)]
pub enum WeatherError {
    #[error("request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("no station ids provided")]
    NoStations,
    #[error("unexpected winds-aloft model response: {0}")]
    WindsAloft(String),
    #[error("failed to read metar cache: {0}")]
    Cache(String),
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

    /// Downloads and parses aviationweather.gov's bulk METAR *cache*
    /// file (`crate::cache`) into a `station_id -> flight_category` map.
    /// Unlike `fetch_metars`, this takes no station list — it pulls every
    /// current worldwide report in one gzipped file, meant to be called
    /// periodically (not per request) so map-marker coloring never fans
    /// out into per-view upstream queries.
    pub async fn fetch_metar_flight_categories(
        &self,
    ) -> Result<std::collections::HashMap<String, String>, WeatherError> {
        let url = format!("{}/{}", crate::cache::CACHE_BASE_URL, crate::cache::METAR_CACHE_FILE);
        let bytes = self
            .http
            .get(&url)
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?;
        crate::cache::parse_metar_flight_categories(&bytes)
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
        // The API returns 204 No Content (empty body) rather than `[]`
        // when none of the requested stations have a current report —
        // confirmed live for a TAF request against a non-towered airport.
        // An empty body isn't valid JSON, so `.json()` would otherwise
        // fail with a confusing "error decoding response body".
        if resp.status() == reqwest::StatusCode::NO_CONTENT {
            return Ok(Vec::new());
        }
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
        // See the matching comment in fetch_metars — small/non-towered
        // airports commonly have no TAF, and the API signals that with
        // 204 No Content instead of an empty JSON array.
        if resp.status() == reqwest::StatusCode::NO_CONTENT {
            return Ok(Vec::new());
        }
        Ok(resp.json::<Vec<Taf>>().await?)
    }

    /// Current Graphical AIRMETs — no station/region filter, same as
    /// aviationweather.gov's own default (all current CONUS records).
    pub async fn fetch_gairmets(&self) -> Result<Vec<GAirmet>, WeatherError> {
        self.fetch_hazard("gairmet").await
    }

    /// Current US domestic/convective SIGMETs. Upstream `/sigmet` now
    /// returns international SIGMETs; the domestic feed is `/airsigmet`.
    pub async fn fetch_sigmets(&self) -> Result<Vec<Sigmet>, WeatherError> {
        self.fetch_hazard("airsigmet").await
    }

    /// Current international/oceanic SIGMETs (DESIGN.md scopes this
    /// project US-only — included for completeness, but likely not
    /// needed by anything else here).
    pub async fn fetch_intl_sigmets(&self) -> Result<Vec<IntlSigmet>, WeatherError> {
        self.fetch_hazard("isigmet").await
    }

    /// Current Center Weather Advisories — no station/region filter,
    /// same all-current-records default as the other hazard endpoints.
    pub async fn fetch_cwas(&self) -> Result<Vec<Cwa>, WeatherError> {
        self.fetch_hazard("cwa").await
    }

    /// Pilot reports within `bbox` (`"lat_min,lon_min,lat_max,lon_max"`,
    /// matching aviationweather.gov's own bbox order — confirmed live,
    /// this is *not* the same corner order as this crate's other
    /// bbox-taking callers use elsewhere in this project). Unlike the
    /// other hazard endpoints, `/pirep` requires either a bbox or a
    /// station id + radial (confirmed live: errors without one), so
    /// this can't go through `fetch_hazard`.
    pub async fn fetch_pireps(&self, bbox: &str) -> Result<Vec<Pirep>, WeatherError> {
        let url = format!("{}/pirep", self.base_url);
        let resp = self
            .http
            .get(&url)
            .query(&[("format", "json"), ("bbox", bbox)])
            .send()
            .await?
            .error_for_status()?;
        if resp.status() == reqwest::StatusCode::NO_CONTENT {
            return Ok(Vec::new());
        }
        Ok(resp.json::<Vec<Pirep>>().await?)
    }

    /// Current D-ATIS for a single airport from datis.clowd.io — one
    /// entry (`"combined"`) at most airports, or two (`"dep"`/`"arr"`)
    /// where the ATIS is split. Airports without Digital ATIS (most
    /// non-major fields) return a 404 + `{"error": …}` object rather than
    /// an array, which this treats as "no ATIS" (empty) rather than an
    /// error, since it's an optional overlay and absence is the norm.
    pub async fn fetch_datis(&self, station_id: &str) -> Result<Vec<Datis>, WeatherError> {
        let url = format!("{DATIS_BASE_URL}/{station_id}");
        let resp = self.http.get(&url).send().await?;
        if !resp.status().is_success() {
            return Ok(Vec::new());
        }
        // A success body is normally the array shape, but the API has
        // been seen to answer 200 with the `{"error": …}` object too;
        // fall back to empty rather than surfacing a decode error.
        Ok(resp.json::<Vec<Datis>>().await.unwrap_or_default())
    }

    async fn fetch_hazard<T: serde::de::DeserializeOwned>(
        &self,
        endpoint: &str,
    ) -> Result<Vec<T>, WeatherError> {
        let url = format!("{}/{endpoint}", self.base_url);
        let resp = self
            .http
            .get(&url)
            .query(&[("format", "json")])
            .send()
            .await?
            .error_for_status()?;
        // Same 204-No-Content-for-no-current-records behavior as
        // fetch_metars/fetch_tafs — not yet confirmed live for these
        // three endpoints specifically (CONUS/oceanic hazards are rarely
        // all-clear), but handling it defensively costs nothing.
        if resp.status() == reqwest::StatusCode::NO_CONTENT {
            return Ok(Vec::new());
        }
        Ok(resp.json::<Vec<T>>().await?)
    }

    /// Hourly GFS vertical profiles (see `winds_aloft`) for `stations`
    /// (`(ident, lat, lon)`), `forecast_hours` hours from the current
    /// hour, in one multi-location Open-Meteo request. Returns the time
    /// axis and one profile per station, in input order.
    ///
    /// Open-Meteo's free tier weighs a call by locations × (variables /
    /// 10) — ~5.2 per station here — against 600/min and 10,000/day, so
    /// callers should chunk large station lists and cache the result.
    pub async fn fetch_gfs_profiles(
        &self,
        stations: &[(&str, f64, f64)],
        forecast_hours: u32,
    ) -> Result<(Vec<i64>, Vec<StationProfile>), WeatherError> {
        let join = |f: fn(&(&str, f64, f64)) -> f64| {
            stations.iter().map(|s| format!("{:.4}", f(s))).collect::<Vec<_>>().join(",")
        };
        let locations: Vec<OpenMeteoLocation> = self
            .http
            .get(OPEN_METEO_GFS_URL)
            .query(&[
                ("latitude", join(|s| s.1)),
                ("longitude", join(|s| s.2)),
                ("hourly", gfs_hourly_variables()),
                ("wind_speed_unit", "kn".to_string()),
                ("timeformat", "unixtime".to_string()),
                ("forecast_hours", forecast_hours.to_string()),
            ])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        if locations.len() != stations.len() {
            return Err(WeatherError::WindsAloft(format!(
                "asked for {} locations, got {}",
                stations.len(),
                locations.len()
            )));
        }
        let mut times = Vec::new();
        let mut profiles = Vec::with_capacity(stations.len());
        for (station, loc) in stations.iter().zip(&locations) {
            let (t, profile) = profile_from_open_meteo(*station, loc)
                .ok_or_else(|| WeatherError::WindsAloft(format!("{}: no time axis", station.0)))?;
            times = t;
            profiles.push(profile);
        }
        Ok((times, profiles))
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
