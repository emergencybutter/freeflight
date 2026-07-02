//! Parser for the FAA CIFP (Coded Instrument Flight Procedures), which is
//! coded in ARINC 424 format (DESIGN.md §3, §5, §12).
//!
//! Two-stage design: [`record`]/[`parser`] classify raw fixed-width lines
//! into record categories (cheap, no field parsing, well covered by
//! tests); [`extract`] turns a classified record into `ff_core` types
//! (field-accurate, not yet implemented — see that module's docs).

pub mod extract;
pub mod parser;
pub mod record;

pub use parser::{classify_bytes, classify_file, CifpError};
pub use record::{classify_line, RawRecord, RecordCategory};
