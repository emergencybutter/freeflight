//! Batch job that builds versioned data-cycle bundles from FAA/NOAA
//! sources (DESIGN.md §7).

pub mod bundle;
pub mod fetch;
pub mod pipeline;
pub mod publish;
pub mod validate;

pub use pipeline::{run, EtlError};
