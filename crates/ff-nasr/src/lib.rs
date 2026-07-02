//! Parser for the FAA NASR (National Airspace System Resources)
//! subscription: airports, runways, frequencies, remarks (DESIGN.md §3,
//! §5). Complements `ff-cifp`, which covers coded procedures/airways from
//! the same 28-day cycle.

pub mod convert;
pub mod parser;
pub mod records;

pub use convert::{airport_from_row, frequency_from_row, runway_from_rows, site_no_index};
pub use parser::{
    parse_apt_base, parse_apt_frequency, parse_apt_runway, parse_apt_runway_end, NasrError,
};
pub use records::{AptBaseRow, AptFrequencyRow, AptRunwayEndRow, AptRunwayRow};
