//! Response shapes for `aviationweather.gov/api/data/{metar,taf}?format=json`.
//! Field names follow the published Data API JSON schema; kept as `Option`
//! wherever the API is known to omit a field for some reports.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CloudLayer {
    pub cover: String,
    pub base: Option<i32>,
}

/// One D-ATIS broadcast from datis.clowd.io (`/api/{icao}`) — not an
/// aviationweather.gov product. `kind` is `"combined"`, or `"dep"`/`"arr"`
/// at airports that split departure and arrival ATIS; `code` is the phonetic
/// info letter (e.g. `"Q"`); `datis` is the full broadcast text. Only the
/// ~100+ major US airports with Digital ATIS return anything.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Datis {
    pub airport: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub code: String,
    pub datis: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Metar {
    #[serde(rename = "icaoId")]
    pub icao_id: String,
    #[serde(rename = "obsTime")]
    pub obs_time: i64,
    #[serde(rename = "rawOb")]
    pub raw_text: String,
    pub temp: Option<f64>,
    pub dewp: Option<f64>,
    pub wdir: Option<serde_json::Value>,
    pub wspd: Option<i32>,
    pub wgst: Option<i32>,
    pub visib: Option<serde_json::Value>,
    pub altim: Option<f64>,
    #[serde(rename = "wxString")]
    pub wx_string: Option<String>,
    #[serde(default)]
    pub clouds: Vec<CloudLayer>,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    pub elev: Option<i32>,
    pub name: Option<String>,
    /// The API's own computed flight category (`"VFR"`, `"MVFR"`,
    /// `"IFR"`, `"LIFR"`) — confirmed live across VFR/MVFR samples.
    /// Kept `Option` defensively: haven't confirmed it's always present
    /// (e.g. if ceiling/visibility data is missing/malformed upstream).
    #[serde(rename = "fltCat")]
    pub flt_cat: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TafForecastPeriod {
    #[serde(rename = "timeFrom")]
    pub time_from: i64,
    #[serde(rename = "timeTo")]
    pub time_to: i64,
    #[serde(rename = "fcstChange")]
    pub fcst_change: Option<String>,
    /// Wind direction in degrees, or `"VRB"` for variable wind — same
    /// shape as `Metar::wdir` (confirmed against live KATL/KDEN/KMIA TAFs).
    pub wdir: Option<serde_json::Value>,
    pub wspd: Option<i32>,
    pub wgst: Option<i32>,
    pub visib: Option<serde_json::Value>,
    #[serde(rename = "wxString")]
    pub wx_string: Option<String>,
    #[serde(default)]
    pub clouds: Vec<CloudLayer>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Taf {
    #[serde(rename = "icaoId")]
    pub icao_id: String,
    #[serde(rename = "issueTime")]
    pub issue_time: String,
    #[serde(rename = "validTimeFrom")]
    pub valid_time_from: i64,
    #[serde(rename = "validTimeTo")]
    pub valid_time_to: i64,
    #[serde(rename = "rawTAF")]
    pub raw_text: String,
    #[serde(default)]
    pub fcsts: Vec<TafForecastPeriod>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializes_a_metar_response() {
        let json = r#"[{
            "icaoId": "KSFO",
            "obsTime": 1751500000,
            "rawOb": "KSFO 021956Z 28012KT 10SM FEW020 18/11 A3005",
            "temp": 18.0,
            "dewp": 11.0,
            "wdir": 280,
            "wspd": 12,
            "wgst": null,
            "visib": "10+",
            "altim": 1017.6,
            "wxString": null,
            "clouds": [{"cover": "FEW", "base": 2000}],
            "lat": 37.62,
            "lon": -122.37,
            "elev": 4,
            "name": "San Francisco Intl"
        }]"#;
        let metars: Vec<Metar> = serde_json::from_str(json).unwrap();
        assert_eq!(metars.len(), 1);
        assert_eq!(metars[0].icao_id, "KSFO");
        assert_eq!(metars[0].clouds[0].cover, "FEW");
    }
}
