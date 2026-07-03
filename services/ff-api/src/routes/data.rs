//! The `/data/*` JSON query endpoints the web thin client is built on
//! (DESIGN.md §4.1, §8): `ff-api` opens the latest published cycle
//! bundle server-side and answers per-view queries, instead of shipping
//! the whole SQLite file to the browser.
//!
//! Field names mirror the `ff-storage` schema columns one-to-one — the
//! web client's TypeScript row types were written against those column
//! names back when it queried the bundle itself with sql.js, and keeping
//! the wire shape identical meant that switch didn't touch the types.
//!
//! Each request re-resolves `latest.json` and opens the bundle fresh:
//! `ff-etl` can publish a new cycle while this server is running and the
//! next request just picks it up — no restart, no cache invalidation to
//! get wrong. Opening a SQLite file is cheap at this scale; revisit with
//! a connection pool keyed on cycle id if profiling ever says otherwise.
use crate::state::AppState;
use axum::extract::{Path as UrlPath, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Serialize)]
pub struct AirportRow {
    pub icao: String,
    pub faa_id: Option<String>,
    pub iata: Option<String>,
    pub name: String,
    pub lat: f64,
    pub lon: f64,
    pub elevation_ft: i64,
    pub airport_type: String,
}

#[derive(Debug, Serialize)]
pub struct RunwayRow {
    pub airport_icao: String,
    pub ident: String,
    pub length_ft: i64,
    pub width_ft: i64,
    pub surface: String,
    pub le_ident: String,
    pub le_lat: f64,
    pub le_lon: f64,
    pub le_heading_deg: f64,
    pub he_ident: String,
    pub he_lat: f64,
    pub he_lon: f64,
    pub he_heading_deg: f64,
}

#[derive(Debug, Serialize)]
pub struct FrequencyRow {
    pub airport_icao: String,
    pub kind: String,
    pub freq_mhz: f64,
    pub remarks: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AirportDetail {
    #[serde(flatten)]
    pub airport: AirportRow,
    pub runways: Vec<RunwayRow>,
    pub frequencies: Vec<FrequencyRow>,
}

#[derive(Debug, Serialize)]
pub struct ProcedureRow {
    pub id: String,
    pub airport_icao: String,
    pub kind: String,
    pub ident: String,
    pub runway_ident: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ProcedureLegRow {
    pub seq: i64,
    pub path_and_term: String,
    pub fix_ident: Option<String>,
    pub course_deg: Option<f64>,
    pub altitude_constraint: Option<String>,
    pub speed_constraint: Option<String>,
    pub turn_direction: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ProcedureTransitionDetail {
    pub id: String,
    pub ident: String,
    pub kind: String,
    pub legs: Vec<ProcedureLegRow>,
}

#[derive(Debug, Serialize)]
pub struct FixCoord {
    pub lat: f64,
    pub lon: f64,
}

#[derive(Debug, Serialize)]
pub struct ProcedureDetail {
    #[serde(flatten)]
    pub procedure: ProcedureRow,
    pub transitions: Vec<ProcedureTransitionDetail>,
    /// Coordinates for every `fix_ident` referenced by this procedure's
    /// legs that resolves to a waypoint or navaid in the cycle bundle —
    /// runway-threshold pseudo-fixes (e.g. "RW28L") simply won't appear.
    /// Resolved server-side so the client doesn't need its own
    /// navaid/waypoint queries just to draw a procedure line.
    pub fixes: HashMap<String, FixCoord>,
}

#[derive(Debug, Serialize)]
pub struct ChartCatalogRow {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub cycle_id: String,
    pub min_lat: f64,
    pub min_lon: f64,
    pub max_lat: f64,
    pub max_lon: f64,
    pub tile_url: String,
}

#[derive(Debug, Serialize)]
pub struct AirspaceRow {
    pub id: String,
    pub name: String,
    pub class: String,
    pub floor: String,
    pub ceiling: String,
    /// A GeoJSON `Polygon` geometry object (not a whole `Feature`) — the
    /// bundle stores exactly what `ff-etl`'s `polygon_geojson` wrote,
    /// unparsed, so the client's MapLibre source can use it directly.
    pub boundary_geojson: String,
    pub min_lat: f64,
    pub min_lon: f64,
    pub max_lat: f64,
    pub max_lon: f64,
}

#[derive(Debug, Deserialize)]
pub struct BboxQuery {
    /// `minLon,minLat,maxLon,maxLat` (GeoJSON/MapLibre bounds order).
    pub bbox: Option<String>,
}

fn parse_bbox(raw: &str) -> Option<(f64, f64, f64, f64)> {
    let parts: Vec<f64> = raw
        .split(',')
        .map_while(|p| p.trim().parse().ok())
        .collect();
    match parts[..] {
        [min_lon, min_lat, max_lon, max_lat] => Some((min_lon, min_lat, max_lon, max_lat)),
        _ => None,
    }
}

enum DataError {
    NoCycle,
    NotFound(&'static str),
    Internal(String),
}

impl IntoResponse for DataError {
    fn into_response(self) -> Response {
        match self {
            DataError::NoCycle => (
                StatusCode::NOT_IMPLEMENTED,
                "no cycle bundle has been published yet; run `cargo run -p ff-etl` first",
            )
                .into_response(),
            DataError::NotFound(what) => {
                (StatusCode::NOT_FOUND, format!("{what} not found")).into_response()
            }
            DataError::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg).into_response(),
        }
    }
}

impl From<rusqlite::Error> for DataError {
    fn from(err: rusqlite::Error) -> Self {
        match err {
            rusqlite::Error::QueryReturnedNoRows => DataError::NotFound("row"),
            other => DataError::Internal(other.to_string()),
        }
    }
}

/// Resolves the latest published bundle and runs `query` against it on
/// the blocking thread pool (rusqlite is synchronous).
async fn with_bundle<T, F>(state: &AppState, query: F) -> Result<T, DataError>
where
    T: Send + 'static,
    F: FnOnce(&Connection) -> Result<T, DataError> + Send + 'static,
{
    let bundle_path: PathBuf = ff_etl::publish::latest_bundle_path(&state.data_dir)
        .map_err(|e| DataError::Internal(e.to_string()))?
        .ok_or(DataError::NoCycle)?;
    tokio::task::spawn_blocking(move || {
        let conn = Connection::open(bundle_path).map_err(DataError::from)?;
        query(&conn)
    })
    .await
    .map_err(|e| DataError::Internal(e.to_string()))?
}

pub async fn airports(State(state): State<AppState>, Query(query): Query<BboxQuery>) -> Response {
    let bbox = query.bbox.as_deref().and_then(parse_bbox);
    let result = with_bundle(&state, move |conn| {
        let mut rows = Vec::new();
        let mut stmt = conn.prepare(
            "SELECT icao, faa_id, iata, name, lat, lon, elevation_ft, airport_type FROM airport ORDER BY icao",
        )?;
        let mapped = stmt.query_map([], |row| {
            Ok(AirportRow {
                icao: row.get(0)?,
                faa_id: row.get(1)?,
                iata: row.get(2)?,
                name: row.get(3)?,
                lat: row.get(4)?,
                lon: row.get(5)?,
                elevation_ft: row.get(6)?,
                airport_type: row.get(7)?,
            })
        })?;
        for airport in mapped {
            let airport = airport?;
            if let Some((min_lon, min_lat, max_lon, max_lat)) = bbox {
                if airport.lon < min_lon || airport.lon > max_lon || airport.lat < min_lat || airport.lat > max_lat {
                    continue;
                }
            }
            rows.push(airport);
        }
        Ok(rows)
    })
    .await;
    match result {
        Ok(rows) => Json(rows).into_response(),
        Err(err) => err.into_response(),
    }
}

#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    pub q: String,
}

/// Ident/name airport search for the web client's search box: exact-ish
/// prefix match on ICAO/FAA/IATA idents, substring match on the name,
/// case-insensitive, capped at 20 rows. (Pulled forward from its planned
/// Phase 2 slot — the search box is the consumer that justifies it.)
pub async fn search(State(state): State<AppState>, Query(query): Query<SearchQuery>) -> Response {
    let q = query.q.trim().to_uppercase();
    if q.is_empty() {
        return Json(Vec::<AirportRow>::new()).into_response();
    }
    let result = with_bundle(&state, move |conn| {
        let prefix = format!("{q}%");
        let substring = format!("%{q}%");
        let mut rows = Vec::new();
        let mut stmt = conn.prepare(
            "SELECT icao, faa_id, iata, name, lat, lon, elevation_ft, airport_type FROM airport
             WHERE icao LIKE ?1 OR faa_id LIKE ?1 OR iata LIKE ?1 OR UPPER(name) LIKE ?2
             ORDER BY (icao = ?3) DESC, icao
             LIMIT 20",
        )?;
        let mapped = stmt.query_map(rusqlite::params![prefix, substring, q], |row| {
            Ok(AirportRow {
                icao: row.get(0)?,
                faa_id: row.get(1)?,
                iata: row.get(2)?,
                name: row.get(3)?,
                lat: row.get(4)?,
                lon: row.get(5)?,
                elevation_ft: row.get(6)?,
                airport_type: row.get(7)?,
            })
        })?;
        for airport in mapped {
            rows.push(airport?);
        }
        Ok(rows)
    })
    .await;
    match result {
        Ok(rows) => Json(rows).into_response(),
        Err(err) => err.into_response(),
    }
}

pub async fn airport_detail(
    State(state): State<AppState>,
    UrlPath(icao): UrlPath<String>,
) -> Response {
    let result = with_bundle(&state, move |conn| {
        let airport = conn
            .query_row(
                "SELECT icao, faa_id, iata, name, lat, lon, elevation_ft, airport_type FROM airport WHERE icao = ?1",
                [&icao],
                |row| {
                    Ok(AirportRow {
                        icao: row.get(0)?,
                        faa_id: row.get(1)?,
                        iata: row.get(2)?,
                        name: row.get(3)?,
                        lat: row.get(4)?,
                        lon: row.get(5)?,
                        elevation_ft: row.get(6)?,
                        airport_type: row.get(7)?,
                    })
                },
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => DataError::NotFound("airport"),
                other => DataError::from(other),
            })?;

        let mut runways = Vec::new();
        let mut stmt = conn.prepare(
            "SELECT airport_icao, ident, length_ft, width_ft, surface,
                    le_ident, le_lat, le_lon, le_heading_deg,
                    he_ident, he_lat, he_lon, he_heading_deg
             FROM runway WHERE airport_icao = ?1 ORDER BY ident",
        )?;
        let mapped = stmt.query_map([&icao], |row| {
            Ok(RunwayRow {
                airport_icao: row.get(0)?,
                ident: row.get(1)?,
                length_ft: row.get(2)?,
                width_ft: row.get(3)?,
                surface: row.get(4)?,
                le_ident: row.get(5)?,
                le_lat: row.get(6)?,
                le_lon: row.get(7)?,
                le_heading_deg: row.get(8)?,
                he_ident: row.get(9)?,
                he_lat: row.get(10)?,
                he_lon: row.get(11)?,
                he_heading_deg: row.get(12)?,
            })
        })?;
        for runway in mapped {
            runways.push(runway?);
        }

        let mut frequencies = Vec::new();
        let mut stmt = conn.prepare(
            "SELECT airport_icao, kind, freq_mhz, remarks FROM frequency WHERE airport_icao = ?1",
        )?;
        let mapped = stmt.query_map([&icao], |row| {
            Ok(FrequencyRow {
                airport_icao: row.get(0)?,
                kind: row.get(1)?,
                freq_mhz: row.get(2)?,
                remarks: row.get(3)?,
            })
        })?;
        for frequency in mapped {
            frequencies.push(frequency?);
        }

        Ok(AirportDetail {
            airport,
            runways,
            frequencies,
        })
    })
    .await;
    match result {
        Ok(detail) => Json(detail).into_response(),
        Err(err) => err.into_response(),
    }
}

pub async fn airport_procedures(
    State(state): State<AppState>,
    UrlPath(icao): UrlPath<String>,
) -> Response {
    let result = with_bundle(&state, move |conn| {
        let mut rows = Vec::new();
        let mut stmt = conn.prepare(
            "SELECT id, airport_icao, kind, ident, runway_ident FROM procedure
             WHERE airport_icao = ?1 ORDER BY kind, ident",
        )?;
        let mapped = stmt.query_map([&icao], |row| {
            Ok(ProcedureRow {
                id: row.get(0)?,
                airport_icao: row.get(1)?,
                kind: row.get(2)?,
                ident: row.get(3)?,
                runway_ident: row.get(4)?,
            })
        })?;
        for procedure in mapped {
            rows.push(procedure?);
        }
        Ok(rows)
    })
    .await;
    match result {
        Ok(rows) => Json(rows).into_response(),
        Err(err) => err.into_response(),
    }
}

pub async fn procedure_detail(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
) -> Response {
    let result = with_bundle(&state, move |conn| {
        let procedure = conn
            .query_row(
                "SELECT id, airport_icao, kind, ident, runway_ident FROM procedure WHERE id = ?1",
                [&id],
                |row| {
                    Ok(ProcedureRow {
                        id: row.get(0)?,
                        airport_icao: row.get(1)?,
                        kind: row.get(2)?,
                        ident: row.get(3)?,
                        runway_ident: row.get(4)?,
                    })
                },
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => DataError::NotFound("procedure"),
                other => DataError::from(other),
            })?;

        let mut transitions = Vec::new();
        let mut stmt = conn.prepare(
            "SELECT id, ident, kind FROM procedure_transition WHERE procedure_id = ?1 ORDER BY kind, ident",
        )?;
        let transition_rows: Vec<(String, String, String)> = stmt
            .query_map([&id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
            .collect::<Result<_, _>>()?;

        let mut fix_idents: Vec<String> = Vec::new();
        for (t_id, t_ident, t_kind) in transition_rows {
            let mut legs = Vec::new();
            let mut stmt = conn.prepare(
                "SELECT seq, path_and_term, fix_ident, course_deg,
                        altitude_constraint, speed_constraint, turn_direction
                 FROM procedure_leg WHERE transition_id = ?1 ORDER BY seq",
            )?;
            let mapped = stmt.query_map([&t_id], |row| {
                Ok(ProcedureLegRow {
                    seq: row.get(0)?,
                    path_and_term: row.get(1)?,
                    fix_ident: row.get(2)?,
                    course_deg: row.get(3)?,
                    altitude_constraint: row.get(4)?,
                    speed_constraint: row.get(5)?,
                    turn_direction: row.get(6)?,
                })
            })?;
            for leg in mapped {
                let leg = leg?;
                if let Some(fix) = &leg.fix_ident {
                    if !fix_idents.contains(fix) {
                        fix_idents.push(fix.clone());
                    }
                }
                legs.push(leg);
            }
            transitions.push(ProcedureTransitionDetail {
                id: t_id,
                ident: t_ident,
                kind: t_kind,
                legs,
            });
        }

        let mut fixes = HashMap::new();
        for ident in &fix_idents {
            // Waypoints first, then navaids — same "first match wins"
            // the web client used when it resolved these itself.
            let coord = conn
                .query_row("SELECT lat, lon FROM waypoint WHERE ident = ?1", [ident], |row| {
                    Ok(FixCoord {
                        lat: row.get(0)?,
                        lon: row.get(1)?,
                    })
                })
                .or_else(|_| {
                    conn.query_row("SELECT lat, lon FROM navaid WHERE ident = ?1", [ident], |row| {
                        Ok(FixCoord {
                            lat: row.get(0)?,
                            lon: row.get(1)?,
                        })
                    })
                });
            if let Ok(coord) = coord {
                fixes.insert(ident.clone(), coord);
            }
        }

        Ok(ProcedureDetail {
            procedure,
            transitions,
            fixes,
        })
    })
    .await;
    match result {
        Ok(detail) => Json(detail).into_response(),
        Err(err) => err.into_response(),
    }
}

pub async fn charts(State(state): State<AppState>, Query(query): Query<BboxQuery>) -> Response {
    let bbox = query.bbox.as_deref().and_then(parse_bbox);
    let result = with_bundle(&state, move |conn| {
        let mut rows = Vec::new();
        let mut stmt = conn.prepare(
            "SELECT id, name, kind, cycle_id, min_lat, min_lon, max_lat, max_lon, tile_url FROM chart_catalog",
        )?;
        let mapped = stmt.query_map([], |row| {
            Ok(ChartCatalogRow {
                id: row.get(0)?,
                name: row.get(1)?,
                kind: row.get(2)?,
                cycle_id: row.get(3)?,
                min_lat: row.get(4)?,
                min_lon: row.get(5)?,
                max_lat: row.get(6)?,
                max_lon: row.get(7)?,
                tile_url: row.get(8)?,
            })
        })?;
        for chart in mapped {
            let chart = chart?;
            if let Some((min_lon, min_lat, max_lon, max_lat)) = bbox {
                let overlaps = chart.min_lon <= max_lon
                    && chart.max_lon >= min_lon
                    && chart.min_lat <= max_lat
                    && chart.max_lat >= min_lat;
                if !overlaps {
                    continue;
                }
            }
            rows.push(chart);
        }
        Ok(rows)
    })
    .await;
    match result {
        Ok(rows) => Json(rows).into_response(),
        Err(err) => err.into_response(),
    }
}

/// Class B/C/D + Special Use Airspace boundaries (DESIGN.md §3), same
/// bbox-overlap-filter shape as `charts` above — a nationwide cycle has
/// ~2800 rows across both real FAA sources (one per shelf/sector, not
/// one per named airspace), too many to always send whole.
pub async fn airspace(State(state): State<AppState>, Query(query): Query<BboxQuery>) -> Response {
    let bbox = query.bbox.as_deref().and_then(parse_bbox);
    let result = with_bundle(&state, move |conn| {
        let mut rows = Vec::new();
        let mut stmt = conn.prepare(
            "SELECT id, name, class, floor, ceiling, boundary_geojson, min_lat, min_lon, max_lat, max_lon FROM airspace",
        )?;
        let mapped = stmt.query_map([], |row| {
            Ok(AirspaceRow {
                id: row.get(0)?,
                name: row.get(1)?,
                class: row.get(2)?,
                floor: row.get(3)?,
                ceiling: row.get(4)?,
                boundary_geojson: row.get(5)?,
                min_lat: row.get(6)?,
                min_lon: row.get(7)?,
                max_lat: row.get(8)?,
                max_lon: row.get(9)?,
            })
        })?;
        for volume in mapped {
            let volume = volume?;
            if let Some((min_lon, min_lat, max_lon, max_lat)) = bbox {
                let overlaps = volume.min_lon <= max_lon
                    && volume.max_lon >= min_lon
                    && volume.min_lat <= max_lat
                    && volume.max_lat >= min_lat;
                if !overlaps {
                    continue;
                }
            }
            rows.push(volume);
        }
        Ok(rows)
    })
    .await;
    match result {
        Ok(rows) => Json(rows).into_response(),
        Err(err) => err.into_response(),
    }
}
