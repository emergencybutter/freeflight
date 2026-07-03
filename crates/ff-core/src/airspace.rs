use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AirspaceClass {
    B,
    C,
    D,
    E,
    G,
    /// Special use airspace (MOA, restricted, prohibited, warning, alert).
    SpecialUse(SpecialUseKind),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpecialUseKind {
    Moa,
    Restricted,
    Prohibited,
    Warning,
    Alert,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AltitudeLimit {
    Msl(u32),
    Agl(u32),
    /// Flight level (hundreds of feet at standard pressure, e.g. `180` =
    /// FL180) — distinct from `Msl` because it's a different reference
    /// (29.92" Hg, not actual sea level) and FAA's special-use-airspace
    /// data reports some limits this way (`UPPER_CODE`/`LOWER_CODE` =
    /// `"STD"`) rather than as MSL feet.
    FlightLevel(u32),
    Surface,
    Unlimited,
}

/// A single closed polygon boundary, lat/lon pairs, first point implicitly
/// closes with the last. Stored as plain coordinates rather than a
/// GeoJSON dependency so `ff-core` stays dependency-light; `ff-charts`/the
/// clients convert to GeoJSON for MapLibre when rendering.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Polygon {
    pub points: Vec<(f64, f64)>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AirspaceVolume {
    pub id: String,
    pub name: String,
    pub class: AirspaceClass,
    pub floor: AltitudeLimit,
    pub ceiling: AltitudeLimit,
    pub boundary: Polygon,
}
