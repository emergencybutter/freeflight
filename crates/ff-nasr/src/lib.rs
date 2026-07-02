//! Parser for the FAA NASR (National Airspace System Resources)
//! subscription: airports, runways, frequencies, remarks (DESIGN.md §3,
//! §5). Complements `ff-cifp`, which covers coded procedures/airways from
//! the same 28-day cycle.

pub mod convert;
pub mod parser;
pub mod records;

pub use convert::{airport_from_row, freq_use_kind, frequencies_for_airport, runway_from_rows};
pub use parser::{parse_apt_base, parse_apt_runway, parse_apt_runway_end, parse_frq, NasrError};
pub use records::{AptBaseRow, AptRunwayEndRow, AptRunwayRow, FrqRow};
