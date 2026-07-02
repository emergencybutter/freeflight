//! Shared domain types for freeflight (DESIGN.md §6).
//!
//! Pure data types with no I/O — parsers (`ff-cifp`, `ff-nasr`), storage
//! (`ff-storage`), and every client binding build on these so there is one
//! definition of "what an Airport/Procedure/AirspaceVolume is" across the
//! whole workspace.

pub mod airport;
pub mod airspace;
pub mod airway;
pub mod cycle;
pub mod navaid;
pub mod procedure;

pub use airport::{
    Airport, AirportType, Frequency, FrequencyKind, Runway, RunwayEnd, RunwaySurface,
};
pub use airspace::{AirspaceClass, AirspaceVolume, AltitudeLimit, Polygon, SpecialUseKind};
pub use airway::{Airway, AirwayKind, AirwayLeg};
pub use cycle::AiracCycle;
pub use navaid::{Navaid, NavaidType, Waypoint};
pub use procedure::{
    AltitudeConstraint, PathAndTerm, Procedure, ProcedureKind, ProcedureLeg, ProcedureTransition,
    SpeedConstraint, TransitionKind, TurnDirection,
};
