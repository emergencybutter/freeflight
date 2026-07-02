use crate::records::{AptBaseRow, AptRunwayEndRow, AptRunwayRow, FrqRow};
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

/// Real `SURFACE_TYPE_CODE` values (verified against a live NASR extract)
/// include compound codes like `"ASPH-CONC"` or `"TURF-GRVL"` for mixed
/// surfaces; this picks the first-listed material as the primary surface
/// rather than modeling every combination.
fn surface_type(code: &str) -> RunwaySurface {
    match code {
        c if c.starts_with('A') => RunwaySurface::Asphalt, // ASPH, ASPH-CONC, ...
        c if c.starts_with('C') => RunwaySurface::Concrete, // CONC, CONC-TURF, ...
        c if c.starts_with('T') => RunwaySurface::Turf,    // TURF, TURF-DIRT, ...
        c if c.starts_with('G') => RunwaySurface::Gravel,  // GRVL, GRAVEL, ...
        c if c.starts_with('W') => RunwaySurface::Water,   // WATER
        _ => RunwaySurface::Other,                         // DIRT, MATS, ROOF-TOP, ...
    }
}

/// Maps a real `FRQ.csv` `FREQ_USE` code onto the coarse
/// [`FrequencyKind`] `ff-core` models. `FREQ_USE` is free text (see
/// [`FrqRow`] docs) with combined forms like `"APCH/P DEP/P"` — checked
/// in priority order so a combined form lands on the first kind it
/// mentions; the original text always survives in
/// [`Frequency::remarks`], so nothing is lost by this bucketing.
pub fn freq_use_kind(freq_use: &str) -> FrequencyKind {
    let upper = freq_use.to_ascii_uppercase();
    if upper == "CTAF" {
        FrequencyKind::Ctaf
    } else if upper == "UNICOM" {
        FrequencyKind::Unicom
    } else if upper.starts_with("LCL") {
        FrequencyKind::Tower
    } else if upper.starts_with("GND") {
        FrequencyKind::Ground
    } else if upper.starts_with("CD") {
        FrequencyKind::Clearance
    } else if upper.contains("ATIS") {
        FrequencyKind::Atis
    } else if upper.starts_with("AWOS") || upper.starts_with("ASOS") {
        FrequencyKind::Awos
    } else if upper.contains("APCH") {
        FrequencyKind::Approach
    } else if upper.contains("DEP") {
        FrequencyKind::Departure
    } else {
        FrequencyKind::Other
    }
}

/// `FRQ.csv`'s `FREQ` column is usually a plain number but can carry a
/// DME channel pair (`"116.65/113Y"`) or a receive-only suffix
/// (`"122.1R"`) on navaid-serviced rows (see [`FrqRow`] docs); this
/// parses the leading numeric part and ignores the rest rather than
/// failing the whole row.
fn parse_freq_mhz(raw: &str) -> Option<f64> {
    let numeric_prefix: String = raw
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    numeric_prefix.parse().ok()
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
        fuel_types: row
            .fuel_types
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect(),
    }
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
/// `low_end`/`high_end` are consistent regardless of file order.
///
/// Missing data is common, not exceptional, for `APT_RWY_END` (~41% of
/// real rows have no coordinates — see [`AptRunwayEndRow`] docs) and is
/// handled at the field level: an end row with a blank `LAT_DECIMAL`
/// still contributes its ident/heading if present, falling back to a
/// zero placeholder only for the fields that are actually missing. An
/// end missing from `ends` entirely falls back on every field.
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
        lat: end.and_then(|e| e.lat_decimal).unwrap_or(0.0),
        lon: end.and_then(|e| e.long_decimal).unwrap_or(0.0),
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

/// Picks the `FRQ.csv` rows that are this airport's own communication
/// frequencies — `SERVICED_FACILITY == arpt_id` (the FAA local id, e.g.
/// `"PAO"`, not the ICAO id) *and* `SERVICED_SITE_TYPE == "AIRPORT"` —
/// since the same table also carries navaid/FSS/AWOS frequencies and
/// TRACON entries that serve many airports at once (verified: real
/// `FRQ.csv` has 33 distinct `SERVICED_SITE_TYPE` values). Rows whose
/// `FREQ` doesn't parse (see [`FrqRow`] docs) are skipped rather than
/// erroring the whole airport.
pub fn frequencies_for_airport(
    rows: &[FrqRow],
    arpt_id: &str,
    airport_icao: &str,
) -> Vec<Frequency> {
    rows.iter()
        .filter(|r| r.serviced_facility == arpt_id && r.serviced_site_type == "AIRPORT")
        .filter_map(|r| {
            let freq_mhz = parse_freq_mhz(&r.freq)?;
            Some(Frequency {
                airport_icao: airport_icao.to_string(),
                kind: freq_use_kind(&r.freq_use),
                freq_mhz,
                remarks: Some(r.freq_use.clone()),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ksfo_base_row() -> AptBaseRow {
        AptBaseRow {
            site_no: "02187.".to_string(),
            arpt_id: "SFO".to_string(),
            icao_id: Some("KSFO".to_string()),
            arpt_name: "SAN FRANCISCO INTL".to_string(),
            lat_decimal: 37.6188,
            long_decimal: -122.375,
            elevation_ft: 13.0,
            site_type_code: "A".to_string(),
            fuel_types: "100LL,A,A++".to_string(),
        }
    }

    #[test]
    fn airport_from_row_parses_comma_separated_fuel_types() {
        let airport = airport_from_row(&ksfo_base_row());
        assert_eq!(airport.icao, "KSFO");
        assert_eq!(airport.faa_id.as_deref(), Some("SFO"));
        assert_eq!(airport.elevation_ft, 13);
        assert_eq!(airport.fuel_types, vec!["100LL", "A", "A++"]);
    }

    #[test]
    fn airport_from_row_handles_no_fuel_available() {
        let row = AptBaseRow {
            fuel_types: "".to_string(),
            ..ksfo_base_row()
        };
        let airport = airport_from_row(&row);
        assert!(airport.fuel_types.is_empty());
    }

    // Real KPAO (Palo Alto) runway 13/31 data, from the 11 Jun 2026 NASR
    // CSV subscription.
    #[test]
    fn runway_from_rows_pairs_matching_ends_by_number_and_uses_real_coordinates() {
        let rwy = AptRunwayRow {
            arpt_id: "PAO".to_string(),
            rwy_id: "13/31".to_string(),
            rwy_len_ft: 2441,
            rwy_width_ft: 70,
            surface_type_code: "ASPH".to_string(),
        };
        let ends = vec![
            AptRunwayEndRow {
                arpt_id: "PAO".to_string(),
                rwy_id: "13/31".to_string(),
                rwy_end_id: "31".to_string(),
                true_alignment: Some(322.0),
                lat_decimal: Some(37.45849213),
                long_decimal: Some(-122.11243811),
            },
            AptRunwayEndRow {
                arpt_id: "PAO".to_string(),
                rwy_id: "13/31".to_string(),
                rwy_end_id: "13".to_string(),
                true_alignment: Some(142.0),
                lat_decimal: Some(37.46375061),
                long_decimal: Some(-122.11765513),
            },
        ];

        let runway = runway_from_rows(&rwy, &ends, "KPAO");
        assert_eq!(runway.surface, RunwaySurface::Asphalt);
        assert_eq!(runway.low_end.ident, "13");
        assert_eq!(runway.low_end.heading_deg, 142.0);
        assert_eq!(runway.low_end.lat, 37.46375061);
        assert_eq!(runway.high_end.ident, "31");
        assert_eq!(runway.high_end.heading_deg, 322.0);
    }

    #[test]
    fn runway_from_rows_falls_back_when_an_end_is_missing() {
        let rwy = AptRunwayRow {
            arpt_id: "PAO".to_string(),
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

    // Real, common case (~41% of APT_RWY_END rows): the end row exists
    // with a real ident/heading but no coordinates at all.
    #[test]
    fn runway_from_rows_falls_back_per_field_when_an_end_has_no_coordinates() {
        let rwy = AptRunwayRow {
            arpt_id: "AL03".to_string(),
            rwy_id: "01/19".to_string(),
            rwy_len_ft: 2000,
            rwy_width_ft: 50,
            surface_type_code: "TURF".to_string(),
        };
        let ends = vec![AptRunwayEndRow {
            arpt_id: "AL03".to_string(),
            rwy_id: "01/19".to_string(),
            rwy_end_id: "01".to_string(),
            true_alignment: None,
            lat_decimal: None,
            long_decimal: None,
        }];
        let runway = runway_from_rows(&rwy, &ends, "AL03");
        assert_eq!(runway.low_end.ident, "01"); // real ident, even with no coordinates
        assert_eq!(runway.low_end.lat, 0.0);
        assert_eq!(runway.low_end.lon, 0.0);
        assert_eq!(runway.high_end.ident, "19"); // no row at all for this end
    }

    #[test]
    fn freq_use_kind_maps_real_faa_codes() {
        assert_eq!(freq_use_kind("CTAF"), FrequencyKind::Ctaf);
        assert_eq!(freq_use_kind("UNICOM"), FrequencyKind::Unicom);
        assert_eq!(freq_use_kind("LCL/P"), FrequencyKind::Tower);
        assert_eq!(freq_use_kind("GND/P"), FrequencyKind::Ground);
        assert_eq!(freq_use_kind("CD/P"), FrequencyKind::Clearance);
        assert_eq!(freq_use_kind("ATIS"), FrequencyKind::Atis);
        assert_eq!(freq_use_kind("D-ATIS"), FrequencyKind::Atis);
        // Combined forms land on the first kind mentioned.
        assert_eq!(freq_use_kind("APCH/P DEP/P"), FrequencyKind::Approach);
        assert_eq!(freq_use_kind("DEP/P"), FrequencyKind::Departure);
        assert_eq!(freq_use_kind("EMERG"), FrequencyKind::Other);
    }

    #[test]
    fn parse_freq_mhz_ignores_dme_channel_and_receive_only_suffixes() {
        assert_eq!(parse_freq_mhz("118.6"), Some(118.6));
        assert_eq!(parse_freq_mhz("122.1R"), Some(122.1));
        assert_eq!(parse_freq_mhz("116.65/113Y"), Some(116.65));
    }

    // Real KPAO FRQ.csv rows (11 Jun 2026 cycle): CTAF, UNICOM, tower,
    // ground, ATIS, plus a TRACON approach/departure row and a row for a
    // different airport that must be excluded.
    #[test]
    fn frequencies_for_airport_filters_to_this_airports_own_rows() {
        let rows = vec![
            FrqRow {
                facility: "PAO".into(),
                facility_type: "ATCT".into(),
                serviced_facility: "PAO".into(),
                serviced_site_type: "AIRPORT".into(),
                freq: "118.6".into(),
                freq_use: "CTAF".into(),
                remark: None,
            },
            FrqRow {
                facility: "PAO".into(),
                facility_type: "ATCT".into(),
                serviced_facility: "PAO".into(),
                serviced_site_type: "AIRPORT".into(),
                freq: "122.95".into(),
                freq_use: "UNICOM".into(),
                remark: None,
            },
            FrqRow {
                facility: "NCT".into(),
                facility_type: "TRACON".into(),
                serviced_facility: "PAO".into(),
                serviced_site_type: "AIRPORT".into(),
                freq: "121.3".into(),
                freq_use: "APCH/P".into(),
                remark: None,
            },
            // A navaid-serviced row for the same FACILITY that should
            // NOT show up in PAO's frequency list.
            FrqRow {
                facility: "SFO".into(),
                facility_type: "VORTAC".into(),
                serviced_facility: "SFO".into(),
                serviced_site_type: "VORTAC".into(),
                freq: "115.8".into(),
                freq_use: "NAVAID".into(),
                remark: None,
            },
            // A different airport entirely.
            FrqRow {
                facility: "SFO".into(),
                facility_type: "ATCT".into(),
                serviced_facility: "SFO".into(),
                serviced_site_type: "AIRPORT".into(),
                freq: "120.5".into(),
                freq_use: "LCL/P".into(),
                remark: None,
            },
        ];

        let freqs = frequencies_for_airport(&rows, "PAO", "KPAO");
        assert_eq!(freqs.len(), 3);
        assert!(freqs.iter().all(|f| f.airport_icao == "KPAO"));
        assert!(freqs
            .iter()
            .any(|f| f.kind == FrequencyKind::Ctaf && f.freq_mhz == 118.6));
        assert!(freqs.iter().any(|f| f.kind == FrequencyKind::Unicom));
        assert!(freqs.iter().any(|f| f.kind == FrequencyKind::Approach));
    }
}
