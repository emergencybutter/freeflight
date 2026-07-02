//! Row shapes for the FAA NASR 28-day subscription CSV tables.
//!
//! The modern NASR subscription ships one CSV per table (`APT_BASE.csv`,
//! `APT_RWY.csv`, `APT_RWY_END.csv`, ...). Column names below for
//! `APT_BASE`/`APT_RWY`/`APT_RWY_END` were cross-checked against real
//! queries from an open-source project that works with live FAA NASR
//! extracts (jlmcgraw/processFaaData, `Sample SQL queries.sql`) rather
//! than guessed — including a structural fact easy to get wrong: the
//! "detail" tables (`APT_RWY`, `APT_RWY_END`, and by the same convention
//! frequency/comm tables) key on `SITE_NO`, *not* `ARPT_ID` — only
//! `APT_BASE` carries both, so joining runway/frequency rows back to an
//! airport identifier requires a `SITE_NO` -> `ARPT_ID` lookup built from
//! `APT_BASE` first (see `crate::convert`).
//!
//! `APT_FREQ`'s exact column set could not be verified the same way and
//! is still a best-effort guess — **check it against the current NASR
//! subscription README before relying on it.**
use serde::Deserialize;

/// One row of `APT_BASE.csv`: core airport facility data.
#[derive(Debug, Clone, Deserialize)]
pub struct AptBaseRow {
    #[serde(rename = "SITE_NO")]
    pub site_no: String,
    #[serde(rename = "ARPT_ID")]
    pub arpt_id: String,
    #[serde(rename = "ICAO_ID")]
    pub icao_id: Option<String>,
    #[serde(rename = "ARPT_NAME")]
    pub arpt_name: String,
    #[serde(rename = "LAT_DECIMAL")]
    pub lat_decimal: f64,
    #[serde(rename = "LONG_DECIMAL")]
    pub long_decimal: f64,
    #[serde(rename = "ELEV")]
    pub elevation_ft: f64,
    #[serde(rename = "SITE_TYPE_CODE")]
    pub site_type_code: String,
}

/// One row of `APT_RWY.csv`: a physical runway (both ends, e.g. `"01/19"`)
/// at an airport, keyed by `SITE_NO` (see module docs).
#[derive(Debug, Clone, Deserialize)]
pub struct AptRunwayRow {
    #[serde(rename = "SITE_NO")]
    pub site_no: String,
    #[serde(rename = "RWY_ID")]
    pub rwy_id: String,
    #[serde(rename = "RWY_LEN")]
    pub rwy_len_ft: u32,
    #[serde(rename = "RWY_WIDTH")]
    pub rwy_width_ft: u32,
    #[serde(rename = "SURFACE_TYPE_CODE")]
    pub surface_type_code: String,
}

/// One row of `APT_RWY_END.csv`: one physical end of a runway (e.g. the
/// `"01"` end of runway `"01/19"`), with its own coordinates and true
/// alignment. Joins to [`AptRunwayRow`] on `(SITE_NO, RWY_ID)`, and its
/// `rwy_end_id` (e.g. `"01"`) is the half of `RWY_ID` (e.g. `"01/19"`)
/// this record describes.
#[derive(Debug, Clone, Deserialize)]
pub struct AptRunwayEndRow {
    #[serde(rename = "SITE_NO")]
    pub site_no: String,
    #[serde(rename = "RWY_ID")]
    pub rwy_id: String,
    #[serde(rename = "RWY_END_ID")]
    pub rwy_end_id: String,
    #[serde(rename = "TRUE_ALIGNMENT")]
    pub true_alignment: Option<f64>,
    #[serde(rename = "LAT_DECIMAL")]
    pub lat_decimal: f64,
    #[serde(rename = "LONG_DECIMAL")]
    pub long_decimal: f64,
}

/// One row of `APT_FREQ.csv`: an airport communications frequency.
///
/// Unlike the other rows in this module, this column set is **not**
/// verified against a real NASR extract (see module docs) — treat it as
/// a starting point, not ground truth.
#[derive(Debug, Clone, Deserialize)]
pub struct AptFrequencyRow {
    #[serde(rename = "SITE_NO")]
    pub site_no: String,
    #[serde(rename = "COMM_TYPE_CODE")]
    pub comm_type_code: String,
    #[serde(rename = "COMM_FREQ")]
    pub comm_freq_mhz: f64,
    #[serde(rename = "REMARK")]
    pub remark: Option<String>,
}
