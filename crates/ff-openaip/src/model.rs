//! Wire types for openAIP's REST API, mirroring the JSON one-to-one.
//!
//! Deliberately a thin, faithful mirror: decoding the *meaning* of
//! openAIP's numeric enums lives in [`crate::convert`], where the
//! evidence for each mapping is recorded next to it. Keeping the two
//! apart means a schema change shows up as a deserialization failure
//! rather than a silently wrong conversion.
//!
//! Only the fields freeflight uses are modelled. openAIP returns a good
//! deal more (`hoursOfOperation`, `images`, `elevationGeoid`, audit
//! fields); serde ignores what isn't named here.

use serde::Deserialize;

/// Every list endpoint returns this envelope.
#[derive(Debug, Clone, Deserialize)]
pub struct Page<T> {
    pub limit: u32,
    #[serde(rename = "totalCount")]
    pub total_count: u32,
    #[serde(rename = "totalPages")]
    pub total_pages: u32,
    pub page: u32,
    pub items: Vec<T>,
}

/// A quantity plus the enum-coded unit it is expressed in. openAIP is
/// metric where freeflight is imperial, so nothing should read `value`
/// without also reading `unit` — see [`crate::convert::UnitCode`].
#[derive(Debug, Clone, Deserialize)]
pub struct Measure {
    /// Numeric for elevations/dimensions, but a *string* for frequencies
    /// (`"115.800"`), so it is kept as raw JSON and parsed per use.
    pub value: serde_json::Value,
    pub unit: Option<i32>,
    #[serde(rename = "referenceDatum")]
    pub reference_datum: Option<i32>,
}

impl Measure {
    /// The value as a number, whether the API sent it as one or as a
    /// string. Frequencies arrive quoted; elevations do not.
    pub fn as_f64(&self) -> Option<f64> {
        match &self.value {
            serde_json::Value::Number(n) => n.as_f64(),
            serde_json::Value::String(s) => s.trim().parse().ok(),
            _ => None,
        }
    }
}

/// GeoJSON geometry. Points for everything except airspace, which is a
/// polygon (first ring only — openAIP does not publish holes).
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum Geometry {
    Point {
        /// `[lon, lat]`, decimal degrees — note the GeoJSON order.
        coordinates: [f64; 2],
    },
    Polygon {
        coordinates: Vec<Vec<[f64; 2]>>,
    },
    #[serde(other)]
    Other,
}

impl Geometry {
    pub fn point(&self) -> Option<(f64, f64)> {
        match self {
            // Returned lat-first, matching ff-core.
            Geometry::Point { coordinates } => Some((coordinates[1], coordinates[0])),
            _ => None,
        }
    }

    /// The outer ring, as `(lat, lon)` pairs.
    pub fn outer_ring(&self) -> Option<Vec<(f64, f64)>> {
        match self {
            Geometry::Polygon { coordinates } => coordinates
                .first()
                .map(|ring| ring.iter().map(|c| (c[1], c[0])).collect()),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Airport {
    pub name: String,
    /// Absent for most small fields — of 1364 German entries only a
    /// minority carry one, so identity cannot depend on it.
    #[serde(rename = "icaoCode")]
    pub icao_code: Option<String>,
    #[serde(rename = "iataCode")]
    pub iata_code: Option<String>,
    pub country: String,
    #[serde(rename = "type")]
    pub kind: Option<i32>,
    pub geometry: Geometry,
    pub elevation: Option<Measure>,
    #[serde(default)]
    pub runways: Vec<Runway>,
    #[serde(default)]
    pub frequencies: Vec<Frequency>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Runway {
    /// One entry per runway *direction* ("18", "07L"), so a two-way
    /// runway appears twice — as in AIXM's `Rdn`.
    pub designator: String,
    #[serde(rename = "trueHeading")]
    pub true_heading: Option<f64>,
    pub dimension: Option<RunwayDimension>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RunwayDimension {
    pub length: Option<Measure>,
    pub width: Option<Measure>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Frequency {
    pub value: String,
    pub unit: Option<i32>,
    pub name: Option<String>,
    #[serde(default)]
    pub primary: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Navaid {
    pub name: String,
    pub identifier: Option<String>,
    #[serde(rename = "type")]
    pub kind: Option<i32>,
    pub country: String,
    pub frequency: Option<Measure>,
    pub elevation: Option<Measure>,
    pub geometry: Geometry,
    /// TACAN channel (`"105X"`). Its *presence* is what distinguishes a
    /// plain VOR from one with a co-located DME/TACAN — see
    /// [`crate::convert::navaid_type`].
    pub channel: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Airspace {
    #[serde(rename = "_id")]
    pub id: String,
    pub name: String,
    pub country: String,
    #[serde(rename = "type")]
    pub kind: Option<i32>,
    #[serde(rename = "icaoClass")]
    pub icao_class: Option<i32>,
    #[serde(rename = "upperLimit")]
    pub upper_limit: Option<Measure>,
    #[serde(rename = "lowerLimit")]
    pub lower_limit: Option<Measure>,
    pub geometry: Geometry,
}
