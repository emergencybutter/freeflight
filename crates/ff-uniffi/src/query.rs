//! Reads against the locally-installed cycle bundle.
//!
//! These are the Android equivalents of `ff-api`'s `/data/*` routes
//! (DESIGN.md §4.1) and answer the same questions with the same SQL — the
//! difference is only where the SQLite file is. On web that file is on the
//! server and the answers arrive as JSON; here it is on the device, which
//! is what makes every screen except live weather work with no network
//! (§8).
//!
//! Everything takes a plain `&Connection` and returns plain records, so
//! each one is testable against a fixture bundle without going near JNI.

use crate::error::CoreError;
use crate::types::{
    Airport, AirportDetail, Airspace, BoundingBox, Chart, DataSourceCredit, Frequency, Plate,
    Procedure, ProcedureDetail, ProcedureLeg, ProcedureTransition, Runway, SearchHit,
};
use rusqlite::{params, Connection, Row};

/// FAA d-TPP marks an airport diagram with this procedure ident (the same
/// constant `ff-etl::dtpp` writes and `ff-api` looks up — duplicated
/// rather than depended on, because `ff-etl` is a server-side batch job
/// with a heavy dependency tree that has no business inside an Android
/// `.so`).
const AIRPORT_DIAGRAM_IDENT: &str = "AIRPORT DIAGRAM";

type Result<T> = std::result::Result<T, CoreError>;

pub fn cycle_effective_date(conn: &Connection) -> Option<String> {
    conn.query_row(
        "SELECT effective_date FROM airac_cycle ORDER BY id DESC LIMIT 1",
        [],
        |row| row.get(0),
    )
    .ok()
}

pub fn count(conn: &Connection, table: &str) -> u32 {
    // `table` is never caller-supplied — the two call sites pass literals.
    conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
        row.get::<_, i64>(0)
    })
    .map(|n| n as u32)
    .unwrap_or(0)
}

/// Airports inside the map viewport, most significant first.
///
/// The ordering matters more than it looks: a zoomed-out viewport over the
/// northeast contains thousands of airports, far more than are legible or
/// worth drawing, so `limit` cuts the list — and it must cut the grass
/// strips, not the Class B fields. "Has a procedure in this bundle" is the
/// available proxy for significance, with landplane airports ahead of
/// heliports/seaplane bases as the tiebreak.
pub fn airports_in_bbox(conn: &Connection, bbox: BoundingBox, limit: u32) -> Result<Vec<Airport>> {
    let sql = format!(
        "SELECT icao, faa_id, iata, name, lat, lon, elevation_ft, airport_type,
                EXISTS(SELECT 1 FROM procedure WHERE procedure.airport_icao = airport.icao)
         FROM airport
         WHERE lat BETWEEN ?1 AND ?2 AND {}
         ORDER BY 9 DESC, (airport_type = 'Airport') DESC, icao
         LIMIT ?5",
        lon_predicate(bbox)
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(
            params![
                bbox.min_lat,
                bbox.max_lat,
                bbox.min_lon,
                bbox.max_lon,
                limit
            ],
            airport_from_row,
        )?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// A longitude filter that survives the antimeridian.
///
/// A viewport over the Aleutians hands back a westward range whose minimum
/// is greater than its maximum (e.g. 170 → -170). `BETWEEN` matches
/// nothing there, which would silently empty the map over the one part of
/// US airspace where it happens, so that case becomes an `OR` of the two
/// halves instead. Parameter numbering is identical in both branches so
/// callers bind the same list either way.
fn lon_predicate(bbox: BoundingBox) -> &'static str {
    if bbox.min_lon <= bbox.max_lon {
        "lon BETWEEN ?3 AND ?4"
    } else {
        "(lon >= ?3 OR lon <= ?4)"
    }
}

fn airport_from_row(row: &Row) -> rusqlite::Result<Airport> {
    Ok(Airport {
        icao: row.get(0)?,
        faa_id: row.get(1)?,
        iata: row.get(2)?,
        name: row.get(3)?,
        lat: row.get(4)?,
        lon: row.get(5)?,
        elevation_ft: row.get(6)?,
        airport_type: row.get(7)?,
        has_procedures: row.get::<_, Option<bool>>(8)?.unwrap_or(false),
    })
}

/// Unified ident/name search across airports, navaids and waypoints —
/// what the search box on the map runs. Exact ident matches rank first,
/// then airports, then everything else, mirroring `ff-api`'s
/// `/data/search_idents`.
pub fn search(conn: &Connection, query: &str, limit: u32) -> Result<Vec<SearchHit>> {
    let q = query.trim().to_uppercase();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let prefix = format!("{q}%");
    let substring = format!("%{q}%");
    let limit = limit.max(1) as i64;

    let mut hits: Vec<(bool, u8, SearchHit)> = Vec::new();

    let mut stmt = conn.prepare(
        "SELECT icao, name, lat, lon FROM airport
         WHERE icao LIKE ?1 OR faa_id LIKE ?1 OR iata LIKE ?1 OR UPPER(name) LIKE ?2
         ORDER BY icao LIMIT ?3",
    )?;
    for hit in stmt.query_map(params![prefix, substring, limit], |row| {
        let ident: String = row.get(0)?;
        Ok(SearchHit {
            kind: "airport".to_string(),
            ident,
            name: row.get(1)?,
            lat: row.get(2)?,
            lon: row.get(3)?,
        })
    })? {
        let hit = hit?;
        hits.push((hit.ident == q, 0, hit));
    }

    for (kind, table) in [("navaid", "navaid"), ("waypoint", "waypoint")] {
        let mut stmt = conn.prepare(&format!(
            "SELECT ident, lat, lon FROM {table} WHERE ident LIKE ?1
             GROUP BY ident ORDER BY ident LIMIT ?2"
        ))?;
        for hit in stmt.query_map(params![prefix, limit], |row| {
            Ok(SearchHit {
                kind: kind.to_string(),
                ident: row.get(0)?,
                name: None,
                lat: row.get(1)?,
                lon: row.get(2)?,
            })
        })? {
            let hit = hit?;
            hits.push((hit.ident == q, if kind == "navaid" { 1 } else { 2 }, hit));
        }
    }

    hits.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    hits.truncate(limit as usize);
    Ok(hits.into_iter().map(|(_, _, hit)| hit).collect())
}

pub fn airport_detail(conn: &Connection, icao: &str) -> Result<AirportDetail> {
    let icao = icao.trim().to_uppercase();
    let airport = conn
        .query_row(
            "SELECT icao, faa_id, iata, name, lat, lon, elevation_ft, airport_type,
                    EXISTS(SELECT 1 FROM procedure WHERE procedure.airport_icao = airport.icao)
             FROM airport WHERE icao = ?1",
            [&icao],
            airport_from_row,
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => CoreError::NotFound(icao.clone()),
            other => CoreError::from(other),
        })?;

    let mut stmt = conn.prepare(
        "SELECT ident, length_ft, width_ft, surface,
                le_ident, le_lat, le_lon, le_heading_deg,
                he_ident, he_lat, he_lon, he_heading_deg
         FROM runway WHERE airport_icao = ?1 ORDER BY ident",
    )?;
    let runways = stmt
        .query_map([&icao], |row| {
            Ok(Runway {
                ident: row.get(0)?,
                length_ft: row.get(1)?,
                width_ft: row.get(2)?,
                surface: row.get(3)?,
                le_ident: row.get(4)?,
                le_lat: row.get(5)?,
                le_lon: row.get(6)?,
                le_heading_deg: row.get(7)?,
                he_ident: row.get(8)?,
                he_lat: row.get(9)?,
                he_lon: row.get(10)?,
                he_heading_deg: row.get(11)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    let mut stmt =
        conn.prepare("SELECT kind, freq_mhz, remarks FROM frequency WHERE airport_icao = ?1")?;
    let frequency_rows = stmt
        .query_map([&icao], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, f64>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let frequencies = collapse_frequencies(frequency_rows);

    let airport_diagram_url = dtpp_chart(conn, &icao, AIRPORT_DIAGRAM_IDENT).map(|(_, url)| url);

    Ok(AirportDetail {
        airport,
        runways,
        frequencies,
        airport_diagram_url,
    })
}

/// How useful a frequency kind is to a pilot looking at an airport page,
/// most useful first. `OTHER` is last and deliberately so: in the CIFP-fed
/// bundle it is overwhelmingly the most common kind (13,150 rows against
/// 1,495 tower frequencies), because every procedure's controlling
/// frequency is recorded that way.
const FREQUENCY_KIND_ORDER: &[&str] = &[
    "ATIS", "AWOS", "ASOS", "CTAF", "UNICOM", "CLNC DEL", "GND", "TWR", "APP", "DEP",
];

/// One row per actual frequency, rather than one per record.
///
/// A big Class B airport carries the same number many times over — KSEA's
/// bundle has 99 `frequency` rows covering 44 distinct frequencies, because
/// each procedure that uses a sector contributes its own row, tagged
/// `OTHER` and remarked with the procedure's name. Listed verbatim that is
/// pages of near-duplicates that push the runways and procedures off the
/// screen, and it answers the wrong question: a pilot wants "what do I
/// tune", not "which records mention this number".
///
/// So rows are grouped by frequency; each group keeps the most informative
/// kind anyone recorded for it (an `APP` row and an `OTHER` row for 119.2
/// make it Approach, not Other) and the distinct remarks, merged. Nothing
/// is dropped — the remarks still name every procedure — but 99 rows
/// become 44.
fn collapse_frequencies(rows: Vec<(String, f64, Option<String>)>) -> Vec<Frequency> {
    fn rank(kind: &str) -> usize {
        FREQUENCY_KIND_ORDER
            .iter()
            .position(|known| known.eq_ignore_ascii_case(kind))
            .unwrap_or(FREQUENCY_KIND_ORDER.len())
    }

    // Keyed on the frequency in kHz so it groups on an exact integer
    // rather than on the bit pattern of a float.
    let mut groups: Vec<(i64, String, Vec<String>)> = Vec::new();
    for (kind, freq_mhz, remarks) in rows {
        let key = (freq_mhz * 1000.0).round() as i64;
        let group = match groups.iter_mut().find(|(existing, _, _)| *existing == key) {
            Some(group) => group,
            None => {
                groups.push((key, kind.clone(), Vec::new()));
                groups.last_mut().expect("just pushed")
            }
        };
        if rank(&kind) < rank(&group.1) {
            group.1 = kind;
        }
        if let Some(remark) = remarks
            .map(|r| r.trim().to_string())
            .filter(|r| !r.is_empty())
        {
            if !group.2.contains(&remark) {
                group.2.push(remark);
            }
        }
    }

    groups.sort_by(|a, b| rank(&a.1).cmp(&rank(&b.1)).then(a.0.cmp(&b.0)));
    groups
        .into_iter()
        .map(|(key, kind, remarks)| Frequency {
            kind,
            freq_mhz: key as f64 / 1000.0,
            remarks: (!remarks.is_empty()).then(|| remarks.join(", ")),
        })
        .collect()
}

pub fn airport_procedures(conn: &Connection, icao: &str) -> Result<Vec<Procedure>> {
    let icao = icao.trim().to_uppercase();
    let mut stmt = conn.prepare(
        "SELECT id, airport_icao, kind, ident, runway_ident FROM procedure
         WHERE airport_icao = ?1 ORDER BY kind, ident",
    )?;
    let rows = stmt
        .query_map([&icao], procedure_from_row)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn procedure_from_row(row: &Row) -> rusqlite::Result<Procedure> {
    Ok(Procedure {
        id: row.get(0)?,
        airport_icao: row.get(1)?,
        kind: row.get(2)?,
        ident: row.get(3)?,
        runway_ident: row.get(4)?,
    })
}

pub fn procedure_detail(conn: &Connection, id: &str) -> Result<ProcedureDetail> {
    let procedure = conn
        .query_row(
            "SELECT id, airport_icao, kind, ident, runway_ident FROM procedure WHERE id = ?1",
            [id],
            procedure_from_row,
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => CoreError::NotFound(id.to_string()),
            other => CoreError::from(other),
        })?;

    let mut stmt = conn.prepare(
        "SELECT id, ident, kind FROM procedure_transition WHERE procedure_id = ?1
         ORDER BY kind, ident",
    )?;
    let transition_rows: Vec<(String, String, String)> = stmt
        .query_map([id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .collect::<std::result::Result<_, _>>()?;

    let mut transitions = Vec::with_capacity(transition_rows.len());
    for (t_id, t_ident, t_kind) in transition_rows {
        let mut stmt = conn.prepare(
            "SELECT seq, path_and_term, fix_ident, course_deg,
                    altitude_constraint, speed_constraint, turn_direction
             FROM procedure_leg WHERE transition_id = ?1 ORDER BY seq",
        )?;
        let mut legs: Vec<ProcedureLeg> = stmt
            .query_map([&t_id], |row| {
                Ok(ProcedureLeg {
                    seq: row.get(0)?,
                    path_and_term: row.get(1)?,
                    fix_ident: row.get(2)?,
                    course_deg: row.get(3)?,
                    altitude_constraint: row.get(4)?,
                    speed_constraint: row.get(5)?,
                    turn_direction: row.get(6)?,
                    lat: None,
                    lon: None,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        for leg in &mut legs {
            if let Some(fix) = leg.fix_ident.clone() {
                if let Some((lat, lon)) = resolve_fix(conn, &fix, &procedure.airport_icao) {
                    leg.lat = Some(lat);
                    leg.lon = Some(lon);
                }
            }
        }
        transitions.push(ProcedureTransition {
            id: t_id,
            ident: t_ident,
            kind: t_kind,
            legs,
        });
    }

    let chart = dtpp_chart(conn, &procedure.airport_icao, &procedure.ident);
    Ok(ProcedureDetail {
        procedure,
        transitions,
        chart_name: chart.as_ref().map(|(name, _)| name.clone()),
        chart_url: chart.map(|(_, url)| url),
    })
}

/// Waypoint, then navaid, then — for ARINC 424's `RWnn` runway-threshold
/// pseudo-fixes — the runway ends of the procedure's own airport. Without
/// that last step the final approach leg has no coordinate and the drawn
/// approach stops short of the runway, which reads as a data bug. Scoped
/// to the airport because runway idents like "10R" repeat everywhere.
fn resolve_fix(conn: &Connection, ident: &str, airport_icao: &str) -> Option<(f64, f64)> {
    let point = |sql: &str, args: &[&dyn rusqlite::ToSql]| -> Option<(f64, f64)> {
        conn.query_row(sql, args, |row| Ok((row.get(0)?, row.get(1)?)))
            .ok()
    };
    point("SELECT lat, lon FROM waypoint WHERE ident = ?1", &[&ident])
        .or_else(|| point("SELECT lat, lon FROM navaid WHERE ident = ?1", &[&ident]))
        .or_else(|| {
            let rwy = ident.strip_prefix("RW")?;
            conn.query_row(
                "SELECT le_ident, le_lat, le_lon, he_lat, he_lon FROM runway
                 WHERE airport_icao = ?1 AND (le_ident = ?2 OR he_ident = ?2)",
                params![airport_icao, rwy],
                |row| {
                    let le_ident: String = row.get(0)?;
                    if le_ident == rwy {
                        Ok((row.get(1)?, row.get(2)?))
                    } else {
                        Ok((row.get(3)?, row.get(4)?))
                    }
                },
            )
            .ok()
        })
}

/// At most one row is expected per (airport, ident), but chart variants
/// (a CAT II minima page, say) can match the same procedure ident — take
/// whichever was inserted first rather than surfacing several links from
/// a single-chart field, exactly as `ff-api` does.
fn dtpp_chart(conn: &Connection, airport_icao: &str, ident: &str) -> Option<(String, String)> {
    conn.query_row(
        "SELECT chart_name, pdf_url FROM dtpp_chart
         WHERE airport_icao = ?1 AND procedure_ident = ?2 ORDER BY id LIMIT 1",
        params![airport_icao, ident],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .ok()
}

/// The order a truncated airspace query keeps.
///
/// Boundaries are unbounded in a way airports are not: each carries a
/// polygon, and a viewport over the country selects tens of thousands of
/// them. So the query takes a `limit` — but alphabetical order would then
/// drop `WARNING`, `RESTRICTED` and `PROHIBITED` off the end while keeping
/// `ALERT` and `ATZ`, which is precisely backwards. This ranks by what a
/// pilot does about the volume, so truncation eats the wide-area classes
/// first: Class E covers most of the country above 1200 AGL and the map
/// already treats it as context rather than a boundary to avoid.
const AIRSPACE_PRIORITY: &str = "CASE class
    WHEN 'B' THEN 0 WHEN 'C' THEN 0 WHEN 'D' THEN 0 WHEN 'ATZ' THEN 0
    WHEN 'PROHIBITED' THEN 1 WHEN 'RESTRICTED' THEN 1
    WHEN 'WARNING' THEN 1 WHEN 'DANGER' THEN 1
    WHEN 'MOA' THEN 2 WHEN 'ALERT' THEN 2 WHEN 'PARACHUTE' THEN 2
    WHEN 'GLIDER' THEN 2 WHEN 'LOW FLYING' THEN 2
    WHEN 'RMZ' THEN 3 WHEN 'TMZ' THEN 3
    ELSE 4
  END";

pub fn airspace_in_bbox(
    conn: &Connection,
    bbox: BoundingBox,
    limit: u32,
) -> Result<Vec<Airspace>> {
    // Overlap, not containment: a Class B shelf far larger than the
    // viewport still has to be drawn when you are inside it.
    let sql = format!(
        "SELECT id, name, class, floor, ceiling, boundary_geojson FROM airspace
         WHERE max_lat >= ?1 AND min_lat <= ?2 AND {}
         ORDER BY {}, class, name
         LIMIT ?5",
        if bbox.min_lon <= bbox.max_lon {
            "max_lon >= ?3 AND min_lon <= ?4"
        } else {
            "(max_lon >= ?3 OR min_lon <= ?4)"
        },
        AIRSPACE_PRIORITY
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(
            params![
                bbox.min_lat,
                bbox.max_lat,
                bbox.min_lon,
                bbox.max_lon,
                limit
            ],
            |row| {
                Ok(Airspace {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    class: row.get(2)?,
                    floor: row.get(3)?,
                    ceiling: row.get(4)?,
                    boundary_geojson: row.get(5)?,
                })
            },
        )?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// The content hash a chart's archive is stored under.
///
/// `chart_catalog.sha256` when the bundle has one (migration 0007), which
/// is what makes an unchanged chart reusable across cycles and lets a
/// download be verified. Bundles published before that migration have no
/// hash, and there is nothing to recover it from without the file, so they
/// fall back to a digest of the chart id: still a stable key, so the chart
/// installs and serves tiles normally, but no verification and no reuse —
/// chart ids embed the cycle date, so an older bundle's charts could not
/// have been shared across cycles anyway.
pub fn chart_blob_key(catalogued_sha256: Option<&str>, chart_id: &str) -> String {
    match catalogued_sha256 {
        Some(hash) if hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit()) => {
            hash.to_ascii_lowercase()
        }
        _ => ff_sync::sha256_hex(chart_id.as_bytes()),
    }
}

/// The catalogued hash for one chart, and whether it was verifiable.
pub fn chart_hash(conn: &Connection, chart_id: &str) -> Result<(String, bool)> {
    let catalogued: Option<String> = conn
        .query_row(
            "SELECT sha256 FROM chart_catalog WHERE id = ?1",
            [chart_id],
            |row| row.get(0),
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => CoreError::NotFound(chart_id.to_string()),
            other => CoreError::from(other),
        })?;
    let key = chart_blob_key(catalogued.as_deref(), chart_id);
    let verifiable = catalogued.as_deref() == Some(key.as_str());
    Ok((key, verifiable))
}

/// Every blob key this bundle still refers to — what pruning must keep.
pub fn catalogued_chart_hashes(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT id, sha256 FROM chart_catalog")?;
    let rows = stmt
        .query_map([], |row| {
            let id: String = row.get(0)?;
            let sha: Option<String> = row.get(1)?;
            Ok(chart_blob_key(sha.as_deref(), &id))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn charts(conn: &Connection) -> Result<Vec<Chart>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, kind, min_lat, min_lon, max_lat, max_lon, tile_url, bytes
         FROM chart_catalog ORDER BY kind, name",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok(Chart {
                id: row.get(0)?,
                name: row.get(1)?,
                kind: row.get(2)?,
                bbox: BoundingBox {
                    min_lat: row.get(3)?,
                    min_lon: row.get(4)?,
                    max_lat: row.get(5)?,
                    max_lon: row.get(6)?,
                },
                tile_url: row.get(7)?,
                download_bytes: row.get::<_, Option<i64>>(8)?.map(|b| b.max(0) as u64),
                // Filled in by the caller, which is the half that knows
                // what is on disk.
                installed: false,
                installed_bytes: 0,
                min_zoom: 0,
                max_zoom: 0,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Credits for the data in this bundle. Older bundles predate the
/// `data_source` table; they get an empty list rather than an error, since
/// a missing credit table is a stale bundle, not a broken app.
pub fn attributions(conn: &Connection) -> Result<Vec<DataSourceCredit>> {
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
            Ok(DataSourceCredit {
                name: row.get(0)?,
                effective_date: row.get(1)?,
                licence: row.get(2)?,
                url: row.get(3)?,
                attribution: row.get(4)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Every plate this airport publishes, diagram first, then by name.
///
/// Deduplicated on `pdf_url` rather than on procedure ident: a single PDF
/// routinely covers several idents (one approach chart serving RWY 4L and
/// 4R), and listing it twice would make a pilot think there were two
/// downloads to take. `installed` is left false here — only the caller
/// knows the data directory — and filled in by `Freeflight::airport_plates`.
pub fn airport_plates(conn: &Connection, icao: &str) -> Result<Vec<Plate>> {
    let icao = icao.trim().to_uppercase();
    let mut stmt = conn.prepare(
        "SELECT chart_name, pdf_url, procedure_ident FROM dtpp_chart
         WHERE airport_icao = ?1 GROUP BY pdf_url ORDER BY MIN(id)",
    )?;
    let mut plates: Vec<Plate> = stmt
        .query_map(params![icao], |row| {
            Ok(Plate {
                chart_name: row.get(0)?,
                pdf_url: row.get(1)?,
                procedure_ident: row.get(2)?,
                installed: false,
            })
        })?
        .collect::<std::result::Result<_, _>>()?;

    // The diagram is the one plate a pilot reaches for on the ground, so
    // it leads regardless of the order d-TPP happened to list it in.
    plates.sort_by(|a, b| {
        let rank = |p: &Plate| (p.procedure_ident != AIRPORT_DIAGRAM_IDENT) as u8;
        rank(a)
            .cmp(&rank(b))
            .then_with(|| a.chart_name.cmp(&b.chart_name))
    });
    Ok(plates)
}
