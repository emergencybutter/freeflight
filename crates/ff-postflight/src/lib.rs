//! GPS track ingestion and phase-of-flight/landing analysis for
//! post-flight review (DESIGN.md §5, §9.4).

pub mod phases;
pub mod track;

pub use phases::{
    segment_phases, summarize, FlightSummary, LandingKind, PhaseKind, PhaseSegment,
    AIRBORNE_SPEED_THRESHOLD_KT, TAXI_SPEED_THRESHOLD_KT, TOUCH_AND_GO_MAX_GROUND_SECONDS,
};
pub use track::{derive_ground_speed, FlightTrack, TrackPoint, TrackPointWithSpeed};
