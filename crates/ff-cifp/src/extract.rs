//! Field extraction: [`RawRecord`] → typed rows → `ff_core` domain types.
//!
//! Column ranges below were cross-checked against the open-source
//! `arinc424` parser (github.com/jack-laverty/arinc424) rather than
//! guessed at — see that project's `airport.py`, `runway.py`,
//! `vhf_navaid.py`, `ndb_navaid.py`, `waypoint.py`, and
//! `sid_star_approach.py` for the field tables. Airport, Runway,
//! VHF/NDB Navaid, Waypoint, and SID/STAR/Approach legs are extracted;
//! Airway and enroute-communication records are classified by
//! `crate::record` but not yet extracted into `ff_core` types.
use crate::decode;
use crate::parser::CifpError;
use crate::record::{RawRecord, RecordCategory, RECORD_LENGTH};
use ff_core::{
    Airport, AirportType, AltitudeConstraint, Navaid, NavaidType, PathAndTerm, Procedure,
    ProcedureKind, ProcedureLeg, ProcedureTransition, Runway, RunwayEnd, RunwaySurface,
    SpeedConstraint, TransitionKind, TurnDirection, Waypoint,
};
use std::collections::BTreeMap;

fn ensure_length(line: &str) -> Result<(), CifpError> {
    if line.len() < RECORD_LENGTH {
        return Err(CifpError::LineTooShort);
    }
    Ok(())
}

fn field(line: &str, start: usize, end: usize) -> &str {
    &line[start..end]
}

/// 4.1.7.1 Airport Primary Records (PA). `airport_type` is always
/// [`AirportType::Airport`] here since [`RecordCategory::Airport`] is only
/// assigned to Section `P` records (heliports are a separate, not-yet
/// classified section); `fuel_types` and `faa_id` aren't present in the
/// CIFP and are left empty/`None` — they come from `ff-nasr` instead.
pub fn extract_airport(record: &RawRecord) -> Result<Airport, CifpError> {
    if record.category != RecordCategory::Airport {
        return Err(CifpError::WrongCategory);
    }
    let line = &record.raw;
    ensure_length(line)?;

    let icao = decode::non_empty(field(line, 6, 10))
        .ok_or(CifpError::MissingField("Airport ICAO Identifier"))?;
    let iata = decode::non_empty(field(line, 13, 16));
    let lat = decode::latitude(field(line, 32, 41))
        .ok_or(CifpError::InvalidField("Airport Reference Pt. Latitude"))?;
    let lon = decode::longitude(field(line, 41, 51))
        .ok_or(CifpError::InvalidField("Airport Reference Pt. Longitude"))?;
    let elevation_ft = decode::signed_int(field(line, 56, 61))
        .ok_or(CifpError::InvalidField("Airport Elevation"))?;
    let name =
        decode::non_empty(field(line, 93, 123)).ok_or(CifpError::MissingField("Airport Name"))?;

    Ok(Airport {
        icao,
        faa_id: None,
        iata,
        name,
        lat,
        lon,
        elevation_ft,
        airport_type: AirportType::Airport,
        fuel_types: Vec::new(),
    })
}

/// One runway *end* as coded by a single CIFP PG record (CIFP codes each
/// physical runway as two separate end records, e.g. "RW10L" and
/// "RW28R"); pair them with [`pair_runway_ends`] to build an
/// [`ff_core::Runway`]. `surface` isn't in the CIFP runway record at all
/// (it's an NASR-only field — see `ff-nasr::convert::runway_from_row`).
#[derive(Debug, Clone, PartialEq)]
pub struct CifpRunwayEnd {
    pub airport_icao: String,
    /// Runway number + optional L/C/R suffix, e.g. `"28L"` (the CIFP
    /// `"RW"` prefix is stripped).
    pub ident: String,
    pub lat: f64,
    pub lon: f64,
    pub length_ft: u32,
    pub width_ft: u32,
    pub magnetic_bearing_deg: Option<f64>,
}

/// 4.1.10.1 Runway Primary Records (PG).
pub fn extract_runway_end(record: &RawRecord) -> Result<CifpRunwayEnd, CifpError> {
    if record.category != RecordCategory::Runway {
        return Err(CifpError::WrongCategory);
    }
    let line = &record.raw;
    ensure_length(line)?;

    let airport_icao = decode::non_empty(field(line, 6, 10))
        .ok_or(CifpError::MissingField("Airport ICAO Identifier"))?;
    let raw_ident = field(line, 13, 18).trim();
    let ident = raw_ident
        .strip_prefix("RW")
        .unwrap_or(raw_ident)
        .trim()
        .to_string();
    if ident.is_empty() {
        return Err(CifpError::MissingField("Runway Identifier"));
    }
    let length_ft =
        decode::uint(field(line, 22, 27)).ok_or(CifpError::InvalidField("Runway Length"))?;
    let magnetic_bearing_deg = decode::tenths_of_degree(field(line, 27, 31));
    let lat =
        decode::latitude(field(line, 32, 41)).ok_or(CifpError::InvalidField("Runway Latitude"))?;
    let lon = decode::longitude(field(line, 41, 51))
        .ok_or(CifpError::InvalidField("Runway Longitude"))?;
    let width_ft = decode::uint(field(line, 77, 80)).unwrap_or(0);

    Ok(CifpRunwayEnd {
        airport_icao,
        ident,
        lat,
        lon,
        length_ft,
        width_ft,
        magnetic_bearing_deg,
    })
}

fn parse_runway_number_suffix(ident: &str) -> Option<(u32, Option<char>)> {
    let digits: String = ident.chars().take_while(|c| c.is_ascii_digit()).collect();
    let number: u32 = digits.parse().ok()?;
    let suffix = ident[digits.len()..].chars().next();
    Some((number, suffix))
}

fn reciprocal_number_suffix(number: u32, suffix: Option<char>) -> (u32, Option<char>) {
    let recip_number = if number > 18 {
        number - 18
    } else {
        number + 18
    };
    let recip_suffix = match suffix {
        Some('L') => Some('R'),
        Some('R') => Some('L'),
        other => other,
    };
    (recip_number, recip_suffix)
}

/// Pair up single-ended CIFP runway records into `ff_core::Runway`s by
/// matching each end's numeric heading (e.g. `"28L"`) to its reciprocal
/// (`"10R"`). An end with no matching reciprocal in `ends` (e.g. only one
/// direction was coded) still produces a `Runway`, with both ends set to
/// that same record rather than being dropped.
pub fn pair_runway_ends(ends: &[CifpRunwayEnd]) -> Vec<Runway> {
    let mut runways = Vec::new();
    let mut used = vec![false; ends.len()];

    for i in 0..ends.len() {
        if used[i] {
            continue;
        }
        let Some((number, suffix)) = parse_runway_number_suffix(&ends[i].ident) else {
            used[i] = true;
            continue;
        };
        let (recip_number, recip_suffix) = reciprocal_number_suffix(number, suffix);

        let partner_idx = ends.iter().enumerate().skip(i + 1).find_map(|(j, end)| {
            if used[j] || end.airport_icao != ends[i].airport_icao {
                return None;
            }
            (parse_runway_number_suffix(&end.ident) == Some((recip_number, recip_suffix)))
                .then_some(j)
        });

        used[i] = true;
        let (low, high) = match partner_idx {
            Some(j) => {
                used[j] = true;
                if number <= recip_number {
                    (&ends[i], &ends[j])
                } else {
                    (&ends[j], &ends[i])
                }
            }
            None => (&ends[i], &ends[i]),
        };

        runways.push(Runway {
            airport_icao: low.airport_icao.clone(),
            ident: format!("{}/{}", low.ident, high.ident),
            length_ft: low.length_ft.max(high.length_ft),
            width_ft: low.width_ft.max(high.width_ft),
            // CIFP doesn't encode surface; merge with ff-nasr for this.
            surface: RunwaySurface::Other,
            low_end: RunwayEnd {
                ident: low.ident.clone(),
                lat: low.lat,
                lon: low.lon,
                heading_deg: low.magnetic_bearing_deg.unwrap_or(0.0),
            },
            high_end: RunwayEnd {
                ident: high.ident.clone(),
                lat: high.lat,
                lon: high.lon,
                heading_deg: high.magnetic_bearing_deg.unwrap_or(0.0),
            },
        });
    }

    runways
}

/// 4.1.2.1 VHF NAVAID Primary Records (Section `D`, any subsection except
/// `B` — VOR, VOR/DME, VORTAC, or standalone DME/TACAN). Column ranges
/// verified against the open-source `arinc424` parser's `vhf_navaid.py`.
///
/// CIFP's NAVAID Class field (spec §5.35) would distinguish VOR-only
/// from VOR/DME from VORTAC, but its exact column layout isn't reliably
/// documented in the sources available here (the reference parser
/// itself leaves that field's decoder unimplemented), so it's left
/// unparsed. `navaid_type` is instead inferred from whether a DME Ident
/// sub-field is populated — a real, verified signal rather than a
/// guessed column offset — which collapses VOR/DME and VORTAC together
/// under [`NavaidType::VorDme`].
pub fn extract_vhf_navaid(record: &RawRecord) -> Result<Navaid, CifpError> {
    if record.category != RecordCategory::VhfNavaid {
        return Err(CifpError::WrongCategory);
    }
    let line = &record.raw;
    ensure_length(line)?;

    let ident =
        decode::non_empty(field(line, 13, 17)).ok_or(CifpError::MissingField("VOR Identifier"))?;
    let region = decode::non_empty(field(line, 10, 12))
        .ok_or(CifpError::MissingField("ICAO Region Code"))?;
    let freq_khz = decode::vor_frequency_khz(field(line, 22, 27));
    let lat =
        decode::latitude(field(line, 32, 41)).ok_or(CifpError::InvalidField("VOR Latitude"))?;
    let lon =
        decode::longitude(field(line, 41, 51)).ok_or(CifpError::InvalidField("VOR Longitude"))?;
    let has_dme = decode::non_empty(field(line, 51, 55)).is_some();
    // Only populated when a co-located DME exists; a VOR-only facility
    // has no elevation field at all in this record.
    let elevation_ft = decode::signed_int(field(line, 79, 84));

    Ok(Navaid {
        ident,
        navaid_type: if has_dme {
            NavaidType::VorDme
        } else {
            NavaidType::Vor
        },
        lat,
        lon,
        elevation_ft,
        freq_khz,
        region,
    })
}

/// 4.1.3.1 NDB NAVAID Primary Records (Section `P` subsection `N` for
/// airport-associated NDBs, or Section `D` subsection `B` for enroute
/// NDBs — both share this column layout, per
/// [`RecordCategory::NdbNavaid`]). Column ranges verified against the
/// `arinc424` parser's `ndb_navaid.py`.
pub fn extract_ndb_navaid(record: &RawRecord) -> Result<Navaid, CifpError> {
    if record.category != RecordCategory::NdbNavaid {
        return Err(CifpError::WrongCategory);
    }
    let line = &record.raw;
    ensure_length(line)?;

    let ident =
        decode::non_empty(field(line, 13, 17)).ok_or(CifpError::MissingField("NDB Identifier"))?;
    let region = decode::non_empty(field(line, 10, 12))
        .ok_or(CifpError::MissingField("ICAO Region Code"))?;
    let freq_khz = decode::ndb_frequency_khz(field(line, 22, 27));
    let lat =
        decode::latitude(field(line, 32, 41)).ok_or(CifpError::InvalidField("NDB Latitude"))?;
    let lon =
        decode::longitude(field(line, 41, 51)).ok_or(CifpError::InvalidField("NDB Longitude"))?;

    Ok(Navaid {
        ident,
        navaid_type: NavaidType::Ndb,
        lat,
        lon,
        // Not present in the NDB primary record at all.
        elevation_ft: None,
        freq_khz,
        region,
    })
}

/// 4.1.4.1 Waypoint Primary Records (Section `E`, enroute fixes —
/// [`RecordCategory::Waypoint`] already excludes the `EA`/`EU`
/// subsections, which are airways/enroute-communication records with a
/// different layout). Column ranges verified against the `arinc424`
/// parser's `waypoint.py` (its `enroute=True` field layout).
pub fn extract_waypoint(record: &RawRecord) -> Result<Waypoint, CifpError> {
    if record.category != RecordCategory::Waypoint {
        return Err(CifpError::WrongCategory);
    }
    let line = &record.raw;
    ensure_length(line)?;

    let ident = decode::non_empty(field(line, 13, 18))
        .ok_or(CifpError::MissingField("Waypoint Identifier"))?;
    let region = decode::non_empty(field(line, 10, 12))
        .ok_or(CifpError::MissingField("ICAO Region Code"))?;
    let lat = decode::latitude(field(line, 32, 41))
        .ok_or(CifpError::InvalidField("Waypoint Latitude"))?;
    let lon = decode::longitude(field(line, 41, 51))
        .ok_or(CifpError::InvalidField("Waypoint Longitude"))?;

    Ok(Waypoint {
        ident,
        lat,
        lon,
        region,
    })
}

/// One SID/STAR/Approach leg record (PD/PE/PF primary), extracted but not
/// yet grouped into a [`Procedure`]/[`ProcedureTransition`] — see
/// [`build_procedures`].
#[derive(Debug, Clone, PartialEq)]
pub struct ProcedureLegRow {
    pub airport_icao: String,
    pub kind: ProcedureKind,
    pub procedure_ident: String,
    /// Spec §5.7 Route Type: distinguishes runway/enroute transitions,
    /// common route, approach transition ('A'), and missed approach
    /// ('Z') — see `_ROUTE_TYPES` cross-referenced in `build_procedures`.
    pub route_type: char,
    /// Blank for legs on the procedure's common/missed-approach segment.
    pub transition_ident: String,
    pub seq: u32,
    pub fix_ident: String,
    pub path_and_term: PathAndTerm,
    pub turn_direction: Option<TurnDirection>,
    pub magnetic_course_deg: Option<f64>,
    pub altitude_desc: char,
    pub altitude1_ft: Option<u32>,
    pub altitude2_ft: Option<u32>,
    pub speed_limit_kt: Option<u32>,
}

/// 4.1.9.1 Airport SID/STAR/Approach Primary Records (PD/PE/PF).
pub fn extract_procedure_leg_row(record: &RawRecord) -> Result<ProcedureLegRow, CifpError> {
    let kind = match record.category {
        RecordCategory::Procedure(kind) => kind,
        _ => return Err(CifpError::WrongCategory),
    };
    let line = &record.raw;
    ensure_length(line)?;

    let airport_icao = decode::non_empty(field(line, 6, 10))
        .ok_or(CifpError::MissingField("Airport Identifier"))?;
    let procedure_ident = decode::non_empty(field(line, 13, 19))
        .ok_or(CifpError::MissingField("SID/STAR/Approach Identifier"))?;
    let route_type = field(line, 19, 20).chars().next().unwrap_or(' ');
    let transition_ident = field(line, 20, 25).trim().to_string();
    let seq =
        decode::uint(field(line, 26, 29)).ok_or(CifpError::InvalidField("Sequence Number"))?;
    let fix_ident = field(line, 29, 34).trim().to_string();
    let path_and_term = decode::path_and_term(field(line, 47, 49));
    let turn_direction = decode::turn_direction(field(line, 43, 44));
    let magnetic_course_deg = decode::tenths_of_degree(field(line, 70, 74));
    let altitude_desc = field(line, 82, 83).chars().next().unwrap_or(' ');
    let altitude1_ft = decode::uint(field(line, 84, 89));
    let altitude2_ft = decode::uint(field(line, 89, 94));
    let speed_limit_kt = decode::uint(field(line, 99, 102));

    Ok(ProcedureLegRow {
        airport_icao,
        kind,
        procedure_ident,
        route_type,
        transition_ident,
        seq,
        fix_ident,
        path_and_term,
        turn_direction,
        magnetic_course_deg,
        altitude_desc,
        altitude1_ft,
        altitude2_ft,
        speed_limit_kt,
    })
}

/// Spec §5.29 Altitude Description: `+`/`-`/`@`(or blank)/`B` selecting
/// which of `AtOrAbove`/`AtOrBelow`/`At`/`Between` the two altitude
/// fields encode. Unrecognized codes fall back to `At` on `altitude1`
/// rather than dropping the constraint.
fn altitude_constraint(
    desc: char,
    altitude1_ft: Option<u32>,
    altitude2_ft: Option<u32>,
) -> Option<AltitudeConstraint> {
    match desc {
        '+' => altitude1_ft.map(AltitudeConstraint::AtOrAbove),
        '-' => altitude1_ft.map(AltitudeConstraint::AtOrBelow),
        'B' => match (altitude1_ft, altitude2_ft) {
            (Some(a), Some(b)) => Some(AltitudeConstraint::Between {
                lower: a.min(b),
                upper: a.max(b),
            }),
            _ => None,
        },
        _ => altitude1_ft.map(AltitudeConstraint::At),
    }
}

/// Classifies a leg row's transition using the ARINC 424 §5.7 Route Type
/// codes (`_ROUTE_TYPES` in the reference parser): Approach uses `'A'`
/// for Approach Transition and `'Z'` for Missed Approach explicitly;
/// everything else is inferred from whether `transition_ident` is set.
///
/// SID/STAR route types further distinguish *runway* transitions from
/// *enroute* transitions (e.g. `'1'` vs `'3'`), but `ff_core::TransitionKind`
/// (DESIGN.md §6) only has one `Enroute` variant, so both collapse to it
/// here — revisit if that distinction becomes UI-relevant.
fn transition_kind(
    kind: ProcedureKind,
    route_type: char,
    has_transition_ident: bool,
) -> TransitionKind {
    if kind == ProcedureKind::Approach && route_type == 'Z' {
        return TransitionKind::Missed;
    }
    if !has_transition_ident {
        return TransitionKind::Common;
    }
    match kind {
        ProcedureKind::Approach if route_type == 'A' => TransitionKind::Approach,
        ProcedureKind::Approach => TransitionKind::Common,
        ProcedureKind::Sid | ProcedureKind::Star => TransitionKind::Enroute,
    }
}

fn procedure_id_for(airport_icao: &str, kind: ProcedureKind, ident: &str) -> String {
    let kind_str = match kind {
        ProcedureKind::Sid => "SID",
        ProcedureKind::Star => "STAR",
        ProcedureKind::Approach => "APPROACH",
    };
    format!("{airport_icao}:{kind_str}:{ident}")
}

/// Extracts a runway number + optional L/C/R suffix starting at the first
/// digit run in `s`, e.g. `"28L"` -> `Some("28L")`, `"10RZ"` -> `Some("10R")`
/// (the trailing `Z` is an FAA suffix distinguishing multiple similar
/// approaches, per the naming convention documented on
/// [`derive_approach_runway_ident`] — not part of the runway ident).
/// Returns `None` if `s` has no digit run (e.g. circling approaches like
/// `"VOR-A"`).
fn extract_runway_number(s: &str) -> Option<String> {
    let chars: Vec<char> = s.chars().collect();
    let start = chars.iter().position(|c| c.is_ascii_digit())?;
    let mut end = start;
    while end < chars.len() && chars[end].is_ascii_digit() {
        end += 1;
    }
    let mut runway: String = chars[start..end].iter().collect();
    if end < chars.len() && matches!(chars[end], 'L' | 'C' | 'R') {
        runway.push(chars[end]);
    }
    Some(runway)
}

/// Approach idents follow a well-known FAA convention: a 1-2 letter
/// approach-type code, then the runway number + optional L/C/R, then an
/// optional distinguishing suffix letter when multiple similar approaches
/// serve the same runway (e.g. `"H10RZ"` = the "Z" RNAV(RNP) approach to
/// runway 10R). Circling approaches with no specific runway (e.g.
/// `"VOR-A"`) have no digit run and correctly yield `None`.
fn derive_approach_runway_ident(procedure_ident: &str) -> Option<String> {
    extract_runway_number(procedure_ident)
}

/// SID/STAR transitions specific to one runway have a `transition_ident`
/// prefixed `"RW"` (e.g. `"RW28L"`); `"ALL"` and enroute-fix transitions
/// don't match and correctly yield `None`.
fn runway_transition_number(transition_ident: &str) -> Option<String> {
    extract_runway_number(transition_ident.strip_prefix("RW")?)
}

/// A SID/STAR "serves" a runway only when every one of its runway-specific
/// transitions agrees on the same runway; if it has several (a multi-runway
/// departure/arrival) or none, there's no single answer, so this yields
/// `None` rather than picking arbitrarily.
fn derive_sid_star_runway_ident(transitions: &[&ProcedureTransition]) -> Option<String> {
    let mut runway_idents = transitions
        .iter()
        .filter_map(|t| runway_transition_number(&t.ident));
    let first = runway_idents.next()?;
    if runway_idents.all(|r| r == first) {
        Some(first)
    } else {
        None
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ParsedProcedures {
    pub procedures: Vec<Procedure>,
    pub transitions: Vec<ProcedureTransition>,
    pub legs: Vec<ProcedureLeg>,
}

/// Groups flat [`ProcedureLegRow`]s (one per CIFP line) into
/// [`Procedure`]/[`ProcedureTransition`]/[`ProcedureLeg`] per the
/// DESIGN.md §6 schema: rows sharing (airport, kind, procedure ident)
/// become one `Procedure`; rows within that sharing the same transition
/// (or both being "common"/"missed", which have no real
/// `transition_ident` of their own) become one `ProcedureTransition`,
/// with legs ordered by CIFP sequence number.
///
/// A CIFP leg can have supplementary *continuation* records (RNP, vertical
/// guidance, FAS block data) that repeat the same (airport, procedure,
/// transition, sequence number) key as the leg they extend, with a
/// completely different field layout at the same column positions — none
/// of that supplementary data is modeled yet, but if it weren't skipped
/// here it would be misread as a second, malformed leg at the same
/// sequence number (this is where all "Unsupported" leg types with a
/// blank raw code come from in real CIFP data; verified by inspecting the
/// FAA's own continuation records, not assumed). Only the first record
/// seen for a given key is kept.
pub fn build_procedures(rows: &[ProcedureLegRow]) -> ParsedProcedures {
    const COMMON_LABEL: &str = "__COMMON__";
    const MISSED_LABEL: &str = "__MISSED__";

    let mut procedures: BTreeMap<String, Procedure> = BTreeMap::new();
    let mut transitions: BTreeMap<String, ProcedureTransition> = BTreeMap::new();
    let mut legs_by_transition: BTreeMap<String, Vec<(u32, ProcedureLeg)>> = BTreeMap::new();
    let mut seen_leg_keys: std::collections::HashSet<(String, String, String, u32)> =
        std::collections::HashSet::new();

    for row in rows {
        let leg_key = (
            row.airport_icao.clone(),
            row.procedure_ident.clone(),
            row.transition_ident.clone(),
            row.seq,
        );
        if !seen_leg_keys.insert(leg_key) {
            continue;
        }

        let procedure_id = procedure_id_for(&row.airport_icao, row.kind, &row.procedure_ident);
        procedures
            .entry(procedure_id.clone())
            .or_insert_with(|| Procedure {
                id: procedure_id.clone(),
                airport_icao: row.airport_icao.clone(),
                kind: row.kind,
                ident: row.procedure_ident.clone(),
                runway_ident: None,
            });

        let t_kind = transition_kind(row.kind, row.route_type, !row.transition_ident.is_empty());
        let label = match t_kind {
            TransitionKind::Common => COMMON_LABEL.to_string(),
            TransitionKind::Missed => MISSED_LABEL.to_string(),
            _ => row.transition_ident.clone(),
        };
        let transition_id = format!("{procedure_id}:{label}");
        transitions
            .entry(transition_id.clone())
            .or_insert_with(|| ProcedureTransition {
                id: transition_id.clone(),
                procedure_id: procedure_id.clone(),
                ident: if label == COMMON_LABEL || label == MISSED_LABEL {
                    String::new()
                } else {
                    label.clone()
                },
                kind: t_kind,
            });

        let leg = ProcedureLeg {
            transition_id: transition_id.clone(),
            seq: row.seq,
            path_and_term: row.path_and_term,
            fix_ident: decode::non_empty(&row.fix_ident),
            course_deg: row.magnetic_course_deg,
            altitude: altitude_constraint(row.altitude_desc, row.altitude1_ft, row.altitude2_ft),
            speed: row.speed_limit_kt.map(SpeedConstraint::AtOrBelow),
            turn_direction: row.turn_direction,
        };
        legs_by_transition
            .entry(transition_id)
            .or_default()
            .push((row.seq, leg));
    }

    let mut legs = Vec::new();
    for (_, mut rows) in legs_by_transition {
        rows.sort_by_key(|(seq, _)| *seq);
        legs.extend(rows.into_iter().map(|(_, leg)| leg));
    }

    let mut procedures: Vec<Procedure> = procedures.into_values().collect();
    for procedure in &mut procedures {
        procedure.runway_ident = match procedure.kind {
            ProcedureKind::Approach => derive_approach_runway_ident(&procedure.ident),
            ProcedureKind::Sid | ProcedureKind::Star => {
                let its_transitions: Vec<&ProcedureTransition> = transitions
                    .values()
                    .filter(|t| t.procedure_id == procedure.id)
                    .collect();
                derive_sid_star_runway_ident(&its_transitions)
            }
        };
    }

    ParsedProcedures {
        procedures,
        transitions: transitions.into_values().collect(),
        legs,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::classify_line;
    use crate::test_util::line_with;

    #[test]
    fn extracts_a_real_airport_record() {
        let line = line_with(&[
            (0, "S"),
            (1, "USA"),
            (4, "P"),
            (6, "KSFO"),
            (10, "K2"),
            (12, "A"),
            (13, "SFO"),
            (32, "N37370800"),
            (41, "W122223800"),
            (56, "00013"),
            (93, "SAN FRANCISCO INTL"),
        ]);
        let record = classify_line(&line).unwrap();
        assert_eq!(record.category, RecordCategory::Airport);

        let airport = extract_airport(&record).unwrap();
        assert_eq!(airport.icao, "KSFO");
        assert_eq!(airport.iata.as_deref(), Some("SFO"));
        assert_eq!(airport.elevation_ft, 13);
        assert_eq!(airport.name, "SAN FRANCISCO INTL");
        let expected_lat = 37.0 + 37.0 / 60.0 + 8.0 / 3600.0;
        let expected_lon = -(122.0 + 22.0 / 60.0 + 38.0 / 3600.0);
        assert!((airport.lat - expected_lat).abs() < 1e-6);
        assert!((airport.lon - expected_lon).abs() < 1e-6);
    }

    #[test]
    fn extract_airport_rejects_the_wrong_record_category() {
        let line = line_with(&[(0, "S"), (4, "P"), (6, "KSFO"), (12, "G")]);
        let record = classify_line(&line).unwrap();
        assert_eq!(record.category, RecordCategory::Runway);
        assert!(matches!(
            extract_airport(&record),
            Err(CifpError::WrongCategory)
        ));
    }

    fn runway_end_line(
        airport: &str,
        ident: &str,
        length_ft: &str,
        bearing_tenths: &str,
        lat: &str,
        lon: &str,
    ) -> String {
        line_with(&[
            (0, "S"),
            (4, "P"),
            (6, airport),
            (12, "G"),
            (13, ident),
            (22, length_ft),
            (27, bearing_tenths),
            (32, lat),
            (41, lon),
            (77, "200"),
        ])
    }

    #[test]
    fn extracts_and_pairs_runway_ends() {
        let low = runway_end_line("KSFO", "RW10L", "11870", "1050", "N37370000", "W122230000");
        let high = runway_end_line("KSFO", "RW28R", "11870", "2850", "N37373000", "W122210000");

        let low_record = classify_line(&low).unwrap();
        let high_record = classify_line(&high).unwrap();
        let low_end = extract_runway_end(&low_record).unwrap();
        let high_end = extract_runway_end(&high_record).unwrap();
        assert_eq!(low_end.ident, "10L");
        assert_eq!(high_end.ident, "28R");
        assert_eq!(low_end.width_ft, 200);
        assert_eq!(low_end.magnetic_bearing_deg, Some(105.0));

        let runways = pair_runway_ends(&[low_end, high_end]);
        assert_eq!(runways.len(), 1);
        let runway = &runways[0];
        assert_eq!(runway.ident, "10L/28R");
        assert_eq!(runway.length_ft, 11870);
        assert_eq!(runway.low_end.ident, "10L");
        assert_eq!(runway.high_end.ident, "28R");
    }

    #[test]
    fn unpaired_runway_end_still_produces_a_runway() {
        let only_end = runway_end_line("KKKK", "RW01", "05000", "0100", "N10000000", "W100000000");
        let record = classify_line(&only_end).unwrap();
        let end = extract_runway_end(&record).unwrap();
        let runways = pair_runway_ends(&[end]);
        assert_eq!(runways.len(), 1);
        assert_eq!(runways[0].ident, "01/01");
    }

    #[allow(clippy::too_many_arguments)]
    fn vhf_navaid_line(
        ident: &str,
        region: &str,
        freq: &str,
        lat: &str,
        lon: &str,
        dme_ident: &str,
        elevation: &str,
    ) -> String {
        line_with(&[
            (0, "S"),
            (4, "D"),
            (10, region),
            (13, ident),
            (22, freq),
            (32, lat),
            (41, lon),
            (51, dme_ident),
            (79, elevation),
        ])
    }

    #[test]
    fn extracts_a_vor_dme_record() {
        let line = vhf_navaid_line(
            "OSI",
            "K2",
            "11350",
            "N37370000",
            "W122230000",
            "OSI",
            "00100",
        );
        let record = classify_line(&line).unwrap();
        assert_eq!(record.category, RecordCategory::VhfNavaid);

        let navaid = extract_vhf_navaid(&record).unwrap();
        assert_eq!(navaid.ident, "OSI");
        assert_eq!(navaid.region, "K2");
        assert_eq!(navaid.navaid_type, NavaidType::VorDme);
        assert_eq!(navaid.freq_khz, Some(113_500));
        assert_eq!(navaid.elevation_ft, Some(100));
        assert!((navaid.lat - 37.6166666).abs() < 1e-4);
    }

    #[test]
    fn extracts_a_vor_only_record_with_no_dme() {
        let line = vhf_navaid_line("SGD", "K2", "11550", "N38300000", "W121490000", "", "");
        let record = classify_line(&line).unwrap();
        let navaid = extract_vhf_navaid(&record).unwrap();
        assert_eq!(navaid.navaid_type, NavaidType::Vor);
        assert_eq!(navaid.elevation_ft, None);
    }

    fn ndb_navaid_line(ident: &str, region: &str, freq: &str, lat: &str, lon: &str) -> String {
        line_with(&[
            (0, "S"),
            (4, "D"),
            (5, "B"),
            (10, region),
            (13, ident),
            (22, freq),
            (32, lat),
            (41, lon),
        ])
    }

    #[test]
    fn extracts_an_ndb_navaid_record() {
        let line = ndb_navaid_line("OAK", "K2", "03650", "N37430000", "W122120000");
        let record = classify_line(&line).unwrap();
        assert_eq!(record.category, RecordCategory::NdbNavaid);

        let navaid = extract_ndb_navaid(&record).unwrap();
        assert_eq!(navaid.ident, "OAK");
        assert_eq!(navaid.navaid_type, NavaidType::Ndb);
        assert_eq!(navaid.freq_khz, Some(365));
        assert_eq!(navaid.elevation_ft, None);
    }

    fn waypoint_line(ident: &str, region: &str, lat: &str, lon: &str) -> String {
        line_with(&[
            (0, "S"),
            (4, "E"),
            (10, region),
            (13, ident),
            (32, lat),
            (41, lon),
        ])
    }

    #[test]
    fn extracts_an_enroute_waypoint_record() {
        let line = waypoint_line("FIXAB", "K2", "N38000000", "W122000000");
        let record = classify_line(&line).unwrap();
        assert_eq!(record.category, RecordCategory::Waypoint);

        let waypoint = extract_waypoint(&record).unwrap();
        assert_eq!(waypoint.ident, "FIXAB");
        assert_eq!(waypoint.region, "K2");
        assert_eq!(waypoint.lat, 38.0);
        assert_eq!(waypoint.lon, -122.0);
    }

    #[allow(clippy::too_many_arguments)]
    fn procedure_leg_line(
        airport: &str,
        subsection: &str,
        procedure_ident: &str,
        route_type: &str,
        transition_ident: &str,
        seq: &str,
        fix_ident: &str,
        path_and_term: &str,
        altitude_desc: &str,
        altitude1: &str,
    ) -> String {
        line_with(&[
            (0, "S"),
            (4, "P"),
            (6, airport),
            (12, subsection),
            (13, procedure_ident),
            (19, route_type),
            (20, transition_ident),
            (26, seq),
            (29, fix_ident),
            (47, path_and_term),
            (70, "0900"),
            (82, altitude_desc),
            (84, altitude1),
        ])
    }

    #[test]
    fn extracts_a_real_sid_leg_record() {
        let line = procedure_leg_line(
            "KSFO", "D", "TEST1 ", "2", "     ", "010", "FIXA ", "TF", "+", "03000",
        );
        let record = classify_line(&line).unwrap();
        assert_eq!(
            record.category,
            RecordCategory::Procedure(ProcedureKind::Sid)
        );

        let row = extract_procedure_leg_row(&record).unwrap();
        assert_eq!(row.airport_icao, "KSFO");
        assert_eq!(row.procedure_ident, "TEST1");
        assert_eq!(row.route_type, '2');
        assert_eq!(row.transition_ident, "");
        assert_eq!(row.seq, 10);
        assert_eq!(row.fix_ident, "FIXA");
        assert_eq!(row.path_and_term, PathAndTerm::TF);
        assert_eq!(row.magnetic_course_deg, Some(90.0));
        assert_eq!(row.altitude_desc, '+');
        assert_eq!(row.altitude1_ft, Some(3000));
    }

    fn leg_row(
        kind: ProcedureKind,
        procedure_ident: &str,
        route_type: char,
        transition_ident: &str,
        seq: u32,
        fix_ident: &str,
    ) -> ProcedureLegRow {
        ProcedureLegRow {
            airport_icao: "KSFO".to_string(),
            kind,
            procedure_ident: procedure_ident.to_string(),
            route_type,
            transition_ident: transition_ident.to_string(),
            seq,
            fix_ident: fix_ident.to_string(),
            path_and_term: PathAndTerm::TF,
            turn_direction: None,
            magnetic_course_deg: None,
            altitude_desc: ' ',
            altitude1_ft: None,
            altitude2_ft: None,
            speed_limit_kt: None,
        }
    }

    #[test]
    fn build_procedures_groups_sid_transitions_and_orders_legs_by_sequence() {
        let rows = vec![
            // Out of sequence order on purpose, to prove sort-by-seq works.
            leg_row(ProcedureKind::Sid, "TEST1", '2', "", 20, "FIXB"),
            leg_row(ProcedureKind::Sid, "TEST1", '2', "", 10, "FIXA"),
            leg_row(ProcedureKind::Sid, "TEST1", '3', "TRANA", 10, "FIXC"),
        ];
        let parsed = build_procedures(&rows);

        assert_eq!(parsed.procedures.len(), 1);
        assert_eq!(parsed.procedures[0].kind, ProcedureKind::Sid);
        assert_eq!(parsed.procedures[0].ident, "TEST1");

        assert_eq!(parsed.transitions.len(), 2);
        let common = parsed
            .transitions
            .iter()
            .find(|t| t.kind == TransitionKind::Common)
            .unwrap();
        assert_eq!(common.ident, "");
        let enroute = parsed
            .transitions
            .iter()
            .find(|t| t.kind == TransitionKind::Enroute)
            .unwrap();
        assert_eq!(enroute.ident, "TRANA");

        let common_legs: Vec<_> = parsed
            .legs
            .iter()
            .filter(|l| l.transition_id == common.id)
            .collect();
        assert_eq!(common_legs.len(), 2);
        assert_eq!(common_legs[0].fix_ident.as_deref(), Some("FIXA"));
        assert_eq!(common_legs[1].fix_ident.as_deref(), Some("FIXB"));
    }

    #[test]
    fn build_procedures_classifies_approach_transition_and_missed_approach() {
        let rows = vec![
            leg_row(ProcedureKind::Approach, "ILS28L", 'A', "IAF1", 10, "IAF1"),
            leg_row(ProcedureKind::Approach, "ILS28L", 'I', "", 20, "FAF"),
            leg_row(ProcedureKind::Approach, "ILS28L", 'Z', "", 30, "MISFIX"),
        ];
        let parsed = build_procedures(&rows);

        assert_eq!(parsed.transitions.len(), 3);
        let has_kind = |kind: TransitionKind| parsed.transitions.iter().any(|t| t.kind == kind);
        assert!(has_kind(TransitionKind::Approach));
        assert!(has_kind(TransitionKind::Common));
        assert!(has_kind(TransitionKind::Missed));
        assert_eq!(parsed.procedures[0].runway_ident.as_deref(), Some("28L"));
    }

    #[test]
    fn derives_approach_runway_ident_from_common_faa_naming_conventions() {
        assert_eq!(
            derive_approach_runway_ident("I28L"),
            Some("28L".to_string())
        );
        assert_eq!(derive_approach_runway_ident("R31"), Some("31".to_string()));
        assert_eq!(
            derive_approach_runway_ident("H10RZ"),
            Some("10R".to_string())
        );
        assert_eq!(
            derive_approach_runway_ident("L19L"),
            Some("19L".to_string())
        );
        // Circling approaches have no specific runway.
        assert_eq!(derive_approach_runway_ident("VOR-A"), None);
    }

    #[test]
    fn derives_sid_star_runway_ident_only_when_transitions_agree() {
        let one_runway = [
            ProcedureTransition {
                id: "t1".into(),
                procedure_id: "p1".into(),
                ident: "RW28L".into(),
                kind: TransitionKind::Enroute,
            },
            ProcedureTransition {
                id: "t2".into(),
                procedure_id: "p1".into(),
                ident: "OSI".into(),
                kind: TransitionKind::Enroute,
            },
        ];
        let refs: Vec<&ProcedureTransition> = one_runway.iter().collect();
        assert_eq!(derive_sid_star_runway_ident(&refs), Some("28L".to_string()));

        let two_runways = [
            ProcedureTransition {
                id: "t1".into(),
                procedure_id: "p1".into(),
                ident: "RW28L".into(),
                kind: TransitionKind::Enroute,
            },
            ProcedureTransition {
                id: "t2".into(),
                procedure_id: "p1".into(),
                ident: "RW10R".into(),
                kind: TransitionKind::Enroute,
            },
        ];
        let refs: Vec<&ProcedureTransition> = two_runways.iter().collect();
        assert_eq!(derive_sid_star_runway_ident(&refs), None);
    }

    #[test]
    fn build_procedures_derives_sid_runway_ident_from_a_single_runway_transition() {
        let rows = vec![
            leg_row(ProcedureKind::Sid, "TEST1", '2', "", 10, "FIXA"),
            leg_row(ProcedureKind::Sid, "TEST1", '1', "RW28L", 10, "RW28L"),
        ];
        let parsed = build_procedures(&rows);
        assert_eq!(parsed.procedures[0].runway_ident.as_deref(), Some("28L"));
    }

    #[test]
    fn altitude_constraint_decodes_all_description_codes() {
        assert_eq!(
            altitude_constraint('+', Some(3000), None),
            Some(AltitudeConstraint::AtOrAbove(3000))
        );
        assert_eq!(
            altitude_constraint('-', Some(3000), None),
            Some(AltitudeConstraint::AtOrBelow(3000))
        );
        assert_eq!(
            altitude_constraint('@', Some(3000), None),
            Some(AltitudeConstraint::At(3000))
        );
        assert_eq!(
            altitude_constraint('B', Some(4000), Some(2000)),
            Some(AltitudeConstraint::Between {
                lower: 2000,
                upper: 4000
            })
        );
    }
}
