use crate::state::AppState;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use ff_weather::{StationWindsAloft, WindsAloftBulletin};
use serde::{Deserialize, Serialize};

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
    /// Resolved against the current cycle bundle so clients can do a
    /// nearest-station lookup for a route leg (DESIGN.md §9.3's wind
    /// correction) — the raw NWS product only carries station idents,
    /// not coordinates. Best-effort: `None` if no cycle is published
    /// yet, or this particular ident doesn't resolve to anything —
    /// winds-aloft stations are usually airport or navaid idents, so
    /// those two tables cover the vast majority.
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

/// Best-effort station-ident → coordinate resolution against the
/// current cycle bundle. Tries airport (by ICAO with a "K" prefix
/// reconstructed, FAA id, or IATA), then navaid, then waypoint — the
/// same fallback shape `procedure_detail`'s fix resolution already
/// uses elsewhere in this server, minus the runway-end step (a winds
/// station is never a runway pseudo-fix). Silently resolves nothing
/// (not an error) if no cycle is published yet: a missing bundle
/// degrades winds-aloft to "no coordinates," not a failed response.
fn resolve_station_coords(
    data_dir: &std::path::Path,
    station_ids: &[&str],
) -> std::collections::HashMap<String, (f64, f64)> {
    let mut coords = std::collections::HashMap::new();
    let Ok(Some(bundle_path)) = ff_etl::publish::latest_bundle_path(data_dir) else {
        return coords;
    };
    let Ok(conn) = rusqlite::Connection::open(bundle_path) else {
        return coords;
    };
    for &id in station_ids {
        let icao_guess = format!("K{id}");
        let coord = conn
            .query_row(
                "SELECT lat, lon FROM airport WHERE icao = ?1 OR faa_id = ?2 OR iata = ?2",
                rusqlite::params![icao_guess, id],
                |row| Ok((row.get::<_, f64>(0)?, row.get::<_, f64>(1)?)),
            )
            .or_else(|_| {
                conn.query_row(
                    "SELECT lat, lon FROM navaid WHERE ident = ?1",
                    [id],
                    |row| Ok((row.get::<_, f64>(0)?, row.get::<_, f64>(1)?)),
                )
            })
            .or_else(|_| {
                conn.query_row(
                    "SELECT lat, lon FROM waypoint WHERE ident = ?1",
                    [id],
                    |row| Ok((row.get::<_, f64>(0)?, row.get::<_, f64>(1)?)),
                )
            });
        if let Ok(coord) = coord {
            coords.insert(id.to_string(), coord);
        }
    }
    coords
}

fn enrich(bulletin: WindsAloftBulletin, data_dir: &std::path::Path) -> EnrichedBulletin {
    let ids: Vec<&str> = bulletin
        .stations
        .iter()
        .map(|s| s.station_id.as_str())
        .collect();
    let coords = resolve_station_coords(data_dir, &ids);
    EnrichedBulletin {
        data_based_on: bulletin.data_based_on,
        valid_time: bulletin.valid_time,
        for_use: bulletin.for_use,
        stations: bulletin
            .stations
            .into_iter()
            .map(|station| {
                let coord = coords.get(&station.station_id).copied();
                EnrichedStation {
                    station,
                    lat: coord.map(|(lat, _)| lat),
                    lon: coord.map(|(_, lon)| lon),
                }
            })
            .collect(),
    }
}

/// Proxies aviationweather.gov's winds/temps aloft forecast — the one
/// product `ff-weather` gets as fixed-width text rather than JSON (see
/// `ff_weather::winds_aloft`), reserialized here as JSON like everything
/// else this server proxies. Each station gets a best-effort resolved
/// coordinate (see `resolve_station_coords`) so the web client can pick
/// the nearest station to a route leg for wind correction.
pub async fn get_winds_aloft(
    State(state): State<AppState>,
    Query(query): Query<WindsAloftQuery>,
) -> Response {
    match state
        .weather
        .fetch_winds_aloft(&query.level, &query.fcst, &query.region)
        .await
    {
        Ok(bulletin) => {
            let data_dir = state.data_dir.clone();
            let enriched = tokio::task::spawn_blocking(move || enrich(bulletin, &data_dir))
                .await
                .expect("blocking task panicked");
            Json(enriched).into_response()
        }
        Err(err) => (StatusCode::BAD_GATEWAY, err.to_string()).into_response(),
    }
}
