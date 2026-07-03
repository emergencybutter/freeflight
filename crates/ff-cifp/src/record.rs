//! ARINC 424 record classification.
//!
//! Every CIFP record is a fixed-width (132 column) line. Column 5
//! (1-indexed, index 4) always carries the Section Code. Where the
//! *Subsection* code lives depends on the section: sections keyed to an
//! airport/heliport identifier (Airport, Runway, SID/STAR/Approach — all
//! Section `P`) carry a 4-character identifier at columns 7-10 and a
//! 2-character ICAO region code at columns 11-12, which pushes their
//! subsection code out to column 13 (index 12); column 6 (index 5) is a
//! spare/blank column for those records. Other sections (VHF/NDB navaid,
//! enroute) put the subsection at column 6 (index 5) instead.
//!
//! This dual-position handling — try index 5 first, fall back to index
//! 12 — mirrors the approach taken by the open-source `arinc424` parser
//! (github.com/jack-laverty/arinc424, see `record.py`'s
//! `identifier_1`/`identifier_2` fallback), which was used to cross-check
//! these offsets against a real, working implementation rather than
//! guessing at the ARINC 424 spec from memory.
use ff_core::ProcedureKind;

pub const RECORD_LENGTH: usize = 132;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordCategory {
    Airport,
    Runway,
    VhfNavaid,
    NdbNavaid,
    Waypoint,
    Airway,
    EnrouteCommunication,
    Procedure(ProcedureKind),
    Unknown,
}

/// A single fixed-width CIFP line plus its classification. Field
/// extraction from `raw` is done by `crate::extract`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawRecord {
    pub raw: String,
    pub category: RecordCategory,
}

fn category_for(section: char, subsection: char) -> Option<RecordCategory> {
    match (section, subsection) {
        ('P', 'A') => Some(RecordCategory::Airport),
        ('P', 'G') => Some(RecordCategory::Runway),
        ('P', 'D') => Some(RecordCategory::Procedure(ProcedureKind::Sid)),
        ('P', 'E') => Some(RecordCategory::Procedure(ProcedureKind::Star)),
        ('P', 'F') => Some(RecordCategory::Procedure(ProcedureKind::Approach)),
        ('P', 'N') => Some(RecordCategory::NdbNavaid),
        // Terminal (airport-area) waypoint — the RNAV fixes SID/STAR/
        // Approach legs actually reference at most airports, as opposed
        // to 'EA' enroute waypoints. Missing this dropped nearly every
        // procedure-leg fix nationwide, silently: procedures still
        // parsed fine, but almost no leg's `fix_ident` resolved to a
        // coordinate, so the map had too few points per transition to
        // draw a line. Confirmed against a real record (KSFO's DOTNE,
        // referenced by the H10RZ approach): same `extract_waypoint`
        // column layout as 'EA' (ident 13:18, region 19:21, lat 32:41,
        // lon 41:51) — the airport-ident prefix that pushes the
        // subsection code out to column 13 for Section P records
        // doesn't shift any of the waypoint's own fields.
        ('P', 'C') => Some(RecordCategory::Waypoint),
        ('D', 'B') => Some(RecordCategory::NdbNavaid),
        ('D', _) => Some(RecordCategory::VhfNavaid),
        // Verified against a real FAA CIFP cycle file: 'EA' records carry
        // waypoint idents/lat/lon (e.g. "AAITT" with real coordinates),
        // while 'ER' records carry an airway ident plus a sequence of leg
        // fixes/altitude limits — the reverse of what an earlier,
        // unverified pass at this mapping assumed.
        ('E', 'A') => Some(RecordCategory::Waypoint),
        ('E', 'R') => Some(RecordCategory::Airway),
        ('E', 'U') => Some(RecordCategory::EnrouteCommunication),
        _ => None,
    }
}

/// Classify a single CIFP line by its section/subsection code.
///
/// Returns `None` if the line is too short to contain a section code and
/// both candidate subsection positions (e.g. a trailing blank line)
/// rather than guessing at a partial record.
pub fn classify_line(line: &str) -> Option<RawRecord> {
    if line.len() < 13 {
        return None;
    }
    let bytes = line.as_bytes();
    let section = bytes[4] as char;
    let subsection_col6 = bytes[5] as char;
    let subsection_col13 = bytes[12] as char;

    let category = category_for(section, subsection_col6)
        .or_else(|| category_for(section, subsection_col13))
        .unwrap_or(RecordCategory::Unknown);

    Some(RawRecord {
        raw: line.to_string(),
        category,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::line_with;

    #[test]
    fn short_lines_are_ignored() {
        assert!(classify_line("SUSP").is_none());
    }

    #[test]
    fn classifies_airport_records_via_the_column_13_subsection() {
        // Airport ident at [6,10), ICAO region at [10,12), subsection 'A' at
        // index 12 — column 6 (index 5) stays blank, as in real CIFP PA rows.
        let line = line_with(&[(0, "SUSAP"), (6, "KSFO"), (10, "K2"), (12, "A")]);
        let record = classify_line(&line).unwrap();
        assert_eq!(record.category, RecordCategory::Airport);
    }

    #[test]
    fn classifies_runway_records() {
        let line = line_with(&[(0, "SUSAP"), (6, "KSFO"), (10, "K2"), (12, "G")]);
        let record = classify_line(&line).unwrap();
        assert_eq!(record.category, RecordCategory::Runway);
    }

    #[test]
    fn classifies_sid_star_approach_records() {
        let sid = line_with(&[(0, "SUSAP"), (6, "KSFO"), (10, "K2"), (12, "D")]);
        let star = line_with(&[(0, "SUSAP"), (6, "KSFO"), (10, "K2"), (12, "E")]);
        let approach = line_with(&[(0, "SUSAP"), (6, "KSFO"), (10, "K2"), (12, "F")]);
        assert_eq!(
            classify_line(&sid).unwrap().category,
            RecordCategory::Procedure(ProcedureKind::Sid)
        );
        assert_eq!(
            classify_line(&star).unwrap().category,
            RecordCategory::Procedure(ProcedureKind::Star)
        );
        assert_eq!(
            classify_line(&approach).unwrap().category,
            RecordCategory::Procedure(ProcedureKind::Approach)
        );
    }

    #[test]
    fn classifies_enroute_waypoint_and_airway_records_via_the_column_6_subsection() {
        // Enroute records use the column-6 (index 5) subsection directly.
        // 'EA' is Waypoint and 'ER' is Airway -- verified against a real
        // CIFP file, the reverse of the two record types' letters.
        let waypoint = line_with(&[(0, "SUSA"), (4, "EA")]);
        assert_eq!(
            classify_line(&waypoint).unwrap().category,
            RecordCategory::Waypoint
        );
        let airway = line_with(&[(0, "SUSA"), (4, "ER")]);
        assert_eq!(
            classify_line(&airway).unwrap().category,
            RecordCategory::Airway
        );
    }

    #[test]
    fn classifies_terminal_waypoint_records_via_the_column_13_subsection() {
        // A real KSFO CIFP line for DOTNE, a terminal-area RNAV fix
        // referenced by the H10RZ approach — this exact record used to be
        // classified Unknown and dropped, which is why almost no
        // procedure leg's fix_ident resolved to a coordinate.
        let line = "SUSAP KSFOK2CDOTNE K20    W     N37430718W122365136                       E0129     NAR           DOTNE                    138992605";
        assert_eq!(
            classify_line(line).unwrap().category,
            RecordCategory::Waypoint
        );
    }
}
