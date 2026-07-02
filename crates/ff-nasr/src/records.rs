//! Row shapes for the FAA NASR 28-day subscription CSV tables.
//!
//! Column names below are verified against a real NASR CSV subscription
//! package (28-Day Subscription effective 11 Jun 2026, `APT_BASE.csv`,
//! `APT_RWY.csv`, `APT_RWY_END.csv`, `FRQ.csv`), not guessed — including
//! two structural facts that are easy to get wrong from the FAA's older
//! documentation:
//!
//! - `APT_RWY`/`APT_RWY_END` carry `ARPT_ID` directly (an earlier version
//!   of this module assumed they only had `SITE_NO` and needed a join
//!   through `APT_BASE`; that was wrong — both identifiers are present on
//!   every table here).
//! - There is no `APT_FREQ.csv`. Airport communication frequencies live
//!   in a top-level `FRQ.csv` shared by many facility types (towers,
//!   TRACONs, navaids, FSS, AWOS/ASOS, ...), keyed by `SERVICED_FACILITY`
//!   and `SERVICED_SITE_TYPE` rather than being airport-specific rows;
//!   see [`crate::convert::frequencies_for_airport`].
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
    /// Comma-separated, e.g. `"100LL,A,A++"` (`A`/`A++` are Jet-A
    /// variants); empty string when no fuel is available.
    #[serde(rename = "FUEL_TYPES")]
    pub fuel_types: String,
}

/// One row of `APT_RWY.csv`: a physical runway (both ends, e.g. `"13/31"`)
/// at an airport.
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

/// One row of `APT_RWY_END.csv`: one physical end of a runway (e.g. the
/// `"13"` end of runway `"13/31"`), with its own coordinates and true
/// alignment. Joins to [`AptRunwayRow`] on `(ARPT_ID, RWY_ID)`.
///
/// `lat_decimal`/`long_decimal` are `Option` because they're often
/// genuinely blank in real data — verified: 16,206 of 39,856 runway-end
/// rows (~41%) in the reference file have no coordinates at all, almost
/// entirely small/unsurveyed GA strips. This is common, not a rare edge
/// case, so `crate::convert::runway_from_rows` must tolerate it.
#[derive(Debug, Clone, Deserialize)]
pub struct AptRunwayEndRow {
    #[serde(rename = "ARPT_ID")]
    pub arpt_id: String,
    #[serde(rename = "RWY_ID")]
    pub rwy_id: String,
    #[serde(rename = "RWY_END_ID")]
    pub rwy_end_id: String,
    #[serde(rename = "TRUE_ALIGNMENT")]
    pub true_alignment: Option<f64>,
    #[serde(rename = "LAT_DECIMAL")]
    pub lat_decimal: Option<f64>,
    #[serde(rename = "LONG_DECIMAL")]
    pub long_decimal: Option<f64>,
}

/// One row of `FRQ.csv`: a single frequency belonging to some facility
/// (airport tower, TRACON, navaid, FSS, AWOS/ASOS, ...) that "services"
/// one or more other facilities. For airport comm frequencies, filter on
/// `serviced_site_type == "AIRPORT"` — see
/// [`crate::convert::frequencies_for_airport`].
///
/// `freq_use` is a free-text code, not a small enum: real values include
/// `"CTAF"`, `"UNICOM"`, `"LCL/P"` (tower, primary), `"GND/P"`,
/// `"CD/P"` (clearance delivery), `"ATIS"`, `"APCH/P"`, `"DEP/P"`, and
/// combined forms like `"APCH/P DEP/P"` — see
/// [`crate::convert::freq_use_kind`] for how these map onto
/// [`ff_core::FrequencyKind`].
#[derive(Debug, Clone, Deserialize)]
pub struct FrqRow {
    #[serde(rename = "FACILITY")]
    pub facility: String,
    #[serde(rename = "FACILITY_TYPE")]
    pub facility_type: String,
    #[serde(rename = "SERVICED_FACILITY")]
    pub serviced_facility: String,
    #[serde(rename = "SERVICED_SITE_TYPE")]
    pub serviced_site_type: String,
    /// Not always a plain number: navaid-serviced rows can carry a DME
    /// channel pair (`"116.65/113Y"`) or a receive-only suffix
    /// (`"122.1R"`); airport-serviced rows are effectively always plain
    /// (verified: 34 exceptions out of 31,495 in the reference file, all
    /// `"...R"` receive-only). Kept as a string and parsed leniently in
    /// `crate::convert`.
    #[serde(rename = "FREQ")]
    pub freq: String,
    #[serde(rename = "FREQ_USE")]
    pub freq_use: String,
    #[serde(rename = "REMARK")]
    pub remark: Option<String>,
}
