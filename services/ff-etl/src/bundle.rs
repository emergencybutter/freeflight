//! Parses a CIFP file (plus optional NASR enrichment and chart imagery)
//! into an `ff-storage`-schema SQLite bundle. Shared by the real pipeline
//! (`pipeline.rs`) and the `build_demo_bundle` example, which differ only
//! in *how* they obtain their inputs (fetched from FAA vs. given as local
//! file paths) — the parsing/insertion logic here is identical either way
//! and was validated against a real CIFP cycle file and NASR subscription
//! (see git history) before this module existed, so it's moved here
//! unchanged rather than rewritten.
use ff_charts::{geotiff_to_pmtiles, ChartCatalogEntry, ChartKind, GeoTiffSource};
use ff_cifp::{
    build_procedures, classify_line, extract_airport, extract_ndb_navaid,
    extract_procedure_leg_row, extract_runway_end, extract_vhf_navaid, extract_waypoint,
    pair_runway_ends, RecordCategory,
};
use ff_core::{
    AirportType, AltitudeConstraint, Frequency, FrequencyKind, Navaid, NavaidType, PathAndTerm,
    ProcedureKind, Runway, RunwaySurface, SpeedConstraint, TransitionKind, TurnDirection, Waypoint,
};
use ff_nasr::{
    frequencies_for_airport, parse_apt_base, parse_apt_runway, parse_apt_runway_end, parse_frq,
};
use rusqlite::params;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BundleError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("failed to parse CIFP/NASR source data: {0}")]
    Parse(String),
    #[error("failed to convert chart GeoTIFF to PMTiles: {0}")]
    Chart(#[from] ff_charts::ChartIngestError),
}

/// A source chart GeoTIFF to run through `ff-charts::geotiff_to_pmtiles`
/// and add as a `chart_catalog` row.
///
/// `id` must be unique per `chart_catalog` row (its primary key) — the
/// real pipeline now adds one row per sectional per cycle, so it can't be
/// derived from `cycle_id` alone the way a single-chart-per-cycle bundle
/// could. `tile_url` is what gets stored in the catalog — i.e. the URL
/// clients will fetch tiles from — and depends on who serves the file:
/// the `build_demo_bundle` example uses a site-root-relative path for
/// Vite, the real pipeline uses `ff-api`'s `/bundles/<cycle>/<file>`
/// route.
pub struct ChartSource {
    pub id: String,
    pub geotiff_path: PathBuf,
    pub pmtiles_out: PathBuf,
    pub cycle_id: String,
    pub name: String,
    pub tile_url: String,
}

/// Everything needed to build one cycle bundle: a CIFP file, optionally a
/// NASR extract directory for enrichment, optionally chart imagery, and
/// optionally a set of ICAOs to restrict to (`None` = the whole file,
/// i.e. a nationwide bundle — CIFP/NASR cover the whole US per file).
pub struct BundleSource {
    pub cifp_path: PathBuf,
    pub nasr_dir: Option<PathBuf>,
    pub chart: Option<ChartSource>,
    pub icaos: Option<HashSet<String>>,
}

fn wanted(icaos: &Option<HashSet<String>>, icao: &str) -> bool {
    match icaos {
        Some(set) => set.contains(icao),
        None => true,
    }
}

#[derive(Debug, Default)]
pub struct BundleStats {
    pub airports: usize,
    pub runways: usize,
    pub frequencies: usize,
    pub procedures: usize,
    pub transitions: usize,
    pub legs: usize,
    pub navaids: usize,
    pub waypoints: usize,
    pub has_chart: bool,
}

/// Parses `source` and writes an `ff-storage`-schema SQLite bundle to
/// `output_path`, overwriting any existing file there.
pub fn build_bundle(source: &BundleSource, output_path: &Path) -> Result<BundleStats, BundleError> {
    let icaos = &source.icaos;
    let contents = fs::read_to_string(&source.cifp_path)?;

    let mut airports = Vec::new();
    let mut runway_ends = Vec::new();
    let mut leg_rows = Vec::new();

    for line in contents.lines() {
        let Some(record) = classify_line(line) else {
            continue;
        };
        match record.category {
            RecordCategory::Airport => {
                if let Ok(airport) = extract_airport(&record) {
                    if wanted(icaos, &airport.icao) {
                        airports.push(airport);
                    }
                }
            }
            RecordCategory::Runway => {
                if let Ok(end) = extract_runway_end(&record) {
                    if wanted(icaos, &end.airport_icao) {
                        runway_ends.push(end);
                    }
                }
            }
            RecordCategory::Procedure(_) => {
                if let Ok(row) = extract_procedure_leg_row(&record) {
                    if wanted(icaos, &row.airport_icao) {
                        leg_rows.push(row);
                    }
                }
            }
            _ => {}
        }
    }

    // Every child table references airport(icao) by foreign key, and at
    // nationwide scope the sources genuinely disagree about the airport
    // set: NASR has frequencies for thousands of airports CIFP has no
    // airport record for, and CIFP itself has runway/procedure records
    // for a handful of airports whose airport record fails extraction.
    // Keep only children of airports actually going into the bundle —
    // anything else would fail the FK constraint at insert time (which
    // is exactly how this was discovered; a 5-airport region never hit
    // it).
    let airport_icaos: HashSet<&str> = airports.iter().map(|a| a.icao.as_str()).collect();
    runway_ends.retain(|end| airport_icaos.contains(end.airport_icao.as_str()));
    leg_rows.retain(|row| airport_icaos.contains(row.airport_icao.as_str()));

    let mut runways = pair_runway_ends(&runway_ends);
    let parsed = build_procedures(&leg_rows);

    // Resolve procedure leg fixes to real coordinates: a second pass over
    // the file (cheap — `contents` is already in memory) picking out just
    // the VHF/NDB navaids and waypoints actually referenced by these
    // airports' procedures, rather than every navaid/waypoint in the
    // country. Runway-threshold pseudo-fixes (e.g. "RW28L") won't match
    // anything here and are simply skipped by consumers.
    let wanted_fixes: HashSet<String> = parsed
        .legs
        .iter()
        .filter_map(|l| l.fix_ident.clone())
        .collect();
    let mut navaids: Vec<Navaid> = Vec::new();
    let mut waypoints: Vec<Waypoint> = Vec::new();
    let mut seen_navaid_idents = HashSet::new();
    let mut seen_waypoint_idents = HashSet::new();
    for line in contents.lines() {
        let Some(record) = classify_line(line) else {
            continue;
        };
        match record.category {
            RecordCategory::VhfNavaid => {
                if let Ok(navaid) = extract_vhf_navaid(&record) {
                    if wanted_fixes.contains(&navaid.ident)
                        && seen_navaid_idents.insert(navaid.ident.clone())
                    {
                        navaids.push(navaid);
                    }
                }
            }
            RecordCategory::NdbNavaid => {
                if let Ok(navaid) = extract_ndb_navaid(&record) {
                    if wanted_fixes.contains(&navaid.ident)
                        && seen_navaid_idents.insert(navaid.ident.clone())
                    {
                        navaids.push(navaid);
                    }
                }
            }
            RecordCategory::Waypoint => {
                if let Ok(waypoint) = extract_waypoint(&record) {
                    if wanted_fixes.contains(&waypoint.ident)
                        && seen_waypoint_idents.insert(waypoint.ident.clone())
                    {
                        waypoints.push(waypoint);
                    }
                }
            }
            _ => {}
        }
    }

    let mut frequencies = if let Some(nasr_dir) = &source.nasr_dir {
        let (surfaces, frequencies) = load_nasr_enrichment(nasr_dir, icaos)?;
        apply_nasr_surfaces(&mut runways, &surfaces);
        frequencies
    } else {
        Vec::new()
    };
    // Same FK-integrity filter as runways/legs above, for NASR-sourced
    // frequencies whose airport CIFP doesn't know.
    frequencies.retain(|f| airport_icaos.contains(f.airport_icao.as_str()));

    if fs::metadata(output_path).is_ok() {
        fs::remove_file(output_path)?;
    }
    let output_path_str = output_path
        .to_str()
        .expect("output path must be valid UTF-8");
    let mut conn =
        ff_storage::open(output_path_str).map_err(|e| BundleError::Parse(e.to_string()))?;
    // One transaction around all inserts: a nationwide bundle writes
    // hundreds of thousands of rows, and SQLite fsyncs per statement in
    // autocommit mode — per-row commits took minutes, one transaction
    // takes seconds.
    let conn = conn.transaction()?;

    for a in &airports {
        conn.execute(
            "INSERT INTO airport (icao, faa_id, iata, name, lat, lon, elevation_ft, airport_type, fuel_types)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, '')",
            params![a.icao, a.faa_id, a.iata, a.name, a.lat, a.lon, a.elevation_ft, airport_type_str(a.airport_type)],
        )?;
    }

    for r in &runways {
        conn.execute(
            "INSERT INTO runway (airport_icao, ident, length_ft, width_ft, surface,
                                  le_ident, le_lat, le_lon, le_heading_deg,
                                  he_ident, he_lat, he_lon, he_heading_deg)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
            params![
                r.airport_icao,
                r.ident,
                r.length_ft,
                r.width_ft,
                surface_str(r.surface),
                r.low_end.ident,
                r.low_end.lat,
                r.low_end.lon,
                r.low_end.heading_deg,
                r.high_end.ident,
                r.high_end.lat,
                r.high_end.lon,
                r.high_end.heading_deg,
            ],
        )?;
    }

    for f in &frequencies {
        conn.execute(
            "INSERT INTO frequency (airport_icao, kind, freq_mhz, remarks) VALUES (?1,?2,?3,?4)",
            params![
                f.airport_icao,
                frequency_kind_str(f.kind),
                f.freq_mhz,
                f.remarks
            ],
        )?;
    }

    for p in &parsed.procedures {
        conn.execute(
            "INSERT INTO procedure (id, airport_icao, kind, ident, runway_ident) VALUES (?1,?2,?3,?4,?5)",
            params![p.id, p.airport_icao, procedure_kind_str(p.kind), p.ident, p.runway_ident],
        )?;
    }
    for t in &parsed.transitions {
        conn.execute(
            "INSERT INTO procedure_transition (id, procedure_id, ident, kind) VALUES (?1,?2,?3,?4)",
            params![t.id, t.procedure_id, t.ident, transition_kind_str(t.kind)],
        )?;
    }
    for l in &parsed.legs {
        conn.execute(
            "INSERT INTO procedure_leg (transition_id, seq, path_and_term, fix_ident, course_deg,
                                         altitude_constraint, speed_constraint, turn_direction)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                l.transition_id,
                l.seq,
                path_and_term_str(l.path_and_term),
                l.fix_ident,
                l.course_deg,
                l.altitude.map(altitude_str),
                l.speed.map(speed_str),
                l.turn_direction.map(turn_direction_str),
            ],
        )?;
    }

    for n in &navaids {
        conn.execute(
            "INSERT INTO navaid (ident, navaid_type, lat, lon, elevation_ft, freq_khz, region)
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![
                n.ident,
                navaid_type_str(n.navaid_type),
                n.lat,
                n.lon,
                n.elevation_ft,
                n.freq_khz,
                n.region
            ],
        )?;
    }
    for w in &waypoints {
        conn.execute(
            "INSERT INTO waypoint (ident, lat, lon, region) VALUES (?1,?2,?3,?4)",
            params![w.ident, w.lat, w.lon, w.region],
        )?;
    }

    conn.commit()?;

    if let Some(chart_source) = &source.chart {
        add_chart(output_path, chart_source)?;
    }

    Ok(BundleStats {
        airports: airports.len(),
        runways: runways.len(),
        frequencies: frequencies.len(),
        procedures: parsed.procedures.len(),
        transitions: parsed.transitions.len(),
        legs: parsed.legs.len(),
        navaids: navaids.len(),
        waypoints: waypoints.len(),
        has_chart: source.chart.is_some(),
    })
}

/// Runs `chart.geotiff_path` through `ff-charts::geotiff_to_pmtiles`
/// (writing `chart.pmtiles_out`) and inserts the matching
/// `chart_catalog` row into an already-built bundle at `bundle_path`.
/// Split out from [`build_bundle`] so the real pipeline can add the
/// chart *after* the bundle exists — the crop bbox is derived from the
/// bundle's own airports (see `pipeline.rs`), which don't exist until
/// the bundle is built.
pub fn add_chart(bundle_path: &Path, chart: &ChartSource) -> Result<(), BundleError> {
    let geotiff = GeoTiffSource {
        path: chart.geotiff_path.clone(),
        kind: ChartKind::Sectional,
        cycle_id: chart.cycle_id.clone(),
    };
    let bbox = geotiff_to_pmtiles(&geotiff, &chart.pmtiles_out)?;
    let entry = ChartCatalogEntry {
        id: chart.id.clone(),
        name: chart.name.clone(),
        kind: ChartKind::Sectional,
        cycle_id: chart.cycle_id.clone(),
        bbox,
        tile_url: chart.tile_url.clone(),
    };

    let conn = rusqlite::Connection::open(bundle_path)?;
    conn.execute(
        "INSERT INTO chart_catalog (id, name, kind, cycle_id, min_lat, min_lon, max_lat, max_lon, tile_url)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
        params![
            entry.id,
            entry.name,
            chart_kind_str(entry.kind),
            entry.cycle_id,
            entry.bbox.min_lat,
            entry.bbox.min_lon,
            entry.bbox.max_lat,
            entry.bbox.max_lon,
            entry.tile_url,
        ],
    )?;
    Ok(())
}

/// Parses the NASR extract at `dir` and returns real runway surfaces
/// (keyed by `(airport_icao, runway_ident)`) and communication
/// frequencies for just the requested `icaos`, merging on top of what
/// CIFP alone can provide (DESIGN.md §9.1 — CIFP has no surface type or
/// comm frequencies at all).
type SurfacesByAirportAndRunway = HashMap<(String, String), RunwaySurface>;

fn load_nasr_enrichment(
    dir: &Path,
    icaos: &Option<HashSet<String>>,
) -> Result<(SurfacesByAirportAndRunway, Vec<Frequency>), BundleError> {
    let airports = parse_apt_base(&fs::read(dir.join("APT_BASE.csv"))?)
        .map_err(|e| BundleError::Parse(format!("APT_BASE.csv: {e}")))?;
    let runways = parse_apt_runway(&fs::read(dir.join("APT_RWY.csv"))?)
        .map_err(|e| BundleError::Parse(format!("APT_RWY.csv: {e}")))?;
    let runway_ends = parse_apt_runway_end(&fs::read(dir.join("APT_RWY_END.csv"))?)
        .map_err(|e| BundleError::Parse(format!("APT_RWY_END.csv: {e}")))?;
    let freqs = parse_frq(&fs::read(dir.join("FRQ.csv"))?)
        .map_err(|e| BundleError::Parse(format!("FRQ.csv: {e}")))?;

    // ARPT_ID (FAA local id, e.g. "SFO") -> our airport_icao (e.g. "KSFO"):
    // APT_RWY/APT_RWY_END/FRQ are keyed by ARPT_ID, not ICAO.
    let wanted_airports: HashMap<String, String> = airports
        .iter()
        .filter_map(|a| {
            let icao = a.icao_id.clone().unwrap_or_else(|| a.arpt_id.clone());
            wanted(icaos, &icao).then(|| (a.arpt_id.clone(), icao))
        })
        .collect();

    // Group the per-airport child rows once, up front — the per-runway
    // "scan every runway end in the country" version of this was fine
    // for a 5-airport region but quadratic (~45k × ~45k) once bundles
    // went nationwide.
    let mut ends_by_arpt: HashMap<&str, Vec<ff_nasr::AptRunwayEndRow>> = HashMap::new();
    for end in &runway_ends {
        ends_by_arpt
            .entry(end.arpt_id.as_str())
            .or_default()
            .push(end.clone());
    }
    let mut freqs_by_facility: HashMap<&str, Vec<ff_nasr::FrqRow>> = HashMap::new();
    for freq in &freqs {
        freqs_by_facility
            .entry(freq.serviced_facility.as_str())
            .or_default()
            .push(freq.clone());
    }
    static NO_ENDS: &[ff_nasr::AptRunwayEndRow] = &[];
    static NO_FREQS: &[ff_nasr::FrqRow] = &[];

    let mut surfaces = HashMap::new();
    for rwy in &runways {
        let Some(icao) = wanted_airports.get(&rwy.arpt_id) else {
            continue;
        };
        let ends = ends_by_arpt
            .get(rwy.arpt_id.as_str())
            .map_or(NO_ENDS, |v| v.as_slice());
        let runway = ff_nasr::runway_from_rows(rwy, ends, icao);
        surfaces.insert((icao.clone(), runway.ident), runway.surface);
    }

    let mut frequencies = Vec::new();
    for (arpt_id, icao) in &wanted_airports {
        let rows = freqs_by_facility
            .get(arpt_id.as_str())
            .map_or(NO_FREQS, |v| v.as_slice());
        frequencies.extend(frequencies_for_airport(rows, arpt_id, icao));
    }

    Ok((surfaces, frequencies))
}

fn apply_nasr_surfaces(
    runways: &mut [Runway],
    surfaces: &HashMap<(String, String), RunwaySurface>,
) {
    for runway in runways.iter_mut() {
        if let Some(surface) = surfaces.get(&(runway.airport_icao.clone(), runway.ident.clone())) {
            runway.surface = *surface;
        }
    }
}

fn airport_type_str(t: AirportType) -> &'static str {
    match t {
        AirportType::Airport => "Airport",
        AirportType::Heliport => "Heliport",
        AirportType::Seaplane => "Seaplane",
        AirportType::Ultralight => "Ultralight",
    }
}

fn surface_str(s: RunwaySurface) -> &'static str {
    match s {
        RunwaySurface::Asphalt => "Asphalt",
        RunwaySurface::Concrete => "Concrete",
        RunwaySurface::Turf => "Turf",
        RunwaySurface::Gravel => "Gravel",
        RunwaySurface::Water => "Water",
        RunwaySurface::Other => "Other",
    }
}

fn frequency_kind_str(k: FrequencyKind) -> &'static str {
    match k {
        FrequencyKind::Ctaf => "CTAF",
        FrequencyKind::Unicom => "UNICOM",
        FrequencyKind::Tower => "TWR",
        FrequencyKind::Ground => "GND",
        FrequencyKind::Approach => "APP",
        FrequencyKind::Departure => "DEP",
        FrequencyKind::Atis => "ATIS",
        FrequencyKind::Awos => "AWOS",
        FrequencyKind::Clearance => "CLNC DEL",
        FrequencyKind::Other => "OTHER",
    }
}

fn procedure_kind_str(k: ProcedureKind) -> &'static str {
    match k {
        ProcedureKind::Sid => "SID",
        ProcedureKind::Star => "STAR",
        ProcedureKind::Approach => "APPROACH",
    }
}

fn transition_kind_str(k: TransitionKind) -> &'static str {
    match k {
        TransitionKind::Enroute => "ENROUTE",
        TransitionKind::Common => "COMMON",
        TransitionKind::Approach => "APPROACH",
        TransitionKind::Missed => "MISSED",
    }
}

fn path_and_term_str(p: PathAndTerm) -> String {
    format!("{p:?}")
}

fn altitude_str(a: AltitudeConstraint) -> String {
    match a {
        AltitudeConstraint::At(ft) => format!("At {ft} ft"),
        AltitudeConstraint::AtOrAbove(ft) => format!("At/above {ft} ft"),
        AltitudeConstraint::AtOrBelow(ft) => format!("At/below {ft} ft"),
        AltitudeConstraint::Between { lower, upper } => format!("Between {lower}-{upper} ft"),
    }
}

fn speed_str(s: SpeedConstraint) -> String {
    match s {
        SpeedConstraint::AtOrBelow(kt) => format!("At/below {kt} kt"),
        SpeedConstraint::At(kt) => format!("At {kt} kt"),
    }
}

fn chart_kind_str(k: ChartKind) -> &'static str {
    match k {
        ChartKind::Sectional => "Sectional",
        ChartKind::TerminalAreaChart => "TerminalAreaChart",
        ChartKind::WorldAeronauticalChart => "WorldAeronauticalChart",
        ChartKind::IfrEnrouteLow => "IfrEnrouteLow",
        ChartKind::IfrEnrouteHigh => "IfrEnrouteHigh",
        ChartKind::HelicopterRoute => "HelicopterRoute",
        ChartKind::TerminalProcedurePlate => "TerminalProcedurePlate",
    }
}

fn navaid_type_str(t: NavaidType) -> &'static str {
    match t {
        NavaidType::Vor => "Vor",
        NavaidType::VorDme => "VorDme",
        NavaidType::Vortac => "Vortac",
        NavaidType::Ndb => "Ndb",
        NavaidType::Dme => "Dme",
        NavaidType::Tacan => "Tacan",
    }
}

fn turn_direction_str(t: TurnDirection) -> &'static str {
    match t {
        TurnDirection::Left => "L",
        TurnDirection::Right => "R",
        TurnDirection::Either => "E",
    }
}
