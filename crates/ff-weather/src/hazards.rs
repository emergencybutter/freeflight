//! Response shapes for aviationweather.gov's graphical AIRMET (`/gairmet`),
//! domestic SIGMET (`/sigmet`), and international SIGMET (`/isigmet`)
//! endpoints. All three were captured live and diverge from each other
//! more than METAR/TAF do (see each type's doc comment for specifics
//! confirmed on real data), so don't assume field shapes generalize
//! across them.
//!
//! The older plain-text `/airmet` endpoint (per-region hazard codes, no
//! polygon coordinates) isn't modeled here — `GAirmet` supersedes it for
//! any use that needs to draw a shape on a map, which is the whole point
//! for this project (DESIGN.md §9.2).
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GAirmetCoord {
    // Confirmed on live data: numeric-looking but sent as JSON strings
    // (e.g. `"35.51"`), unlike every other coordinate pair in this
    // module — parse before doing math with these.
    pub lat: String,
    pub lon: String,
}

/// A Graphical AIRMET record (`/gairmet`). Both `"AREA"` and `"LINE"`
/// `geometry_type`s use this same flat `coords` shape (confirmed on live
/// data for both) — unlike `IntlSigmet`, there's no nested/multi-polygon
/// variant to handle here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GAirmet {
    pub tag: String,
    #[serde(rename = "forecastHour")]
    pub forecast_hour: i32,
    #[serde(rename = "validTime")]
    pub valid_time: String,
    pub hazard: String,
    #[serde(rename = "geometryType")]
    pub geometry_type: String,
    pub latlonpairs: i32,
    pub frequency: Option<String>,
    pub severity: Option<String>,
    pub due_to: Option<String>,
    pub status: String,
    pub top: Option<String>,
    pub base: Option<String>,
    pub fzltop: Option<String>,
    pub fzlbase: Option<String>,
    pub level: Option<String>,
    #[serde(rename = "receiptTime")]
    pub receipt_time: i64,
    #[serde(rename = "issueTime")]
    pub issue_time: i64,
    #[serde(rename = "expireTime")]
    pub expire_time: i64,
    /// The forecast center issuing this, e.g. `"SIERRA"`, `"ZULU"`,
    /// `"TANGO"` — not an airport/region code.
    pub product: String,
    pub geom: String,
    pub coords: Vec<GAirmetCoord>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SigmetCoord {
    pub lat: f64,
    pub lon: f64,
}

/// A US domestic SIGMET record (`/sigmet`) — includes convective SIGMETs
/// (`hazard: "CONVECTIVE"`). No `geom` field and no nested-polygon case
/// here (unlike `IntlSigmet`); `coords` is always a flat point list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sigmet {
    #[serde(rename = "icaoId")]
    pub icao_id: String,
    #[serde(rename = "alphaChar")]
    pub alpha_char: String,
    #[serde(rename = "seriesId")]
    pub series_id: String,
    #[serde(rename = "receiptTime")]
    pub receipt_time: String,
    #[serde(rename = "creationTime")]
    pub creation_time: String,
    #[serde(rename = "validTimeFrom")]
    pub valid_time_from: i64,
    #[serde(rename = "validTimeTo")]
    pub valid_time_to: i64,
    #[serde(rename = "airSigmetType")]
    pub air_sigmet_type: String,
    pub hazard: String,
    #[serde(rename = "altitudeHi1")]
    pub altitude_hi1: Option<i32>,
    #[serde(rename = "altitudeHi2")]
    pub altitude_hi2: Option<i32>,
    #[serde(rename = "altitudeLow1")]
    pub altitude_low1: Option<i32>,
    #[serde(rename = "altitudeLow2")]
    pub altitude_low2: Option<i32>,
    #[serde(rename = "movementDir")]
    pub movement_dir: Option<i32>,
    #[serde(rename = "movementSpd")]
    pub movement_spd: Option<i32>,
    #[serde(rename = "rawAirSigmet")]
    pub raw_air_sigmet: String,
    #[serde(rename = "postProcessFlag")]
    pub post_process_flag: i32,
    pub severity: i32,
    pub coords: Vec<SigmetCoord>,
}

/// An international/oceanic SIGMET record (`/isigmet`) — FIRs outside
/// the US (e.g. Johannesburg, Casablanca, oceanic control areas).
/// DESIGN.md scopes this project US-only; included anyway since the
/// shape is basically free once `Sigmet` above is modeled.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IntlSigmet {
    #[serde(rename = "icaoId")]
    pub icao_id: String,
    #[serde(rename = "firId")]
    pub fir_id: String,
    #[serde(rename = "firName")]
    pub fir_name: String,
    #[serde(rename = "receiptTime")]
    pub receipt_time: String,
    #[serde(rename = "validTimeFrom")]
    pub valid_time_from: i64,
    #[serde(rename = "validTimeTo")]
    pub valid_time_to: i64,
    #[serde(rename = "seriesId")]
    pub series_id: String,
    /// Confirmed nullable on live data (unlike `Sigmet::hazard`, which
    /// so far has always been present).
    pub hazard: Option<String>,
    pub qualifier: Option<String>,
    pub base: Option<i32>,
    pub top: Option<i32>,
    /// `"AREA"` (single polygon) or `"AREAS"` (multiple) — see `coords`.
    pub geom: String,
    /// Shape depends on `geom`: `"AREA"` → a flat array of `{lat, lon}`
    /// points; `"AREAS"` → an array of arrays of `{lat, lon}` points
    /// (confirmed on live data: 2 of 143 sampled records were
    /// `"AREAS"`). Individual points can have a null `lon` with `lat`
    /// still present (also seen live) — kept as raw JSON rather than a
    /// fixed struct that would fail to deserialize whichever shape it
    /// doesn't expect.
    pub coords: serde_json::Value,
    /// Movement direction — a numeric string, the literal placeholder
    /// `"-"` (stationary/not given), or absent. Never a JSON number on
    /// live data, so this is `String` not `i32`.
    pub dir: Option<String>,
    /// Movement speed — same caveat as `dir` (seen as `"0"`, a string).
    pub spd: Option<String>,
    /// Forecast change, e.g. `"NC"` (no change), `"INTSF"`, `"WKN"`.
    pub chng: Option<String>,
    #[serde(rename = "rawSigmet")]
    pub raw_sigmet: String,
}
