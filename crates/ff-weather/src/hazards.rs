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
use serde::{Deserialize, Deserializer, Serialize};

/// `#[serde(default)]` alone only kicks in when a key is *absent* — real
/// `/pirep` data has `"clouds": null` too (confirmed live, not just an
/// absent key or a real array), which `#[serde(default)]` can't turn
/// into an empty `Vec` on its own since `null` isn't a valid `Vec`.
fn null_to_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::deserialize(deserializer)?.unwrap_or_default())
}

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

/// A Center Weather Advisory record (`/cwa`) — short-fuse, ARTCC-issued
/// tactical hazard advisories (thunderstorms, turbulence, IFR
/// conditions), similar in spirit to `Sigmet` but more localized and
/// typically faster-issued. `coords` uses the same string-lat/lon shape
/// as `GAirmetCoord` (confirmed live), not `SigmetCoord`'s numeric one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cwa {
    pub cwsu: String,
    pub name: String,
    #[serde(rename = "receiptTime")]
    pub receipt_time: String,
    #[serde(rename = "validTimeFrom")]
    pub valid_time_from: i64,
    #[serde(rename = "validTimeTo")]
    pub valid_time_to: i64,
    #[serde(rename = "seriesId")]
    pub series_id: String,
    pub hazard: String,
    pub qualifier: Option<String>,
    pub base: Option<i32>,
    pub top: Option<i32>,
    pub geom: String,
    pub coords: Vec<GAirmetCoord>,
    #[serde(rename = "rawText")]
    pub raw_text: String,
}

/// A pilot report (`/pirep`) — real-time, point-in-space conditions
/// reported by another aircraft (icing, turbulence, clouds, remarks),
/// unlike every other type in this module which is a forecaster-issued
/// area/line hazard. Requires a bounding box or station+radial when
/// fetching (confirmed live: the bare endpoint errors without one),
/// unlike `GAirmet`/`Sigmet`/`IntlSigmet`/`Cwa` which return all current
/// records with no params.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pirep {
    #[serde(rename = "receiptTime")]
    pub receipt_time: String,
    #[serde(rename = "obsTime")]
    pub obs_time: i64,
    #[serde(rename = "icaoId")]
    pub icao_id: Option<String>,
    #[serde(rename = "acType")]
    pub ac_type: Option<String>,
    pub lat: f64,
    pub lon: f64,
    #[serde(rename = "fltLvl")]
    pub flt_lvl: Option<i32>,
    #[serde(default, deserialize_with = "null_to_default")]
    pub clouds: Vec<PirepCloud>,
    pub visib: Option<serde_json::Value>,
    #[serde(rename = "wxString")]
    pub wx_string: Option<String>,
    pub temp: Option<f64>,
    /// First reported icing layer — intensity/type are free-text codes
    /// (e.g. `"LGT-MOD"`, `"RIME"`), empty string rather than absent
    /// when not reported (confirmed live).
    #[serde(rename = "icgBas1")]
    pub icg_base1: Option<i32>,
    #[serde(rename = "icgTop1")]
    pub icg_top1: Option<i32>,
    #[serde(rename = "icgInt1")]
    pub icg_intensity1: Option<String>,
    #[serde(rename = "icgType1")]
    pub icg_type1: Option<String>,
    /// Second icing layer, if a distinct one was reported.
    #[serde(rename = "icgBas2")]
    pub icg_base2: Option<i32>,
    #[serde(rename = "icgTop2")]
    pub icg_top2: Option<i32>,
    #[serde(rename = "icgInt2")]
    pub icg_intensity2: Option<String>,
    #[serde(rename = "icgType2")]
    pub icg_type2: Option<String>,
    #[serde(rename = "tbBas1")]
    pub tb_base1: Option<i32>,
    #[serde(rename = "tbTop1")]
    pub tb_top1: Option<i32>,
    #[serde(rename = "tbInt1")]
    pub tb_intensity1: Option<String>,
    #[serde(rename = "tbType1")]
    pub tb_type1: Option<String>,
    #[serde(rename = "tbFreq1")]
    pub tb_frequency1: Option<String>,
    #[serde(rename = "tbBas2")]
    pub tb_base2: Option<i32>,
    #[serde(rename = "tbTop2")]
    pub tb_top2: Option<i32>,
    #[serde(rename = "tbInt2")]
    pub tb_intensity2: Option<String>,
    #[serde(rename = "tbType2")]
    pub tb_type2: Option<String>,
    #[serde(rename = "tbFreq2")]
    pub tb_frequency2: Option<String>,
    /// `"PIREP"`, `"AIREP"`, or `"Urgent PIREP"` (confirmed live, all
    /// three) — an Urgent PIREP is a pilot report of a hazard severe
    /// enough that ATC/other pilots need to know immediately (severe
    /// icing/turbulence, etc.), worth flagging distinctly from routine
    /// ones rather than just another data point.
    #[serde(rename = "pirepType")]
    pub pirep_type: String,
    #[serde(rename = "rawOb")]
    pub raw_ob: String,
}

/// Cloud layer shape as reported in a `Pirep` — unlike `records::CloudLayer`
/// (METAR/TAF), this includes a `top` (confirmed live), so it isn't the
/// same struct despite the field-name overlap.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PirepCloud {
    pub cover: String,
    pub base: Option<i32>,
    pub top: Option<i32>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializes_a_real_cwa_response() {
        let json = r#"[{
            "cwsu": "ZMA",
            "name": "Miami",
            "receiptTime": "2026-07-05T17:28:15.556Z",
            "validTimeFrom": 1783272420,
            "validTimeTo": 1783276200,
            "seriesId": "201",
            "hazard": "TS",
            "qualifier": "",
            "base": null,
            "top": 33000,
            "geom": "AREA",
            "coords": [
                {"lat": "25.792", "lon": "-76.601"},
                {"lat": "23.264", "lon": "-74.741"},
                {"lat": "25.792", "lon": "-76.601"}
            ],
            "rawText": "FAUS22 KZMA 051728\nZMA2 CWA 051727 \nZMA CWA 201 VALID UNTIL 051830\n"
        }]"#;
        let cwas: Vec<Cwa> = serde_json::from_str(json).unwrap();
        assert_eq!(cwas.len(), 1);
        assert_eq!(cwas[0].cwsu, "ZMA");
        assert_eq!(cwas[0].top, Some(33000));
        assert_eq!(cwas[0].coords[0].lat, "25.792");
    }

    #[test]
    fn deserializes_a_real_pirep_response_with_icing_and_turbulence() {
        let json = r#"[{
            "receiptTime": "2026-07-05T18:05:57.680Z",
            "obsTime": 1783274460,
            "qcField": 0,
            "icaoId": "KMSC",
            "acType": "E170",
            "lat": 41.9438,
            "lon": -88.6899,
            "fltLvl": 85,
            "fltLvlType": "OTHER",
            "clouds": [{"cover": "OVC", "base": 1900, "top": 2600}],
            "visib": null,
            "wxString": "",
            "temp": null,
            "wdir": null,
            "wspd": null,
            "icgBas1": null,
            "icgTop1": null,
            "icgInt1": "",
            "icgType1": "",
            "icgBas2": null,
            "icgTop2": null,
            "icgInt2": "",
            "icgType2": "",
            "tbBas1": null,
            "tbTop1": null,
            "tbInt1": "NEG",
            "tbType1": "",
            "tbFreq1": "",
            "tbBas2": null,
            "tbTop2": null,
            "tbInt2": "",
            "tbType2": "",
            "tbFreq2": "",
            "vertGust": null,
            "brkAction": "",
            "pirepType": "PIREP",
            "rawOb": "ORD UA /OV 35 W ORD/TM 1801/FL085/TP E170/TB SMOOTH "
        }]"#;
        let pireps: Vec<Pirep> = serde_json::from_str(json).unwrap();
        assert_eq!(pireps.len(), 1);
        assert_eq!(pireps[0].pirep_type, "PIREP");
        assert_eq!(pireps[0].tb_intensity1.as_deref(), Some("NEG"));
        assert_eq!(pireps[0].clouds[0].top, Some(2600));
    }

    #[test]
    fn deserializes_a_real_pirep_with_null_clouds() {
        // Confirmed live: "clouds": null (not just an absent key or a
        // real array) shows up in practice — this is what broke before
        // null_to_default was added.
        let json = r#"[{
            "receiptTime": "2026-07-05T18:05:04.841Z",
            "obsTime": 1783274340,
            "icaoId": "KWBC",
            "acType": "B39M",
            "lat": 39.8669,
            "lon": -118.0202,
            "fltLvl": 350,
            "clouds": null,
            "visib": null,
            "wxString": "",
            "temp": null,
            "tbInt1": "LGT-MOD",
            "tbType1": "CHOP",
            "pirepType": "PIREP",
            "rawOb": "WBC UA /OV WBC/TM 1805/FL350/TP B39M/TB LGT-MOD CHOP"
        }]"#;
        let pireps: Vec<Pirep> = serde_json::from_str(json).unwrap();
        assert!(pireps[0].clouds.is_empty());
    }

    #[test]
    fn deserializes_an_urgent_pirep() {
        let json = r#"[{
            "receiptTime": "2026-07-05T18:05:57.680Z",
            "obsTime": 1783274460,
            "icaoId": null,
            "acType": "C182",
            "lat": 40.0,
            "lon": -105.0,
            "fltLvl": 120,
            "clouds": [],
            "visib": null,
            "wxString": null,
            "temp": null,
            "icgBas1": 8000,
            "icgTop1": 12000,
            "icgInt1": "SEV",
            "icgType1": "RIME",
            "tbBas1": null,
            "tbTop1": null,
            "tbInt1": "",
            "pirepType": "Urgent PIREP",
            "rawOb": "UA /OV DEN270020/TM 1805/FL120/TP C182/IC SEV RIME 080-120"
        }]"#;
        let pireps: Vec<Pirep> = serde_json::from_str(json).unwrap();
        assert_eq!(pireps[0].pirep_type, "Urgent PIREP");
        assert_eq!(pireps[0].icg_intensity1.as_deref(), Some("SEV"));
    }
}
