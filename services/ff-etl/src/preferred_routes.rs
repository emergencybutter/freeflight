//! Fetches two FAA-published city-pair route databases and resolves them
//! against this bundle's own airport table, producing suggested
//! departure/arrival routings for the flight-plan route builder. Neither
//! is a clearance or a guarantee either will be assigned — both describe
//! what's commonly filed/expected between two specific airports.
//!
//! - **NFDC Preferred Routes** (`PFR` rows,
//!   <https://www.fly.faa.gov/rmt/nfdc_preferred_routes_database>) — ATC-
//!   published high/low altitude and Tower Enroute Control (TEC)
//!   routings, some restricted by altitude/aircraft/direction. Orig/Dest
//!   are FAA 3-letter local identifiers (no "K" prefix) for domestic
//!   airports, already full ICAO for non-US ones (e.g. `CYUL`) — resolved
//!   against the bundle's own `faa_id`/`icao` columns (see
//!   `bundle::load_nasr_enrichment`, which backfills `faa_id`; without it
//!   this whole module would have nothing domestic to resolve against).
//!   A `NAR` route type exists in the same file but its "Orig" is an
//!   oceanic entry/exit fix (e.g. "ALLEX", "MT"), not an airport — there
//!   is no departure airport for a flight plan to match, so these rows
//!   are dropped entirely.
//! - **ATCSCC Coded Departure Routes**
//!   (`CDR` rows, <https://www.fly.faa.gov/rmt/cdm_operational_coded_departur>)
//!   — pre-coordinated reroute strings, mostly used for weather/traffic-
//!   flow reroutes. Already keyed by full ICAO (`KABE`, not `ABE`), so no
//!   resolution beyond confirming the airport exists in this bundle.
//!
//! Both source files are plain unquoted CSV (confirmed against a live
//! download: zero `"` bytes, constant field count per data row) — no
//! CSV-quoting crate needed, a `split(',')` per line is exact.
use crate::fetch::http_client;
use rusqlite::Connection;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use thiserror::Error;

const PFR_URL: &str = "https://www.fly.faa.gov/rmt/data_file/prefroutes_db.csv";
const CDR_URL: &str = "https://www.fly.faa.gov/rmt/data_file/codedswap_db.csv";

#[derive(Debug, Error)]
pub enum PreferredRoutesError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

/// One suggested routing between two airports already in this bundle.
#[derive(Debug, Clone)]
pub struct PreferredRoute {
    pub source: &'static str, // "PFR" | "CDR"
    pub orig_icao: String,
    pub dest_icao: String,
    /// Intermediate fixes/airways only — endpoints stripped, since the
    /// route builder already has departure/arrival as separate fields.
    /// Empty means a direct routing with no filed intermediate fixes.
    pub route_string: String,
    pub route_type: Option<String>,
    pub altitude: Option<String>,
    pub aircraft: Option<String>,
    pub direction: Option<String>,
    pub area: Option<String>,
    pub code: Option<String>,
    pub dep_fix: Option<String>,
    pub coordination_required: Option<String>,
    pub nav_equipment: Option<String>,
    pub dep_artcc: Option<String>,
    pub arr_artcc: Option<String>,
    pub seq: Option<i64>,
}

/// `None` for an empty (post-trim) field, matching the CSVs' own
/// convention of a blank cell rather than an explicit sentinel.
fn opt(s: &str) -> Option<String> {
    let t = s.trim();
    (!t.is_empty()).then(|| t.to_string())
}

/// Splits one data line on `,` — safe because both source files are
/// confirmed unquoted (see module docs) — stripping a leading UTF-8 BOM
/// from the first field if present (both files start with one, on the
/// header line only, but stripping unconditionally is harmless).
fn split_line(line: &str) -> Vec<&str> {
    line.trim_start_matches('\u{feff}').split(',').collect()
}

/// Removes the leading/trailing endpoint tokens from a route string when
/// they textually match the CSV's own raw Orig/Dest fields — both source
/// files embed the endpoints in the route string itself (e.g. PFR row
/// `ABE,ABE FJC ARD CYN ACY,ACY,...`), but showing them again in the
/// suggested middle tokens would duplicate the airports the route builder
/// already has as separate fields.
fn strip_endpoints<'a>(route_string: &'a str, raw_orig: &str, raw_dest: &str) -> &'a str {
    let mut s = route_string.trim();
    if let Some(rest) = s.strip_prefix(raw_orig) {
        if rest.is_empty() || rest.starts_with(char::is_whitespace) {
            s = rest.trim_start();
        }
    }
    if let Some(rest) = s.strip_suffix(raw_dest) {
        if rest.is_empty() || rest.ends_with(char::is_whitespace) {
            s = rest.trim_end();
        }
    }
    s
}

/// Builds the two lookups needed to resolve a CSV row's Orig/Dest into
/// this bundle's own ICAO idents: every known ICAO (covers already-ICAO
/// values like Canada's `CYUL`), and FAA local identifier -> ICAO (covers
/// the Preferred Routes file's 3-letter domestic idents, e.g. `ABE` ->
/// `KABE`).
fn load_airport_lookup(
    bundle_path: &Path,
) -> Result<(HashSet<String>, HashMap<String, String>), PreferredRoutesError> {
    let conn = Connection::open(bundle_path)?;
    let mut stmt = conn.prepare("SELECT icao, faa_id FROM airport")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
    })?;
    let mut icaos = HashSet::new();
    let mut faa_id_to_icao = HashMap::new();
    for row in rows {
        let (icao, faa_id) = row?;
        if let Some(faa_id) = faa_id {
            faa_id_to_icao.insert(faa_id, icao.clone());
        }
        icaos.insert(icao);
    }
    Ok((icaos, faa_id_to_icao))
}

/// Resolves a raw Orig/Dest field to this bundle's ICAO ident. `None`
/// means this bundle has no matching airport — the row is dropped rather
/// than guessed at (a "K" + ident heuristic breaks for Alaska/Hawaii/
/// territories, which is exactly why `faa_id` exists as a real column
/// instead of a prefix rule).
fn resolve_icao<'a>(
    raw: &str,
    icaos: &'a HashSet<String>,
    faa_id_to_icao: &'a HashMap<String, String>,
) -> Option<&'a str> {
    icaos
        .get(raw)
        .map(String::as_str)
        .or_else(|| faa_id_to_icao.get(raw).map(String::as_str))
}

fn parse_pfr(
    csv: &str,
    icaos: &HashSet<String>,
    faa_id_to_icao: &HashMap<String, String>,
) -> Vec<PreferredRoute> {
    let mut out = Vec::new();
    for line in csv.lines().skip(1) {
        if line.trim().is_empty() {
            continue;
        }
        let f = split_line(line);
        if f.len() != 14 {
            continue;
        }
        let (raw_orig, route_string, raw_dest, route_type, area, altitude, aircraft, direction, seq, dcntr, acntr) =
            (f[0], f[1], f[2], f[6], f[7], f[8], f[9], f[10], f[11], f[12], f[13]);
        // NAR rows' "Orig" is an oceanic entry/exit fix, not a departure
        // airport — there is nothing for a flight-plan lookup to match.
        if route_type == "NAR" {
            continue;
        }
        let Some(orig_icao) = resolve_icao(raw_orig, icaos, faa_id_to_icao) else {
            continue;
        };
        let Some(dest_icao) = resolve_icao(raw_dest, icaos, faa_id_to_icao) else {
            continue;
        };
        out.push(PreferredRoute {
            source: "PFR",
            orig_icao: orig_icao.to_string(),
            dest_icao: dest_icao.to_string(),
            route_string: strip_endpoints(route_string, raw_orig, raw_dest).to_string(),
            route_type: opt(route_type),
            altitude: opt(altitude),
            aircraft: opt(aircraft),
            direction: opt(direction),
            area: opt(area),
            code: None,
            dep_fix: None,
            coordination_required: None,
            nav_equipment: None,
            dep_artcc: opt(dcntr),
            arr_artcc: opt(acntr),
            seq: seq.trim().parse().ok(),
        });
    }
    out
}

fn parse_cdr(csv: &str, icaos: &HashSet<String>) -> Vec<PreferredRoute> {
    let mut out = Vec::new();
    for line in csv.lines().skip(1) {
        if line.trim().is_empty() {
            continue;
        }
        let f = split_line(line);
        if f.len() != 11 {
            continue;
        }
        let (code, raw_orig, raw_dest, dep_fix, route_string, dcntr, acntr, coord_req, nav_eqp) =
            (f[0], f[1], f[2], f[3], f[4], f[5], f[6], f[8], f[10]);
        // CDR's Orig/Dest are already full ICAO — just confirm this
        // bundle actually has the airport, the same FK-integrity filter
        // every other child table in this pipeline applies.
        if !icaos.contains(raw_orig) || !icaos.contains(raw_dest) {
            continue;
        }
        out.push(PreferredRoute {
            source: "CDR",
            orig_icao: raw_orig.to_string(),
            dest_icao: raw_dest.to_string(),
            route_string: strip_endpoints(route_string, raw_orig, raw_dest).to_string(),
            route_type: None,
            altitude: None,
            aircraft: None,
            direction: None,
            area: None,
            code: opt(code),
            dep_fix: opt(dep_fix),
            coordination_required: opt(coord_req),
            nav_equipment: opt(nav_eqp),
            dep_artcc: opt(dcntr),
            arr_artcc: opt(acntr),
            seq: None,
        });
    }
    out
}

/// Fetches both source databases fresh and resolves them against
/// `bundle_path`'s own `airport` table. Best-effort per source: a fetch
/// failure on one (FAA's servers, transient network) shouldn't cost the
/// other, so each is caught and logged rather than propagated — matching
/// the d-TPP/AIXM/openAIP steps' policy elsewhere in this pipeline.
pub fn fetch_and_parse(bundle_path: &Path) -> Result<Vec<PreferredRoute>, PreferredRoutesError> {
    let (icaos, faa_id_to_icao) = load_airport_lookup(bundle_path)?;
    let client = http_client();
    let mut routes = Vec::new();

    match client
        .get(PFR_URL)
        .send()
        .and_then(|r| r.error_for_status())
        .and_then(|r| r.text())
    {
        Ok(csv) => routes.extend(parse_pfr(&csv, &icaos, &faa_id_to_icao)),
        Err(err) => {
            tracing::warn!(error = %err, "couldn't fetch NFDC Preferred Routes Database — skipping")
        }
    }

    match client
        .get(CDR_URL)
        .send()
        .and_then(|r| r.error_for_status())
        .and_then(|r| r.text())
    {
        Ok(csv) => routes.extend(parse_cdr(&csv, &icaos)),
        Err(err) => {
            tracing::warn!(error = %err, "couldn't fetch Coded Departure Routes database — skipping")
        }
    }

    Ok(routes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_matching_endpoints_but_not_a_false_prefix() {
        assert_eq!(strip_endpoints("ABE FJC ARD CYN ACY", "ABE", "ACY"), "FJC ARD CYN");
        // Direct route: stripping both ends leaves nothing.
        assert_eq!(strip_endpoints("ABE ACY", "ABE", "ACY"), "");
        // "MT" must not eat the front of "MTHEW", which merely starts
        // with the same two letters.
        assert_eq!(strip_endpoints("MTHEW FOO BAR", "MT", "BAR"), "MTHEW FOO");
    }

    #[test]
    fn nar_rows_are_dropped_for_lack_of_a_real_departure_airport() {
        let icaos: HashSet<String> = ["KADW".to_string()].into_iter().collect();
        let faa_ids = HashMap::new();
        let csv = "Orig,Route String,Dest,Hours1,Hours2,Hours3,Type,Area,Altitude,Aircraft,Direction,Seq,DCNTR,ACNTR\n\
                    ALLEX,ALLEX LARIE Q220 RIFLE Q167 ZIZZI KNUKK ATR LAFLN SPISY2 ADW,ADW,,,,NAR,,,,WESTBOUND,1,,ZDC\n";
        assert!(parse_pfr(csv, &icaos, &faa_ids).is_empty());
    }

    #[test]
    fn pfr_resolves_faa_local_ids_and_already_icao_idents() {
        let icaos: HashSet<String> = ["KACY".to_string(), "CYUL".to_string()].into_iter().collect();
        let faa_ids: HashMap<String, String> = [
            ("ABE".to_string(), "KABE".to_string()),
            ("ACY".to_string(), "KACY".to_string()),
        ]
        .into_iter()
        .collect();
        let csv = "Orig,Route String,Dest,Hours1,Hours2,Hours3,Type,Area,Altitude,Aircraft,Direction,Seq,DCNTR,ACNTR\n\
                    ABE,ABE FJC ARD CYN ACY,ACY,,,,TEC,,5000,,,1,ZNY,ZDC\n";
        let routes = parse_pfr(csv, &icaos, &faa_ids);
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].orig_icao, "KABE");
        assert_eq!(routes[0].dest_icao, "KACY");
        assert_eq!(routes[0].route_string, "FJC ARD CYN");
        assert_eq!(routes[0].route_type.as_deref(), Some("TEC"));
        assert_eq!(routes[0].altitude.as_deref(), Some("5000"));
        assert_eq!(routes[0].seq, Some(1));
    }

    #[test]
    fn pfr_drops_rows_whose_endpoint_has_no_match_in_this_bundle() {
        let icaos: HashSet<String> = HashSet::new();
        let faa_ids = HashMap::new();
        let csv = "Orig,Route String,Dest,Hours1,Hours2,Hours3,Type,Area,Altitude,Aircraft,Direction,Seq,DCNTR,ACNTR\n\
                    ABE,ABE FJC ARD CYN ACY,ACY,,,,TEC,,5000,,,1,ZNY,ZDC\n";
        assert!(parse_pfr(csv, &icaos, &faa_ids).is_empty());
    }

    #[test]
    fn cdr_uses_already_icao_idents_directly() {
        let icaos: HashSet<String> = ["KABE".to_string(), "KCLT".to_string()].into_iter().collect();
        let csv = "RCode,Orig,Dest,DepFix,Route String,DCNTR,ACNTR,TCNTRs,CoordReq,Play,NavEqp\n\
                    ABECLTGV,KABE,KCLT,LRP,KABE LRP EMI GVE AIROW CHSLY8 KCLT,ZNY,ZTL,ZDC ZNY ZTL,N,,1\n";
        let routes = parse_cdr(csv, &icaos);
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].orig_icao, "KABE");
        assert_eq!(routes[0].dest_icao, "KCLT");
        assert_eq!(routes[0].route_string, "LRP EMI GVE AIROW CHSLY8");
        assert_eq!(routes[0].code.as_deref(), Some("ABECLTGV"));
        assert_eq!(routes[0].dep_fix.as_deref(), Some("LRP"));
        assert_eq!(routes[0].coordination_required.as_deref(), Some("N"));
    }

    #[test]
    fn cdr_drops_rows_for_airports_not_in_this_bundle() {
        let icaos: HashSet<String> = ["KABE".to_string()].into_iter().collect(); // KCLT missing
        let csv = "RCode,Orig,Dest,DepFix,Route String,DCNTR,ACNTR,TCNTRs,CoordReq,Play,NavEqp\n\
                    ABECLTGV,KABE,KCLT,LRP,KABE LRP EMI GVE AIROW CHSLY8 KCLT,ZNY,ZTL,ZDC ZNY ZTL,N,,1\n";
        assert!(parse_cdr(csv, &icaos).is_empty());
    }
}
