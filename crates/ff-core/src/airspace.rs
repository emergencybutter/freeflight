use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AirspaceClass {
    /// IFR only — VFR traffic may not enter. Absent from US airspace
    /// below FL180 so the FAA sources never produce it, but common
    /// outside the US: 107 UK volumes are Class A (the London TMA and
    /// most CTAs). Omitting it would hide precisely the airspace a VFR
    /// pilot most needs to be warned about, so it is modelled even though
    /// no US source emits it.
    A,
    B,
    C,
    D,
    E,
    /// Class F — advisory, not separated. Unused in the US and the UK,
    /// but 85 Canadian volumes are Class F.
    F,
    G,
    /// Special use airspace (MOA, restricted, prohibited, warning, alert).
    SpecialUse(SpecialUseKind),
}

/// Everything that is *not* an ICAO airspace class but still has to be
/// shown to a pilot.
///
/// The first five are the FAA's special-use categories, which is all the
/// US sources produce. The rest are European/ICAO constructs that appear
/// once non-US data is imported (DESIGN.md §3.1.2) — without them a third
/// of German and UK airspace has no faithful representation and gets
/// dropped, which is worse than showing it.
///
/// Note that RMZ/TMZ/ATZ are not "special use" in the narrow FAA sense —
/// an RMZ is an equipment requirement laid over otherwise ordinary
/// airspace. They live here because this is the "not an ICAO class"
/// bucket, and each serializes to its own name, so nothing is presented
/// to a pilot as something it isn't.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpecialUseKind {
    Moa,
    Restricted,
    Prohibited,
    Warning,
    Alert,
    /// Radio mandatory zone — entry requires two-way radio.
    Rmz,
    /// Transponder mandatory zone.
    Tmz,
    /// Aerodrome traffic zone.
    Atz,
    /// Glider/soaring sector (German `UGR`, UK national soaring areas).
    Glider,
    /// Parachute jumping area.
    Parachute,
    /// Military low flying area.
    LowFlying,
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
