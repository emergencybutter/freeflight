use crate::records::{AptBaseRow, AptFrequencyRow, AptRunwayRow};
use ff_core::{Airport, AirportType, Frequency, FrequencyKind, Runway, RunwayEnd, RunwaySurface};

/// Best-effort `SITE_TYPE_CODE` mapping; unrecognized codes fall back to
/// `AirportType::Airport` rather than erroring, since this is a coarse
/// display classification, not something safety-relevant.
fn site_type(code: &str) -> AirportType {
    match code {
        "A" => AirportType::Airport,
        "H" => AirportType::Heliport,
        "S" => AirportType::Seaplane,
        "U" => AirportType::Ultralight,
        _ => AirportType::Airport,
    }
}

fn surface_type(code: &str) -> RunwaySurface {
    match code {
        c if c.starts_with('A') => RunwaySurface::Asphalt,
        c if c.starts_with('C') => RunwaySurface::Concrete,
        c if c.starts_with('T') => RunwaySurface::Turf,
        c if c.starts_with('G') => RunwaySurface::Gravel,
        c if c.starts_with('W') => RunwaySurface::Water,
        _ => RunwaySurface::Other,
    }
}

fn comm_type(code: &str) -> FrequencyKind {
    match code {
        "CTAF" => FrequencyKind::Ctaf,
        "UNICOM" => FrequencyKind::Unicom,
        "TWR" => FrequencyKind::Tower,
        "GND" => FrequencyKind::Ground,
        "APP" => FrequencyKind::Approach,
        "DEP" => FrequencyKind::Departure,
        "ATIS" => FrequencyKind::Atis,
        "AWOS" | "ASOS" => FrequencyKind::Awos,
        "CLNC DEL" => FrequencyKind::Clearance,
        _ => FrequencyKind::Other,
    }
}

/// Identifier to key an [`Airport`] by: prefer ICAO, fall back to the FAA
/// local identifier when a facility has no ICAO code (common for small
/// GA airports).
fn airport_key(row: &AptBaseRow) -> String {
    row.icao_id.clone().unwrap_or_else(|| row.arpt_id.clone())
}

pub fn airport_from_row(row: &AptBaseRow) -> Airport {
    Airport {
        icao: airport_key(row),
        faa_id: Some(row.arpt_id.clone()),
        iata: None,
        name: row.arpt_name.clone(),
        lat: row.lat_decimal,
        lon: row.long_decimal,
        elevation_ft: row.elevation_ft.round() as i32,
        airport_type: site_type(&row.site_type_code),
        fuel_types: Vec::new(),
    }
}

/// NASR runways are one row per end pair; without both ends' lat/lon in
/// this row shape we can't populate [`RunwayEnd`] coordinates/headings, so
/// this leaves them zeroed pending a richer row (`APT_RWY_END.csv`).
pub fn runway_from_row(row: &AptRunwayRow, airport_icao: &str) -> Runway {
    Runway {
        airport_icao: airport_icao.to_string(),
        ident: row.rwy_id.clone(),
        length_ft: row.rwy_len_ft,
        width_ft: row.rwy_width_ft,
        surface: surface_type(&row.surface_type_code),
        low_end: RunwayEnd {
            ident: row.rwy_id.split('/').next().unwrap_or_default().to_string(),
            lat: 0.0,
            lon: 0.0,
            heading_deg: 0.0,
        },
        high_end: RunwayEnd {
            ident: row.rwy_id.split('/').nth(1).unwrap_or_default().to_string(),
            lat: 0.0,
            lon: 0.0,
            heading_deg: 0.0,
        },
    }
}

pub fn frequency_from_row(row: &AptFrequencyRow, airport_icao: &str) -> Frequency {
    Frequency {
        airport_icao: airport_icao.to_string(),
        kind: comm_type(&row.comm_type_code),
        freq_mhz: row.comm_freq_mhz,
        remarks: row.remark.clone(),
    }
}
