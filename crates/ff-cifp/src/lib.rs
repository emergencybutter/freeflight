//! Parser for the FAA CIFP (Coded Instrument Flight Procedures), which is
//! coded in ARINC 424 format (DESIGN.md §3, §5, §12).
//!
//! Two-stage design: [`record`]/[`parser`] classify raw fixed-width lines
//! into record categories; [`decode`]/[`extract`] turn a classified
//! record into `ff_core` types: Airport, Runway (paired from CIFP's
//! per-end records), VHF/NDB Navaid, Waypoint, and SID/STAR/Approach legs
//! (grouped into Procedure/ProcedureTransition/ProcedureLeg by
//! [`extract::build_procedures`]). Airway and enroute-communication
//! records are classified by [`record`] but not yet extracted.

pub mod decode;
pub mod extract;
pub mod parser;
pub mod record;

pub use extract::{
    build_procedures, extract_airport, extract_ndb_navaid, extract_procedure_leg_row,
    extract_runway_end, extract_vhf_navaid, extract_waypoint, pair_runway_ends, CifpRunwayEnd,
    ParsedProcedures, ProcedureLegRow,
};
pub use parser::{classify_bytes, classify_file, CifpError};
pub use record::{classify_line, RawRecord, RecordCategory};

#[cfg(test)]
pub(crate) mod test_util {
    use crate::record::RECORD_LENGTH;

    /// Builds a `RECORD_LENGTH`-column test line with each `value` placed
    /// starting at its `start` column, space-padded elsewhere. Column
    /// numbers match the 0-indexed ranges documented on the `extract_*`
    /// functions (cross-checked against a real ARINC 424 parser).
    pub fn line_with(fields: &[(usize, &str)]) -> String {
        let mut chars = vec![b' '; RECORD_LENGTH];
        for (start, value) in fields {
            for (i, b) in value.bytes().enumerate() {
                chars[start + i] = b;
            }
        }
        String::from_utf8(chars).unwrap()
    }
}
