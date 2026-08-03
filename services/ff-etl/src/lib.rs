//! Batch job that builds versioned data-cycle bundles from FAA/NOAA
//! sources (DESIGN.md §7).

pub mod aixm;
pub mod openaip;
pub mod airspace;
pub mod bundle;
pub mod chart_prep;
pub mod dtpp;
pub mod fetch;
pub mod pipeline;
pub mod preferred_routes;
pub mod publish;
pub mod validate;

pub use pipeline::{run, EtlError};
