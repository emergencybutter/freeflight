//! Fetches the current cycle's FAA d-TPP (Digital Terminal Procedures
//! Publication) metadata and matches its SID/STAR/Approach chart entries
//! against this bundle's own `procedure` rows, producing `dtpp_chart`
//! rows (one per confidently-matched procedure) for `add_dtpp_charts` to
//! insert.
//!
//! The actual PDF stays hosted on `aeronav.faa.gov` — this only stores
//! the URL, precomputed from the metafile's own `pdf_name` field, the
//! same "no re-hosting needed" approach as linking rather than mirroring.
//! Confirmed live: `https://aeronav.faa.gov/d-tpp/<cycle>/<pdf_name>` is
//! a plain public PDF, no auth, no robots.txt restriction on `/d-tpp/`.
//!
//! d-TPP's own `YYCC` cycle numbering (e.g. `2607`) doesn't match this
//! app's CIFP-derived `cycle_date` (`YYYY-MM-DD`) — the directory lists
//! several cycles at once (past/current/future all present
//! simultaneously, confirmed live), so the current one is discovered the
//! same "list candidates, try newest-first, confirm effectiveness"
//! technique `fetch::discover_chart_cycle` already uses for sectionals,
//! just checking the metafile's own `from_edate`/`to_edate` header
//! (fetched via an HTTP Range request — no need to pull the whole ~16MB
//! file just to pick a cycle) rather than trying a chart file.
use crate::fetch::http_client;
use chrono::NaiveDate;
use quick_xml::events::Event;
use quick_xml::reader::Reader;
use rusqlite::Connection;
use std::collections::HashMap;
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DtppError {
    #[error("http request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("xml parse error: {0}")]
    Xml(#[from] quick_xml::Error),
    #[error("no currently-effective d-TPP cycle found under https://aeronav.faa.gov/d-tpp/")]
    NoCycleFound,
}

/// One resolved chart to add to `dtpp_chart` — a d-TPP metafile record
/// that was confidently matched to one of this bundle's own procedure
/// idents.
pub struct MatchedDtppChart {
    pub airport_icao: String,
    pub procedure_ident: String,
    pub chart_name: String,
    pub pdf_url: String,
    pub cycle: String,
}

/// Scans a d-TPP directory listing for `YYCC/` cycle folder names (IIS-
/// style markup, confirmed live: uppercase `HREF=`, unlike the lowercase
/// `href=` sectional-chart listings elsewhere in this crate).
fn dtpp_cycle_folders(listing_html: &str) -> Vec<String> {
    let mut cycles = Vec::new();
    let mut rest = listing_html;
    while let Some(pos) = rest.find("/d-tpp/") {
        rest = &rest[pos + "/d-tpp/".len()..];
        if rest.len() >= 4 {
            let candidate = &rest[..4];
            if candidate.chars().all(|c| c.is_ascii_digit()) {
                cycles.push(candidate.to_string());
            }
        }
    }
    cycles.sort();
    cycles.dedup();
    cycles
}

/// Parses `from_edate`/`to_edate` off the metafile's root `<digital_tpp>`
/// tag, e.g. `"0901Z  07/09/26"` — a Zulu time prefix (ignored) then
/// `MM/DD/YY` (the only part needed to tell if a cycle is currently
/// effective).
fn parse_edate(raw: &str) -> Option<NaiveDate> {
    let date_part = raw.split_whitespace().last()?;
    NaiveDate::parse_from_str(date_part, "%m/%d/%y").ok()
}

/// Lists available d-TPP cycle folders, then — newest first — checks
/// each candidate's own effective-date header (a cheap Range request,
/// not the full ~16MB metafile) until one currently covers today.
pub fn discover_dtpp_cycle() -> Result<String, DtppError> {
    let client = http_client();
    let listing = client
        .get("https://aeronav.faa.gov/d-tpp/")
        .send()?
        .error_for_status()?
        .text()?;
    let mut cycles = dtpp_cycle_folders(&listing);
    cycles.sort();
    cycles.reverse();

    let today = chrono::Local::now().date_naive();
    for cycle in &cycles {
        let url = format!("https://aeronav.faa.gov/d-tpp/{cycle}/xml_data/d-TPP_Metafile.xml");
        let resp = client.get(&url).header("Range", "bytes=0-300").send()?;
        if !resp.status().is_success() {
            continue;
        }
        let head = resp.text()?;
        let from = head
            .split("from_edate=\"")
            .nth(1)
            .and_then(|s| s.split('"').next())
            .and_then(parse_edate);
        let to = head
            .split("to_edate=\"")
            .nth(1)
            .and_then(|s| s.split('"').next())
            .and_then(parse_edate);
        if let (Some(from), Some(to)) = (from, to) {
            if from <= today && today <= to {
                return Ok(cycle.clone());
            }
        }
    }
    Err(DtppError::NoCycleFound)
}

/// One `<record>` from the metafile, kept only for `chart_code`s this
/// app links (SIDs/ODPs/STARs/Approaches — not airport diagrams, takeoff/
/// alternate minimums, or hot spots).
struct DtppRecord {
    icao_ident: String,
    chart_code: String,
    chart_name: String,
    pdf_name: String,
    faanfd18: String,
}

const WANTED_CHART_CODES: [&str; 4] = ["DP", "ODP", "STR", "IAP"];

/// Streams the metafile rather than building a full DOM — it's ~16MB and
/// this only needs a handful of fields off each `<record>`, tagged with
/// whichever `<airport_name icao_ident=...>` it's nested under.
fn parse_dtpp_records(xml: &[u8]) -> Result<Vec<DtppRecord>, DtppError> {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().trim_text(true);

    let mut records = Vec::new();
    let mut current_icao = String::new();
    let mut in_record = false;
    let mut current_tag = String::new();
    let mut chart_code = String::new();
    let mut chart_name = String::new();
    let mut pdf_name = String::new();
    let mut faanfd18 = String::new();

    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Eof => break,
            Event::Start(e) => {
                let name = e.name();
                let local = String::from_utf8_lossy(name.as_ref()).into_owned();
                if local == "airport_name" {
                    current_icao = e
                        .attributes()
                        .flatten()
                        .find(|a| a.key.as_ref() == b"icao_ident")
                        .map(|a| String::from_utf8_lossy(&a.value).into_owned())
                        .unwrap_or_default();
                } else if local == "record" {
                    in_record = true;
                    chart_code.clear();
                    chart_name.clear();
                    pdf_name.clear();
                    faanfd18.clear();
                }
                current_tag = local;
            }
            Event::Text(e) => {
                if in_record {
                    let text = e.unescape()?.into_owned();
                    match current_tag.as_str() {
                        "chart_code" => chart_code = text,
                        "chart_name" => chart_name = text,
                        "pdf_name" => pdf_name = text,
                        "faanfd18" => faanfd18 = text,
                        _ => {}
                    }
                }
            }
            Event::End(e) if e.name().as_ref() == b"record" => {
                in_record = false;
                if !current_icao.is_empty()
                    && WANTED_CHART_CODES.contains(&chart_code.as_str())
                    && !pdf_name.is_empty()
                {
                    records.push(DtppRecord {
                        icao_ident: current_icao.clone(),
                        chart_code: chart_code.clone(),
                        chart_name: chart_name.clone(),
                        pdf_name: pdf_name.clone(),
                        faanfd18: faanfd18.clone(),
                    });
                }
            }
            _ => {}
        }
        buf.clear();
    }
    Ok(records)
}

/// ARINC 424 approach-type ident prefix -> the `chart_name` phrase(s)
/// that prefix corresponds to. Verified empirically (not from spec
/// recall) against ~250 real approach idents across 100 random airports
/// cross-referenced with their actual current d-TPP chart names: ~95%
/// matched with this table. The unmapped remainder is real: a handful of
/// idents (e.g. an `S`-prefixed one) had no corresponding chart phrase in
/// any sample checked, and a few `H`-prefixed idents wanted a specific
/// suffix/runway combination no current chart actually has — rather than
/// guess, those simply won't resolve to a chart, same as any prefix not
/// listed here at all.
const APPROACH_TYPE_TABLE: &[(&str, &[&str])] = &[
    ("I", &["ILS"]),
    ("L", &["LOC", "LOC/DME"]),
    ("LOC", &["LOC", "LOC/DME"]),
    ("B", &["LOC BC", "LOC/BC"]),
    ("LBC", &["LOC BC", "LOC/BC"]),
    ("R", &["RNAV (GPS)", "RNAV (RNP)"]),
    ("RNV", &["RNAV (GPS)", "RNAV (RNP)"]),
    ("RNVA", &["RNAV (GPS)", "RNAV (RNP)"]),
    ("V", &["VOR"]),
    ("VOR", &["VOR"]),
    ("D", &["VOR/DME"]),
    ("VDM", &["VOR/DME"]),
    ("N", &["NDB"]),
    ("NDB", &["NDB"]),
    ("Q", &["NDB/DME"]),
    ("T", &["TACAN"]),
    ("H", &["HI-TACAN", "HI-ILS", "HI-LOC", "COPTER"]),
    ("X", &["LDA"]),
    ("LDA", &["LDA"]),
    ("G", &["GLS", "IGS"]),
    ("P", &["GPS"]),
    ("GPS", &["GPS"]),
];

/// Splits `chart_name` into its alternative approach-type phrases
/// (`"ILS OR LOC RWY 04L"` -> `["ILS", "LOC"]`), the runway (`"04L"`, or
/// `None` for a circling-only chart), and the trailing suffix letter
/// distinguishing multiple approaches of the same type/runway
/// (`"RNAV (GPS) Y RWY 04L"` -> `Some('Y')`; `"VOR-A"` -> `Some('A')` with
/// no runway). The overall suffix is found once (the single letter right
/// before " RWY", or after a trailing "-" for a circling chart), then
/// stripped again from *each* phrase individually after splitting on
/// " OR " — `"ILS Y OR LOC Y RWY 18"` carries it after every alternative,
/// not just the last, so a single whole-string strip isn't enough.
fn parse_chart_name(name: &str) -> (Vec<String>, Option<String>, Option<char>) {
    let mut suffix = None;
    if let Some(rwy_pos) = name.find(" RWY") {
        let before_rwy = &name[..rwy_pos];
        if let Some(last_word) = before_rwy.split_whitespace().last() {
            if last_word.len() == 1 {
                if let Some(c) = last_word.chars().next() {
                    if c.is_ascii_uppercase() {
                        suffix = Some(c);
                    }
                }
            }
        }
    } else if name.len() >= 2 && name.as_bytes()[name.len() - 2] == b'-' {
        if let Some(c) = name.chars().last() {
            if c.is_ascii_uppercase() {
                suffix = Some(c);
            }
        }
    }

    let runway = name.find(" RWY").map(|pos| {
        name[pos + 4..]
            .trim_start()
            .split(|c: char| !c.is_ascii_alphanumeric())
            .next()
            .unwrap_or("")
            .to_string()
    });

    let type_part = match name.find(" RWY") {
        Some(pos) => &name[..pos],
        None => match name.rfind('-') {
            Some(dash) if dash + 2 == name.len() => &name[..dash],
            _ => name,
        },
    };
    let phrases = type_part
        .split(" OR ")
        .map(|p| {
            let p = p.trim();
            match p.rsplit_once(' ') {
                Some((rest, last))
                    if last.len() == 1
                        && last.chars().next().is_some_and(|c| c.is_ascii_uppercase()) =>
                {
                    rest.to_string()
                }
                _ => p.to_string(),
            }
        })
        .collect();
    (phrases, runway, suffix)
}

fn candidate_prefixes(phrases: &[String]) -> Vec<&'static str> {
    APPROACH_TYPE_TABLE
        .iter()
        .filter(|(_, known)| known.iter().any(|k| phrases.iter().any(|p| p == k)))
        .map(|(prefix, _)| *prefix)
        .collect()
}

/// Splits an approach ident into its leading letter-prefix and trailing
/// suffix letter distinguishing multiple approaches of the same type/
/// runway — e.g. `R04LY` -> `("R", Some('Y'))`, `VOR-A` -> `("VOR",
/// Some('A'))`, `I04L` -> `("I", None)`. The runway itself isn't parsed
/// here — this app's own `procedure.runway_ident` column already has it
/// (validated empirically against that column directly, not a re-parse
/// of the ident string, which is simpler and was what got the ~95% match
/// rate above).
fn parse_approach_ident(ident: &str) -> (String, Option<char>) {
    let prefix_len = ident
        .find(|c: char| !c.is_ascii_uppercase())
        .unwrap_or(ident.len());
    let prefix = &ident[..prefix_len];
    let rest = ident[prefix_len..].trim_start_matches('-');

    let digit_end = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    if digit_end == 0 {
        // No runway digits at all -- a pure circling suffix, e.g. "-A".
        let suffix = rest.chars().next().filter(|c| c.is_ascii_uppercase());
        return (prefix.to_string(), suffix);
    }
    let after_digits = rest[digit_end..].trim_start_matches('-');
    let suffix_part = match after_digits.chars().next() {
        Some(c) if c == 'L' || c == 'R' || c == 'C' => &after_digits[1..],
        _ => after_digits,
    };
    let suffix_part = suffix_part.trim_start_matches('-');
    let suffix = suffix_part
        .chars()
        .next()
        .filter(|c| c.is_ascii_uppercase());
    (prefix.to_string(), suffix)
}

/// This airport's procedures already parsed into the bundle being built
/// — `(ident, runway_ident, kind)`, `kind` one of `SID`/`STAR`/`APPROACH`.
type AirportProcedures = Vec<(String, Option<String>, String)>;

/// Matches one metafile record against this airport's known procedure
/// idents. SID/STAR match exactly via `faanfd18` (SIDs are written
/// `{ident}.{transition}`, STARs `{transition}.{ident}` — confirmed
/// against real KJFK data, e.g. `DEEZZ6.DEEZZ` vs `SIE.CAMRN5`).
/// Approaches have no `faanfd18` and go through the type/runway/suffix
/// heuristic above.
fn match_record(record: &DtppRecord, procedures: &AirportProcedures) -> Option<String> {
    match record.chart_code.as_str() {
        "DP" | "ODP" => {
            let ident = record.faanfd18.split('.').next()?;
            procedures
                .iter()
                .find(|(pi, _, kind)| kind == "SID" && pi == ident)
                .map(|(pi, _, _)| pi.clone())
        }
        "STR" => {
            let ident = record.faanfd18.split('.').next_back()?;
            procedures
                .iter()
                .find(|(pi, _, kind)| kind == "STAR" && pi == ident)
                .map(|(pi, _, _)| pi.clone())
        }
        "IAP" => {
            let (phrases, runway, suffix) = parse_chart_name(&record.chart_name);
            let prefixes = candidate_prefixes(&phrases);
            if prefixes.is_empty() {
                return None;
            }
            procedures
                .iter()
                .find(|(pi, runway_ident, kind)| {
                    if kind != "APPROACH" {
                        return false;
                    }
                    if runway_ident.as_deref() != runway.as_deref() {
                        return false;
                    }
                    let (ident_prefix, ident_suffix) = parse_approach_ident(pi);
                    prefixes.contains(&ident_prefix.as_str()) && ident_suffix == suffix
                })
                .map(|(pi, _, _)| pi.clone())
        }
        _ => None,
    }
}

/// Downloads and parses the given cycle's d-TPP metafile, then matches
/// its SID/STAR/Approach records against every procedure already in the
/// bundle being built (`bundle_path`, opened read-only here — the
/// `procedure` table is populated earlier in the same pipeline run).
pub fn fetch_and_match_dtpp_charts(
    bundle_path: &Path,
    cycle: &str,
) -> Result<Vec<MatchedDtppChart>, DtppError> {
    let client = http_client();
    let url = format!("https://aeronav.faa.gov/d-tpp/{cycle}/xml_data/d-TPP_Metafile.xml");
    let xml = client.get(&url).send()?.error_for_status()?.bytes()?;
    let records = parse_dtpp_records(&xml)?;

    let conn = Connection::open(bundle_path)?;
    let mut by_airport: HashMap<String, AirportProcedures> = HashMap::new();
    let mut stmt = conn.prepare("SELECT airport_icao, ident, runway_ident, kind FROM procedure")?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, String>(3)?,
        ))
    })?;
    for row in rows {
        let (airport_icao, ident, runway_ident, kind) = row?;
        by_airport
            .entry(airport_icao)
            .or_default()
            .push((ident, runway_ident, kind));
    }

    let mut matched = Vec::new();
    for record in &records {
        let Some(procedures) = by_airport.get(&record.icao_ident) else {
            continue;
        };
        if let Some(procedure_ident) = match_record(record, procedures) {
            matched.push(MatchedDtppChart {
                airport_icao: record.icao_ident.clone(),
                procedure_ident,
                chart_name: record.chart_name.clone(),
                pdf_url: format!("https://aeronav.faa.gov/d-tpp/{cycle}/{}", record.pdf_name),
                cycle: cycle.to_string(),
            });
        }
    }
    Ok(matched)
}
