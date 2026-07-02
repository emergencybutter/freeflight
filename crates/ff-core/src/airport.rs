use serde::{Deserialize, Serialize};

/// An airport/heliport as published in the FAA NASR facility directory.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Airport {
    /// ICAO identifier, e.g. "KSFO". Primary key.
    pub icao: String,
    /// FAA-local identifier, e.g. "SFO" (may differ from IATA).
    pub faa_id: Option<String>,
    pub iata: Option<String>,
    pub name: String,
    pub lat: f64,
    pub lon: f64,
    pub elevation_ft: i32,
    pub airport_type: AirportType,
    /// Fuel types available, e.g. ["100LL", "JET-A"].
    pub fuel_types: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AirportType {
    Airport,
    Heliport,
    Seaplane,
    Ultralight,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Runway {
    pub airport_icao: String,
    /// e.g. "10/28".
    pub ident: String,
    pub length_ft: u32,
    pub width_ft: u32,
    pub surface: RunwaySurface,
    pub low_end: RunwayEnd,
    pub high_end: RunwayEnd,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunwayEnd {
    /// e.g. "10L".
    pub ident: String,
    pub lat: f64,
    pub lon: f64,
    /// True heading in degrees.
    pub heading_deg: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RunwaySurface {
    Asphalt,
    Concrete,
    Turf,
    Gravel,
    Water,
    Other,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Frequency {
    pub airport_icao: String,
    pub kind: FrequencyKind,
    pub freq_mhz: f64,
    pub remarks: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FrequencyKind {
    Ctaf,
    Unicom,
    Tower,
    Ground,
    Approach,
    Departure,
    Atis,
    Awos,
    Clearance,
    Other,
}
