//! Per-category field extraction: [`crate::record::RawRecord`] → `ff_core`
//! domain types.
//!
//! Not implemented yet. Each of these needs the exact ARINC 424 column
//! layout for its record category (Attachment 5 of the spec / the FAA CIFP
//! User's Guide) — deliberately left as `todo!()` rather than guessed at,
//! since wrong field offsets here would silently corrupt procedure data.
use crate::parser::CifpError;
use crate::record::RawRecord;
use ff_core::{Airport, ProcedureLeg, Runway};

pub fn extract_airport(_record: &RawRecord) -> Result<Airport, CifpError> {
    todo!("ARINC 424 Section P/A field layout")
}

pub fn extract_runway(_record: &RawRecord) -> Result<Runway, CifpError> {
    todo!("ARINC 424 Section P/G field layout")
}

pub fn extract_procedure_leg(_record: &RawRecord) -> Result<ProcedureLeg, CifpError> {
    todo!("ARINC 424 Section P/D,E,F leg field layout (path & terminator, cols per Attachment 5)")
}
