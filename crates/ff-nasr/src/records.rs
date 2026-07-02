//! Row shapes for the FAA NASR 28-day subscription CSV tables.
//!
//! The modern NASR subscription ships one CSV per table (`APT_BASE.csv`,
//! `APT_RWY.csv`, `APT_FREQ.csv`, ...). Column names below are a best
//! effort from the public NASR subscription layout and **must be checked
//! against the current `NASR_Subscription_...-README.txt` for the cycle
//! being ingested** before this is used for anything beyond scaffolding —
//! the FAA has changed column sets between cycles before.
use serde::Deserialize;

/// One row of `APT_BASE.csv`: core airport facility data.
#[derive(Debug, Clone, Deserialize)]
pub struct AptBaseRow {
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

/// One row of `APT_RWY.csv`: a single runway at an airport.
#[derive(Debug, Clone, Deserialize)]
pub struct AptRunwayRow {
    #[serde(rename = "ARPT_ID")]
    pub arpt_id: String,
    #[serde(rename = "RWY_ID")]
    pub rwy_id: String,
    #[serde(rename = "RWY_LEN")]
    pub rwy_len_ft: u32,
    #[serde(rename = "RWY_WIDTH")]
    pub rwy_width_ft: u32,
    #[serde(rename = "SURFACE_TYPE_CODE")]
    pub surface_type_code: String,
}

/// One row of `APT_FREQ.csv`: an airport communications frequency.
#[derive(Debug, Clone, Deserialize)]
pub struct AptFrequencyRow {
    #[serde(rename = "ARPT_ID")]
    pub arpt_id: String,
    #[serde(rename = "COMM_TYPE_CODE")]
    pub comm_type_code: String,
    #[serde(rename = "COMM_FREQ")]
    pub comm_freq_mhz: f64,
    #[serde(rename = "REMARK")]
    pub remark: Option<String>,
}
