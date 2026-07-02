//! Parser for the FAA NASR (National Airspace System Resources)
//! subscription: airports, runways, frequencies, remarks (DESIGN.md §3,
//! §5). Complements `ff-cifp`, which covers coded procedures/airways from
//! the same 28-day cycle.

pub mod convert;
pub mod parser;
pub mod records;

pub use parser::{parse_apt_base, parse_apt_frequency, parse_apt_runway, NasrError};
