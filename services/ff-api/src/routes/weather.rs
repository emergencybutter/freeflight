use crate::state::AppState;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use ff_weather::StationWindsAloft;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct PirepQuery {
    /// `minLon,minLat,maxLon,maxLat` — same convention as this server's
    /// other bbox-taking routes (`/data/airports`, `/data/airspace`),
    /// *not* aviationweather.gov's own `/pirep` bbox order
    /// (`lat_min,lon_min,lat_max,lon_max`); converted below so clients
    /// don't need to special-case this one endpoint.
    pub bbox: String,
}

#[derive(Debug, Deserialize)]
pub struct StationQuery {
    /// Comma-separated ICAO station ids, e.g. "KSFO,KOAK".
    pub ids: String,
}

fn parse_ids(raw: &str) -> Vec<&str> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect()
}

/// Proxies aviationweather.gov METAR (DESIGN.md §4, §9.2) so clients don't
/// need to embed/refresh an upstream base URL and get a consistent place
/// to add caching later.
pub async fn get_metars(
    State(state): State<AppState>,
    Query(query): Query<StationQuery>,
) -> Response {
    let ids = parse_ids(&query.ids);
    match state.weather.fetch_metars(&ids).await {
        Ok(metars) => Json(metars).into_response(),
        Err(err) => (StatusCode::BAD_GATEWAY, err.to_string()).into_response(),
    }
}

/// Serves the in-memory METAR flight-category map (`station_id ->
/// "VFR"/"MVFR"/"IFR"/"LIFR"`) kept fresh by the background task in
/// `main::spawn_flight_category_refresh`. The web client fetches this
/// once (and refreshes on an interval) to color every visible airport
/// marker, rather than firing a METAR query on every pan/zoom. Returns
/// `{}` until the first successful upstream load.
pub async fn get_flight_categories(State(state): State<AppState>) -> Response {
    let cache = state.flight_categories.read().await;
    Json(&cache.categories).into_response()
}

pub async fn get_tafs(
    State(state): State<AppState>,
    Query(query): Query<StationQuery>,
) -> Response {
    let ids = parse_ids(&query.ids);
    match state.weather.fetch_tafs(&ids).await {
        Ok(tafs) => Json(tafs).into_response(),
        Err(err) => (StatusCode::BAD_GATEWAY, err.to_string()).into_response(),
    }
}

/// Proxies datis.clowd.io D-ATIS for a single airport (the first of
/// `ids`). Airports without Digital ATIS just return an empty array — see
/// `WeatherClient::fetch_datis` — so the caller treats absence as "no
/// ATIS", not an error.
pub async fn get_datis(
    State(state): State<AppState>,
    Query(query): Query<StationQuery>,
) -> Response {
    let ids = parse_ids(&query.ids);
    let Some(id) = ids.first() else {
        return (StatusCode::BAD_REQUEST, "no station id provided").into_response();
    };
    match state.weather.fetch_datis(id).await {
        Ok(datis) => Json(datis).into_response(),
        Err(err) => (StatusCode::BAD_GATEWAY, err.to_string()).into_response(),
    }
}

/// Proxies aviationweather.gov's Graphical AIRMET — no station filter,
/// same as upstream (all current CONUS records).
pub async fn get_gairmets(State(state): State<AppState>) -> Response {
    match state.weather.fetch_gairmets().await {
        Ok(records) => Json(records).into_response(),
        Err(err) => (StatusCode::BAD_GATEWAY, err.to_string()).into_response(),
    }
}

/// Proxies aviationweather.gov's US domestic/convective SIGMET.
pub async fn get_sigmets(State(state): State<AppState>) -> Response {
    match state.weather.fetch_sigmets().await {
        Ok(records) => Json(records).into_response(),
        Err(err) => (StatusCode::BAD_GATEWAY, err.to_string()).into_response(),
    }
}

/// Proxies aviationweather.gov's international/oceanic SIGMET.
pub async fn get_intl_sigmets(State(state): State<AppState>) -> Response {
    match state.weather.fetch_intl_sigmets().await {
        Ok(records) => Json(records).into_response(),
        Err(err) => (StatusCode::BAD_GATEWAY, err.to_string()).into_response(),
    }
}

/// Proxies aviationweather.gov's Center Weather Advisories — no
/// station/region filter, same as upstream (all current records).
pub async fn get_cwas(State(state): State<AppState>) -> Response {
    match state.weather.fetch_cwas().await {
        Ok(records) => Json(records).into_response(),
        Err(err) => (StatusCode::BAD_GATEWAY, err.to_string()).into_response(),
    }
}

/// Proxies aviationweather.gov's PIREPs for a bounding box — unlike the
/// other hazard endpoints, upstream requires one (see `PirepQuery`).
pub async fn get_pireps(
    State(state): State<AppState>,
    Query(query): Query<PirepQuery>,
) -> Response {
    let parts: Vec<f64> = query
        .bbox
        .split(',')
        .map_while(|p| p.trim().parse().ok())
        .collect();
    let [min_lon, min_lat, max_lon, max_lat] = match parts[..] {
        [a, b, c, d] => [a, b, c, d],
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                "bbox must be minLon,minLat,maxLon,maxLat",
            )
                .into_response()
        }
    };
    let upstream_bbox = format!("{min_lat},{min_lon},{max_lat},{max_lon}");
    match state.weather.fetch_pireps(&upstream_bbox).await {
        Ok(records) => Json(records).into_response(),
        Err(err) => (StatusCode::BAD_GATEWAY, err.to_string()).into_response(),
    }
}

#[derive(Debug, Deserialize)]
pub struct WindsAloftQuery {
    #[serde(default = "default_level")]
    pub level: String,
    #[serde(default = "default_fcst")]
    pub fcst: String,
    #[serde(default = "default_region")]
    pub region: String,
}

fn default_level() -> String {
    "low".to_string()
}

fn default_fcst() -> String {
    "06".to_string()
}

fn default_region() -> String {
    "all".to_string()
}

#[derive(Debug, Serialize)]
struct EnrichedStation {
    #[serde(flatten)]
    station: StationWindsAloft,
    /// Station location, so clients can do a nearest-station lookup for
    /// a route leg (DESIGN.md §9.3's wind correction). Always set now
    /// (from `ff_weather::FD_STATIONS`); kept optional for wire
    /// compatibility with clients written against the old FD proxy.
    lat: Option<f64>,
    lon: Option<f64>,
}

#[derive(Debug, Serialize)]
struct EnrichedBulletin {
    data_based_on: String,
    valid_time: String,
    for_use: String,
    stations: Vec<EnrichedStation>,
}

/// Maps the old FD forecast product (`06`/`12`/`24`) onto an offset from
/// now. FD's 06 product was the "use now" forecast and 12/24 were for
/// roughly 6 and 18 hours later, so those are the offsets used.
fn fcst_offset_hours(fcst: &str) -> Option<i64> {
    match fcst {
        "06" | "6" => Some(0),
        "12" => Some(6),
        "24" => Some(18),
        _ => None,
    }
}

/// Winds/temps aloft at the old FD station set, served from the GFS
/// snapshot the background task keeps in `state.winds_aloft` (see
/// `main::spawn_winds_aloft_refresh` and `ff_weather::winds_aloft`).
/// Same JSON shape as when this proxied NWS's retired FD bulletin.
/// Only the "low" product over all of CONUS exists now, which is the
/// only thing either client ever requested.
pub async fn get_winds_aloft(
    State(state): State<AppState>,
    Query(query): Query<WindsAloftQuery>,
) -> Response {
    if query.level != "low" || query.region != "all" {
        return (
            StatusCode::BAD_REQUEST,
            "only level=low&region=all is available (NWS retired the FD product; winds aloft now come from GFS)",
        )
            .into_response();
    }
    let Some(offset) = fcst_offset_hours(&query.fcst) else {
        return (StatusCode::BAD_REQUEST, "fcst must be 06, 12 or 24").into_response();
    };
    let cache = state.winds_aloft.read().await;
    let Some(forecast) = cache.as_ref() else {
        return (StatusCode::SERVICE_UNAVAILABLE, "winds aloft not loaded yet").into_response();
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    let Some(bulletin) = ff_weather::bulletin_at(forecast, now + offset * 3600) else {
        return (StatusCode::SERVICE_UNAVAILABLE, "winds aloft forecast is empty").into_response();
    };
    let stations = bulletin
        .stations
        .into_iter()
        .zip(&forecast.stations)
        .map(|(station, profile)| EnrichedStation {
            station,
            lat: Some(profile.lat),
            lon: Some(profile.lon),
        })
        .collect();
    Json(EnrichedBulletin {
        data_based_on: bulletin.data_based_on,
        valid_time: bulletin.valid_time,
        for_use: bulletin.for_use,
        stations,
    })
    .into_response()
}
