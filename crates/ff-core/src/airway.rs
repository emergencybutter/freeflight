use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AirwayKind {
    /// Low-altitude victor airway.
    Victor,
    /// High-altitude jet route.
    Jet,
    /// RNAV T-route.
    RnavLow,
    /// RNAV Q-route.
    RnavHigh,
    /// Anything else — real CIFP data includes Alaska/oceanic
    /// colored-airway idents (A342, B233...) that don't fit the CONUS
    /// V/J/T/Q naming convention.
    Other,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Airway {
    /// Airway identifier, e.g. "V27", "J501".
    pub ident: String,
    pub kind: AirwayKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AirwayLeg {
    pub airway_ident: String,
    /// 1-based position along the airway.
    pub seq: u32,
    /// Identifier of the fix/navaid/waypoint at this point on the airway.
    pub fix_ident: String,
    pub min_altitude_ft: Option<u32>,
    pub max_altitude_ft: Option<u32>,
}
