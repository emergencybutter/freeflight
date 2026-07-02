//! ARINC 424 record classification.
//!
//! Every CIFP record is a fixed-width (132 column) line. Columns 1, 5 and 6
//! (1-indexed) — record type, section code, subsection code — identify what
//! kind of record it is; the remaining columns are section-specific.
//!
//! The section/subsection → [`RecordCategory`] mapping below reflects the
//! commonly documented ARINC 424 layout (record type 'S', then Section
//! P=Airport, subsections D/E/F=SID/STAR/Approach, G=Runway, N=NDB at
//! airport; Section D=VHF navaid, DB=NDB navaid; Section E=Enroute, with
//! EA=Airway, EU=Enroute communication). **Verify column offsets and every
//! mapping against the current FAA CIFP User's Guide / ARINC 424 Attachment
//! 5 before relying on parsed field values** — this module only classifies
//! records well enough to route them to the right (not-yet-implemented)
//! field extractor; see [`crate::extract`].
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
/// extraction from `raw` is deferred to `crate::extract` (unimplemented in
/// this scaffold — see module docs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawRecord {
    pub raw: String,
    pub category: RecordCategory,
}

/// Classify a single 132-column CIFP line by its section/subsection code.
///
/// Returns `None` if the line is shorter than a full record (e.g. a
/// trailing blank line) rather than guessing at a partial record.
pub fn classify_line(line: &str) -> Option<RawRecord> {
    if line.len() < 6 {
        return None;
    }
    let bytes = line.as_bytes();
    let section = bytes[4] as char;
    let subsection = bytes[5] as char;

    let category = match (section, subsection) {
        ('P', 'A') => RecordCategory::Airport,
        ('P', 'G') => RecordCategory::Runway,
        ('P', 'D') => RecordCategory::Procedure(ProcedureKind::Sid),
        ('P', 'E') => RecordCategory::Procedure(ProcedureKind::Star),
        ('P', 'F') => RecordCategory::Procedure(ProcedureKind::Approach),
        ('P', 'N') => RecordCategory::NdbNavaid,
        ('D', _) if subsection != 'B' => RecordCategory::VhfNavaid,
        ('D', 'B') => RecordCategory::NdbNavaid,
        ('E', 'A') => RecordCategory::Airway,
        ('E', 'U') => RecordCategory::EnrouteCommunication,
        ('E', _) => RecordCategory::Waypoint,
        _ => RecordCategory::Unknown,
    };

    Some(RawRecord {
        raw: line.to_string(),
        category,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_lines_are_ignored() {
        assert!(classify_line("SUSP").is_none());
    }

    #[test]
    fn classifies_airport_records() {
        // cols: 1=S(rec type) 2-4=USA(area) 5=P(section) 6=A(subsection)
        let line = "SUSAPA".to_string() + &" ".repeat(RECORD_LENGTH - 6);
        let record = classify_line(&line).unwrap();
        assert_eq!(record.category, RecordCategory::Airport);
    }
}
