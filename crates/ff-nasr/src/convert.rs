use crate::records::{AptBaseRow, AptFrequencyRow, AptRunwayEndRow, AptRunwayRow};
use ff_core::{Airport, AirportType, Frequency, FrequencyKind, Runway, RunwayEnd, RunwaySurface};
use std::collections::HashMap;

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

/// Builds a `SITE_NO` -> airport ident lookup from `APT_BASE` rows.
/// `APT_RWY`/`APT_RWY_END`/`APT_FREQ` rows only carry `SITE_NO` (see
/// module docs on [`crate::records`]), so this is the join needed before
/// calling [`runway_from_rows`]/[`frequency_from_row`] on them.
pub fn site_no_index(airports: &[AptBaseRow]) -> HashMap<String, String> {
    airports
        .iter()
        .map(|a| (a.site_no.clone(), airport_key(a)))
        .collect()
}

fn parse_end_number(rwy_end_id: &str) -> u32 {
    rwy_end_id
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .unwrap_or(0)
}

/// Builds a [`Runway`] from an `APT_RWY` row plus its `APT_RWY_END` rows
/// (already filtered to the same airport; this filters further to the
/// matching `RWY_ID`). Ends are ordered by their numeric heading so
/// `low_end`/`high_end` are consistent regardless of file order — real
/// end coordinates/true heading populate `RunwayEnd`, unlike `ff-cifp`'s
/// runway extraction which only has this airport's *magnetic* bearing.
/// An end missing from `ends` (incomplete data) falls back to a
/// zero-coordinate placeholder rather than dropping the runway.
pub fn runway_from_rows(
    rwy: &AptRunwayRow,
    ends: &[AptRunwayEndRow],
    airport_icao: &str,
) -> Runway {
    let mut matching: Vec<&AptRunwayEndRow> =
        ends.iter().filter(|e| e.rwy_id == rwy.rwy_id).collect();
    matching.sort_by_key(|e| parse_end_number(&e.rwy_end_id));

    let make_end = |end: Option<&&AptRunwayEndRow>, fallback_ident: &str| RunwayEnd {
        ident: end
            .map(|e| e.rwy_end_id.clone())
            .unwrap_or_else(|| fallback_ident.to_string()),
        lat: end.map(|e| e.lat_decimal).unwrap_or(0.0),
        lon: end.map(|e| e.long_decimal).unwrap_or(0.0),
        heading_deg: end.and_then(|e| e.true_alignment).unwrap_or(0.0),
    };

    Runway {
        airport_icao: airport_icao.to_string(),
        ident: rwy.rwy_id.clone(),
        length_ft: rwy.rwy_len_ft,
        width_ft: rwy.rwy_width_ft,
        surface: surface_type(&rwy.surface_type_code),
        low_end: make_end(
            matching.first(),
            rwy.rwy_id.split('/').next().unwrap_or_default(),
        ),
        high_end: make_end(
            matching.get(1),
            rwy.rwy_id.split('/').nth(1).unwrap_or_default(),
        ),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn ksfo_base_row() -> AptBaseRow {
        AptBaseRow {
            site_no: "12345.*A".to_string(),
            arpt_id: "SFO".to_string(),
            icao_id: Some("KSFO".to_string()),
            arpt_name: "SAN FRANCISCO INTL".to_string(),
            lat_decimal: 37.6188,
            long_decimal: -122.375,
            elevation_ft: 13.0,
            site_type_code: "A".to_string(),
        }
    }

    #[test]
    fn site_no_index_prefers_icao_and_falls_back_to_faa_ident() {
        let no_icao = AptBaseRow {
            icao_id: None,
            site_no: "99999.*A".to_string(),
            arpt_id: "1C9".to_string(),
            ..ksfo_base_row()
        };
        let index = site_no_index(&[ksfo_base_row(), no_icao]);
        assert_eq!(index.get("12345.*A"), Some(&"KSFO".to_string()));
        assert_eq!(index.get("99999.*A"), Some(&"1C9".to_string()));
    }

    #[test]
    fn runway_from_rows_pairs_matching_ends_by_number_and_uses_real_coordinates() {
        let rwy = AptRunwayRow {
            site_no: "12345.*A".to_string(),
            rwy_id: "01/19".to_string(),
            rwy_len_ft: 7650,
            rwy_width_ft: 200,
            surface_type_code: "ASPH".to_string(),
        };
        // Deliberately out of order, to prove sorting-by-number (not file
        // order) determines low_end/high_end.
        let ends = vec![
            AptRunwayEndRow {
                site_no: "12345.*A".to_string(),
                rwy_id: "01/19".to_string(),
                rwy_end_id: "19".to_string(),
                true_alignment: Some(194.0),
                lat_decimal: 37.63,
                long_decimal: -122.36,
            },
            AptRunwayEndRow {
                site_no: "12345.*A".to_string(),
                rwy_id: "01/19".to_string(),
                rwy_end_id: "01".to_string(),
                true_alignment: Some(14.0),
                lat_decimal: 37.6167,
                long_decimal: -122.39,
            },
        ];

        let runway = runway_from_rows(&rwy, &ends, "KSFO");
        assert_eq!(runway.airport_icao, "KSFO");
        assert_eq!(runway.surface, RunwaySurface::Asphalt);
        assert_eq!(runway.low_end.ident, "01");
        assert_eq!(runway.low_end.heading_deg, 14.0);
        assert_eq!(runway.low_end.lat, 37.6167);
        assert_eq!(runway.high_end.ident, "19");
        assert_eq!(runway.high_end.heading_deg, 194.0);
    }

    #[test]
    fn runway_from_rows_falls_back_when_an_end_is_missing() {
        let rwy = AptRunwayRow {
            site_no: "1.*A".to_string(),
            rwy_id: "13/31".to_string(),
            rwy_len_ft: 2441,
            rwy_width_ft: 70,
            surface_type_code: "TURF".to_string(),
        };
        let runway = runway_from_rows(&rwy, &[], "KPAO");
        assert_eq!(runway.surface, RunwaySurface::Turf);
        assert_eq!(runway.low_end.ident, "13");
        assert_eq!(runway.low_end.lat, 0.0);
        assert_eq!(runway.high_end.ident, "31");
    }

    #[test]
    fn airport_from_row_prefers_icao_over_faa_local_id() {
        let airport = airport_from_row(&ksfo_base_row());
        assert_eq!(airport.icao, "KSFO");
        assert_eq!(airport.faa_id.as_deref(), Some("SFO"));
        assert_eq!(airport.elevation_ft, 13);
        assert_eq!(airport.airport_type, AirportType::Airport);
    }
}
