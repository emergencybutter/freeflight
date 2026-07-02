use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NavaidType {
    Vor,
    VorDme,
    Vortac,
    Ndb,
    Dme,
    Tacan,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Navaid {
    /// Navaid identifier, e.g. "SFO". Not globally unique on its own —
    /// pair with `region` when disambiguating.
    pub ident: String,
    pub navaid_type: NavaidType,
    pub lat: f64,
    pub lon: f64,
    pub elevation_ft: Option<i32>,
    pub freq_khz: Option<u32>,
    pub region: String,
}

/// A named enroute/terminal fix that is not a navaid transmitter
/// (a plain lat/lon waypoint from the CIFP).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Waypoint {
    pub ident: String,
    pub lat: f64,
    pub lon: f64,
    pub region: String,
}
