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
    /// The FAA d-TPP airport diagram, if `ff-etl`'s d-TPP matching found
    /// one for this airport (see `ff-etl::dtpp::AIRPORT_DIAGRAM_IDENT`) —
    /// `None` doesn't mean there's no real diagram, just that this cycle
    /// didn't confidently link one (or the airport has none published).
    pub airport_diagram_url: Option<String>,
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
    /// The FAA d-TPP plate chart for this procedure, if `ff-etl`'s
    /// best-effort ident matching (see `ff-etl::dtpp`) found one —
    /// `None` doesn't mean there's no real chart, just that this app
    /// couldn't confidently match it (common for some approach types).
    pub chart_name: Option<String>,
    pub chart_url: Option<String>,
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

#[derive(Debug, Serialize)]
pub struct DataSourceRow {
    pub name: String,
    pub effective_date: Option<String>,
    pub licence: Option<String>,
    pub url: Option<String>,
    pub attribution: String,
}

/// Data-source attributions shipped in the current cycle bundle — e.g. the
/// French SIA credit + AIRAC effective date, required by the Licence
/// Ouverte. Returns `[]` for older bundles that predate the `data_source`
/// table (opened raw here, without migrations), rather than erroring.
pub async fn attributions(State(state): State<AppState>) -> Response {
    let result = with_bundle(&state, move |conn| {
        let has_table: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='data_source')",
            [],
            |row| row.get(0),
        )?;
        if !has_table {
            return Ok(Vec::new());
        }
        let mut stmt = conn.prepare(
            "SELECT name, effective_date, licence, url, attribution FROM data_source ORDER BY name",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok(DataSourceRow {
                    name: row.get(0)?,
                    effective_date: row.get(1)?,
                    licence: row.get(2)?,
                    url: row.get(3)?,
                    attribution: row.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
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

        // Same "at most one row expected, take the first" shape as
        // procedure_detail's own chart lookup below — see that comment.
        let airport_diagram_url = conn
            .query_row(
                "SELECT pdf_url FROM dtpp_chart
                 WHERE airport_icao = ?1 AND procedure_ident = ?2
                 ORDER BY id LIMIT 1",
                rusqlite::params![icao, ff_etl::dtpp::AIRPORT_DIAGRAM_IDENT],
                |row| row.get::<_, String>(0),
            )
            .ok();

        Ok(AirportDetail {
            airport,
            runways,
            frequencies,
            airport_diagram_url,
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
            // the web client used when it resolved these itself. Runway-
            // threshold pseudo-fixes (e.g. "RW10R" — ARINC 424's way of
            // pointing a leg at a runway end rather than a named
            // waypoint/navaid) are never rows in either table, so they
            // fall through to a third lookup against this procedure's
            // own airport's runway ends: without it, the final-approach
            // leg into the runway just vanishes from the drawn path,
            // which reads as the approach never actually reaching the
            // runway. Scoped to `airport_icao` because runway end idents
            // like "10R" aren't unique across airports.
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
                })
                .or_else(|_| {
                    let rwy_ident = ident.strip_prefix("RW").ok_or(rusqlite::Error::QueryReturnedNoRows)?;
                    conn.query_row(
                        "SELECT le_ident, le_lat, le_lon, he_ident, he_lat, he_lon
                         FROM runway WHERE airport_icao = ?1 AND (le_ident = ?2 OR he_ident = ?2)",
                        rusqlite::params![procedure.airport_icao, rwy_ident],
                        |row| {
                            let le_ident: String = row.get(0)?;
                            if le_ident == rwy_ident {
                                Ok(FixCoord {
                                    lat: row.get(1)?,
                                    lon: row.get(2)?,
                                })
                            } else {
                                Ok(FixCoord {
                                    lat: row.get(4)?,
                                    lon: row.get(5)?,
                                })
                            }
                        },
                    )
                });
            if let Ok(coord) = coord {
                fixes.insert(ident.clone(), coord);
            }
        }

        // At most one row expected per (airport, ident) in practice, but
        // a chart_name/chart_type variant (e.g. a CAT II minima page)
        // can independently match the same procedure ident — take
        // whichever was inserted first rather than surfacing more than
        // one link from a single-chart field.
        let (chart_name, chart_url) = conn
            .query_row(
                "SELECT chart_name, pdf_url FROM dtpp_chart
                 WHERE airport_icao = ?1 AND procedure_ident = ?2
                 ORDER BY id LIMIT 1",
                rusqlite::params![procedure.airport_icao, procedure.ident],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .map(|(name, url)| (Some(name), Some(url)))
            .unwrap_or((None, None));

        Ok(ProcedureDetail {
            procedure,
            transitions,
            fixes,
            chart_name,
            chart_url,
        })
    })
    .await;
    match result {
        Ok(detail) => Json(detail).into_response(),
        Err(err) => err.into_response(),
    }
}

#[derive(Debug, Serialize)]
pub struct AirwayLegRow {
    pub seq: i64,
    pub fix_ident: String,
    pub min_altitude_ft: Option<i64>,
    pub max_altitude_ft: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct AirwayDetail {
    pub ident: String,
    pub kind: String,
    pub legs: Vec<AirwayLegRow>,
    /// Coordinates for every `fix_ident` on this airway that resolves to
    /// a waypoint or navaid in the cycle bundle — resolved server-side
    /// for the same reason `ProcedureDetail::fixes` is. No runway-end
    /// fallback here: airways never terminate at a runway pseudo-fix.
    pub fixes: HashMap<String, FixCoord>,
}

pub async fn airway_detail(
    State(state): State<AppState>,
    UrlPath(ident): UrlPath<String>,
) -> Response {
    let result = with_bundle(&state, move |conn| {
        let (airway_id, ident, kind) = conn
            .query_row(
                "SELECT id, ident, kind FROM airway WHERE ident = ?1 COLLATE NOCASE",
                [&ident],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => DataError::NotFound("airway"),
                other => DataError::Internal(other.to_string()),
            })?;

        let mut legs = Vec::new();
        let mut stmt = conn.prepare(
            "SELECT seq, fix_ident, min_altitude_ft, max_altitude_ft
             FROM airway_leg WHERE airway_id = ?1 ORDER BY seq",
        )?;
        let mapped = stmt.query_map([airway_id], |row| {
            Ok(AirwayLegRow {
                seq: row.get(0)?,
                fix_ident: row.get(1)?,
                min_altitude_ft: row.get(2)?,
                max_altitude_ft: row.get(3)?,
            })
        })?;
        for leg in mapped {
            legs.push(leg?);
        }

        let mut fixes = HashMap::new();
        for leg in &legs {
            if fixes.contains_key(&leg.fix_ident) {
                continue;
            }
            // Same waypoint-then-navaid resolution as procedure_detail.
            let coord = conn
                .query_row(
                    "SELECT lat, lon FROM waypoint WHERE ident = ?1",
                    [&leg.fix_ident],
                    |row| {
                        Ok(FixCoord {
                            lat: row.get(0)?,
                            lon: row.get(1)?,
                        })
                    },
                )
                .or_else(|_| {
                    conn.query_row(
                        "SELECT lat, lon FROM navaid WHERE ident = ?1",
                        [&leg.fix_ident],
                        |row| {
                            Ok(FixCoord {
                                lat: row.get(0)?,
                                lon: row.get(1)?,
                            })
                        },
                    )
                });
            if let Ok(coord) = coord {
                fixes.insert(leg.fix_ident.clone(), coord);
            }
        }

        Ok(AirwayDetail {
            ident,
            kind,
            legs,
            fixes,
        })
    })
    .await;
    match result {
        Ok(detail) => Json(detail).into_response(),
        Err(err) => err.into_response(),
    }
}

#[derive(Debug, Serialize)]
pub struct IdentSearchRow {
    /// "airport" | "waypoint" | "navaid" | "airway"
    pub kind: String,
    pub ident: String,
    pub name: Option<String>,
    /// Absent for airways (an airway is a path, not a point).
    pub lat: Option<f64>,
    pub lon: Option<f64>,
}

/// Unified ident search across airports, waypoints, navaids, and airways
/// for the route builder's single search box (DESIGN.md §9.3: "add
/// fixes/navaids/airways in between"). Exact ident matches rank before
/// prefix matches, airports before fixes; capped at ~20 rows total. The
/// airport-only `/data/search` keeps serving the map view unchanged.
pub async fn search_idents(
    State(state): State<AppState>,
    Query(query): Query<SearchQuery>,
) -> Response {
    let q = query.q.trim().to_uppercase();
    if q.is_empty() {
        return Json(Vec::<IdentSearchRow>::new()).into_response();
    }
    let result = with_bundle(&state, move |conn| {
        let prefix = format!("{q}%");
        let mut rows: Vec<(bool, usize, IdentSearchRow)> = Vec::new();

        let mut stmt = conn.prepare(
            "SELECT icao, name, lat, lon FROM airport
             WHERE icao LIKE ?1 OR faa_id LIKE ?1 OR iata LIKE ?1
             ORDER BY icao LIMIT 10",
        )?;
        let mapped = stmt.query_map([&prefix], |row| {
            Ok(IdentSearchRow {
                kind: "airport".to_string(),
                ident: row.get(0)?,
                name: row.get(1)?,
                lat: row.get(2)?,
                lon: row.get(3)?,
            })
        })?;
        for r in mapped {
            rows.push((false, 0, r?));
        }

        let mut stmt = conn.prepare(
            "SELECT ident, lat, lon FROM navaid WHERE ident LIKE ?1 GROUP BY ident ORDER BY ident LIMIT 10",
        )?;
        let mapped = stmt.query_map([&prefix], |row| {
            Ok(IdentSearchRow {
                kind: "navaid".to_string(),
                ident: row.get(0)?,
                name: None,
                lat: row.get(1)?,
                lon: row.get(2)?,
            })
        })?;
        for r in mapped {
            rows.push((false, 1, r?));
        }

        let mut stmt = conn.prepare(
            "SELECT ident, lat, lon FROM waypoint WHERE ident LIKE ?1 GROUP BY ident ORDER BY ident LIMIT 10",
        )?;
        let mapped = stmt.query_map([&prefix], |row| {
            Ok(IdentSearchRow {
                kind: "waypoint".to_string(),
                ident: row.get(0)?,
                name: None,
                lat: row.get(1)?,
                lon: row.get(2)?,
            })
        })?;
        for r in mapped {
            rows.push((false, 2, r?));
        }

        let mut stmt = conn.prepare(
            "SELECT ident, kind FROM airway WHERE ident LIKE ?1 GROUP BY ident ORDER BY ident LIMIT 10",
        )?;
        let mapped = stmt.query_map([&prefix], |row| {
            Ok(IdentSearchRow {
                kind: "airway".to_string(),
                ident: row.get(0)?,
                name: row.get(1)?, // airway kind (VICTOR/JET/...) doubles as its display name
                lat: None,
                lon: None,
            })
        })?;
        for r in mapped {
            rows.push((false, 3, r?));
        }

        // Exact ident matches first, then airports < navaids < waypoints
        // < airways, then ident — a stable, predictable ordering for a
        // dropdown. The bool sorts false-first, so flip it for "exact".
        for row in &mut rows {
            row.0 = row.2.ident != q;
        }
        rows.sort_by(|a, b| {
            (a.0, a.1, &a.2.ident).cmp(&(b.0, b.1, &b.2.ident))
        });
        rows.truncate(20);
        Ok(rows.into_iter().map(|(_, _, r)| r).collect::<Vec<_>>())
    })
    .await;
    match result {
        Ok(rows) => Json(rows).into_response(),
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

#[derive(Debug, Serialize)]
pub struct NearestFixRow {
    /// "WAYPOINT" for a plain enroute fix, else the navaid's own type
    /// (e.g. "VOR", "VORTAC", "NDB") — there's no single ident-scoped
    /// "kind" column spanning both tables, so this is populated from
    /// whichever table the result actually came from.
    pub kind: String,
    pub ident: String,
    pub lat: f64,
    pub lon: f64,
}

#[derive(Debug, Deserialize)]
pub struct PointQuery {
    pub lat: f64,
    pub lon: f64,
}

/// Nearest waypoint or navaid to a tapped map point — used by the map's
/// "Waypoint" tap tab. Straight full-table scan over both `waypoint`
/// (~49k rows) and `navaid` (~900 rows nationwide) computing planar
/// distance-squared in Rust, same "load it all, filter/compare in Rust"
/// approach `airports`/`airspace` above already use for bbox queries —
/// at this row count a single scan is low-single-digit milliseconds, so
/// a spatial index isn't worth the complexity yet.
pub async fn nearest_fix(
    State(state): State<AppState>,
    Query(query): Query<PointQuery>,
) -> Response {
    let (lat, lon) = (query.lat, query.lon);
    let result = with_bundle(&state, move |conn| {
        let mut best: Option<(f64, NearestFixRow)> = None;

        let mut wpt_stmt = conn.prepare("SELECT ident, lat, lon FROM waypoint")?;
        let wpt_rows = wpt_stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, f64>(1)?,
                row.get::<_, f64>(2)?,
            ))
        })?;
        for row in wpt_rows {
            let (ident, wlat, wlon) = row?;
            let dist_sq = (wlat - lat).powi(2) + (wlon - lon).powi(2);
            if best
                .as_ref()
                .is_none_or(|(best_dist, _)| dist_sq < *best_dist)
            {
                best = Some((
                    dist_sq,
                    NearestFixRow {
                        kind: "WAYPOINT".to_string(),
                        ident,
                        lat: wlat,
                        lon: wlon,
                    },
                ));
            }
        }

        let mut navaid_stmt = conn.prepare("SELECT ident, navaid_type, lat, lon FROM navaid")?;
        let navaid_rows = navaid_stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, f64>(2)?,
                row.get::<_, f64>(3)?,
            ))
        })?;
        for row in navaid_rows {
            let (ident, navaid_type, nlat, nlon) = row?;
            let dist_sq = (nlat - lat).powi(2) + (nlon - lon).powi(2);
            if best
                .as_ref()
                .is_none_or(|(best_dist, _)| dist_sq < *best_dist)
            {
                best = Some((
                    dist_sq,
                    NearestFixRow {
                        kind: navaid_type,
                        ident,
                        lat: nlat,
                        lon: nlon,
                    },
                ));
            }
        }

        Ok(best.map(|(_, row)| row))
    })
    .await;
    match result {
        Ok(Some(row)) => Json(row).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            "no waypoints or navaids in this cycle",
        )
            .into_response(),
        Err(err) => err.into_response(),
    }
}
