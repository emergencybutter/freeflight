//! One-off tool to build a small SQLite demo bundle (a handful of real
//! airports) so the web client has real data to render without the full
//! ff-etl pipeline (§7, still a stub) being wired up yet.
//!
//! Usage:
//! ```sh
//! cargo run -p ff-etl --example build_demo_bundle -- <cifp-file> <output.sqlite> [--nasr-dir <dir>] ICAO1 [ICAO2 ...]
//! ```
//!
//! `--nasr-dir` points at a directory containing an unzipped NASR 28-day
//! CSV subscription (`APT_BASE.csv`, `APT_RWY.csv`, `APT_RWY_END.csv`,
//! `FRQ.csv`). When given, real runway surface type and airport
//! communication frequencies are merged in on top of the CIFP-derived
//! data — CIFP alone has neither.
use ff_cifp::{
    build_procedures, classify_line, extract_airport, extract_procedure_leg_row,
    extract_runway_end, pair_runway_ends, RecordCategory,
};
use ff_core::{
    AirportType, AltitudeConstraint, Frequency, FrequencyKind, PathAndTerm, ProcedureKind, Runway,
    RunwaySurface, SpeedConstraint, TransitionKind, TurnDirection,
};
use ff_nasr::{
    frequencies_for_airport, parse_apt_base, parse_apt_runway, parse_apt_runway_end, parse_frq,
};
use rusqlite::params;
use std::collections::{HashMap, HashSet};
use std::{env, fs};

struct Args {
    cifp_path: String,
    output_path: String,
    nasr_dir: Option<String>,
    icaos: HashSet<String>,
}

fn parse_args() -> Args {
    let raw: Vec<String> = env::args().collect();
    if raw.len() < 4 {
        eprintln!("usage: build_demo_bundle <cifp-file> <output.sqlite> [--nasr-dir <dir>] ICAO1 [ICAO2 ...]");
        std::process::exit(1);
    }
    let cifp_path = raw[1].clone();
    let output_path = raw[2].clone();
    let mut nasr_dir = None;
    let mut icaos = HashSet::new();
    let mut rest = raw[3..].iter().peekable();
    while let Some(arg) = rest.next() {
        if arg == "--nasr-dir" {
            nasr_dir = rest.next().cloned();
        } else {
            icaos.insert(arg.to_uppercase());
        }
    }
    Args {
        cifp_path,
        output_path,
        nasr_dir,
        icaos,
    }
}

/// Parses the NASR extract at `dir` and returns real runway surfaces
/// (keyed by `(airport_icao, runway_ident)`) and communication
/// frequencies for just the requested `icaos`, merging on top of what
/// CIFP alone can provide (DESIGN.md §9.1 — CIFP has no surface type or
/// comm frequencies at all).
fn load_nasr_enrichment(
    dir: &str,
    icaos: &HashSet<String>,
) -> (HashMap<(String, String), RunwaySurface>, Vec<Frequency>) {
    let airports =
        parse_apt_base(&fs::read(format!("{dir}/APT_BASE.csv")).expect("read APT_BASE.csv"))
            .expect("parse APT_BASE.csv");
    let runways =
        parse_apt_runway(&fs::read(format!("{dir}/APT_RWY.csv")).expect("read APT_RWY.csv"))
            .expect("parse APT_RWY.csv");
    let runway_ends = parse_apt_runway_end(
        &fs::read(format!("{dir}/APT_RWY_END.csv")).expect("read APT_RWY_END.csv"),
    )
    .expect("parse APT_RWY_END.csv");
    let freqs = parse_frq(&fs::read(format!("{dir}/FRQ.csv")).expect("read FRQ.csv"))
        .expect("parse FRQ.csv");

    // ARPT_ID (FAA local id, e.g. "SFO") -> our airport_icao (e.g. "KSFO"):
    // APT_RWY/APT_RWY_END/FRQ are keyed by ARPT_ID, not ICAO.
    let wanted: HashMap<String, String> = airports
        .iter()
        .filter_map(|a| {
            let icao = a.icao_id.clone().unwrap_or_else(|| a.arpt_id.clone());
            icaos.contains(&icao).then(|| (a.arpt_id.clone(), icao))
        })
        .collect();

    let mut surfaces = HashMap::new();
    for rwy in &runways {
        let Some(icao) = wanted.get(&rwy.arpt_id) else {
            continue;
        };
        let ends: Vec<_> = runway_ends
            .iter()
            .filter(|e| e.arpt_id == rwy.arpt_id)
            .cloned()
            .collect();
        let runway = ff_nasr::runway_from_rows(rwy, &ends, icao);
        surfaces.insert((icao.clone(), runway.ident), runway.surface);
    }

    let mut frequencies = Vec::new();
    for (arpt_id, icao) in &wanted {
        frequencies.extend(frequencies_for_airport(&freqs, arpt_id, icao));
    }

    (surfaces, frequencies)
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

fn main() {
    let args = parse_args();
    let icaos = &args.icaos;

    let contents = fs::read_to_string(&args.cifp_path).expect("failed to read CIFP file");

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
                    if icaos.contains(&airport.icao) {
                        airports.push(airport);
                    }
                }
            }
            RecordCategory::Runway => {
                if let Ok(end) = extract_runway_end(&record) {
                    if icaos.contains(&end.airport_icao) {
                        runway_ends.push(end);
                    }
                }
            }
            RecordCategory::Procedure(_) => {
                if let Ok(row) = extract_procedure_leg_row(&record) {
                    if icaos.contains(&row.airport_icao) {
                        leg_rows.push(row);
                    }
                }
            }
            _ => {}
        }
    }

    let mut runways = pair_runway_ends(&runway_ends);
    let parsed = build_procedures(&leg_rows);

    let frequencies = if let Some(nasr_dir) = &args.nasr_dir {
        let (surfaces, frequencies) = load_nasr_enrichment(nasr_dir, icaos);
        apply_nasr_surfaces(&mut runways, &surfaces);
        frequencies
    } else {
        Vec::new()
    };

    if fs::metadata(&args.output_path).is_ok() {
        fs::remove_file(&args.output_path).expect("failed to remove stale output file");
    }
    let conn = ff_storage::open(&args.output_path).expect("failed to open/migrate sqlite");

    for a in &airports {
        conn.execute(
            "INSERT INTO airport (icao, faa_id, iata, name, lat, lon, elevation_ft, airport_type, fuel_types)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, '')",
            params![a.icao, a.faa_id, a.iata, a.name, a.lat, a.lon, a.elevation_ft, airport_type_str(a.airport_type)],
        )
        .expect("insert airport");
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
        )
        .expect("insert runway");
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
        )
        .expect("insert frequency");
    }

    for p in &parsed.procedures {
        conn.execute(
            "INSERT INTO procedure (id, airport_icao, kind, ident, runway_ident) VALUES (?1,?2,?3,?4,?5)",
            params![p.id, p.airport_icao, procedure_kind_str(p.kind), p.ident, p.runway_ident],
        )
        .expect("insert procedure");
    }
    for t in &parsed.transitions {
        conn.execute(
            "INSERT INTO procedure_transition (id, procedure_id, ident, kind) VALUES (?1,?2,?3,?4)",
            params![t.id, t.procedure_id, t.ident, transition_kind_str(t.kind)],
        )
        .expect("insert transition");
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
        )
        .expect("insert leg");
    }

    println!(
        "wrote {} airports, {} runways, {} frequencies, {} procedures, {} transitions, {} legs to {}",
        airports.len(),
        runways.len(),
        frequencies.len(),
        parsed.procedures.len(),
        parsed.transitions.len(),
        parsed.legs.len(),
        args.output_path,
    );
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

fn turn_direction_str(t: TurnDirection) -> &'static str {
    match t {
        TurnDirection::Left => "L",
        TurnDirection::Right => "R",
        TurnDirection::Either => "E",
    }
}
