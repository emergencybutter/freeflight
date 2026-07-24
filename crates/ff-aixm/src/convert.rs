//! AIXM 4.5 feature field-maps → `ff-core` domain types.
//!
//! The streaming [`crate::parser`] collects each feature's leaf elements
//! into a flat `field -> text` map (first occurrence wins — see the parser
//! docs); these functions interpret that map. Kept separate from the
//! parser so the field/unit/enum handling is unit-testable with hand-built
//! maps, and so every `// VERIFY:` note (things to confirm against a real
//! SIA export) lives in one place.
use crate::coord::{parse_lat, parse_lon};
use ff_core::{Airport, AirportType, Navaid, NavaidType, Waypoint};
use std::collections::HashMap;

const METERS_TO_FEET: f64 = 3.280_839_895;

type Fields = HashMap<String, String>;

fn get<'a>(f: &'a Fields, key: &str) -> Option<&'a str> {
    f.get(key).map(String::as_str).filter(|s| !s.is_empty())
}

/// Elevation → feet from a `valElev` value plus its `uomDistVer` unit.
/// VERIFY: SIA elevations are usually already feet (`uomDistVer=FT`);
/// `M` is converted. Unknown/missing unit is treated as feet.
fn elevation_ft(f: &Fields) -> Option<i32> {
    let val: f64 = get(f, "valElev")?.parse().ok()?;
    let ft = match get(f, "uomDistVer") {
        Some("M") => val * METERS_TO_FEET,
        _ => val, // FT or unspecified
    };
    Some(ft.round() as i32)
}

/// VERIFY: AIXM 4.5 `Ahp` `codeType` enum. `HP` = heliport; everything
/// else (incl. `AD` aerodrome, `AH` aerodrome+heliport) → `Airport`.
/// Seaplane/ultralight aren't distinguished by `codeType` here — revisit
/// against real data if that matters for display.
fn airport_type(code: Option<&str>) -> AirportType {
    match code {
        Some("HP") => AirportType::Heliport,
        _ => AirportType::Airport,
    }
}

/// Builds an [`Airport`] from an `Ahp` field-map. Returns `None` without a
/// usable ICAO id or coordinates (an airport that can't be keyed or placed
/// is skipped, the same discipline `ff-nasr` applies). `fuel_types` is
/// empty — AIXM models fuel via separate service features not read here.
pub fn airport_from_fields(f: &Fields) -> Option<Airport> {
    // `codeId` is the AhpUid identity (ICAO); `codeIcao` is a redundant
    // top-level copy — prefer the Uid, fall back to the copy.
    let icao = get(f, "codeId").or_else(|| get(f, "codeIcao"))?.to_string();
    let lat = parse_lat(get(f, "geoLat")?)?;
    let lon = parse_lon(get(f, "geoLong")?)?;
    Some(Airport {
        icao,
        faa_id: None,
        iata: get(f, "codeIata").map(str::to_string),
        name: get(f, "txtName").unwrap_or("").to_string(),
        lat,
        lon,
        elevation_ft: elevation_ft(f).unwrap_or(0),
        airport_type: airport_type(get(f, "codeType")),
        fuel_types: Vec::new(),
    })
}

/// Maps an AIXM navaid feature tag to a `ff-core` [`NavaidType`].
///
/// VERIFY: AIXM 4.5 models VOR-DME / VORTAC as *co-located separate*
/// features (a `Vor` plus a `Dme`/`Tcn` linked by Uid reference), so each
/// feature here maps to its own base type; merging co-located pairs into
/// `VorDme`/`Vortac` is a deferred follow-up (see crate docs).
fn navaid_type(tag: &str) -> Option<NavaidType> {
    match tag {
        "Vor" => Some(NavaidType::Vor),
        "Ndb" => Some(NavaidType::Ndb),
        "Dme" => Some(NavaidType::Dme),
        "Tcn" => Some(NavaidType::Tacan),
        _ => None,
    }
}

/// Navaid frequency → kHz from `valFreq` + `uomFreq`. VOR/DME report MHz,
/// NDB reports kHz; the unit disambiguates. VERIFY the `uomFreq` spellings
/// (`MHZ`/`KHZ`) against a real export.
fn freq_khz(f: &Fields) -> Option<u32> {
    let val: f64 = get(f, "valFreq")?.parse().ok()?;
    let khz = match get(f, "uomFreq") {
        Some("KHZ") => val,
        Some("MHZ") => val * 1000.0,
        // No unit: infer from magnitude (NDBs are ~200–1750 kHz; VHF nav
        // is ~108–118 "MHz" i.e. a small number).
        _ if val < 100.0 => val * 1000.0,
        _ => val,
    };
    Some(khz.round() as u32)
}

/// Builds a [`Navaid`] from a navaid feature (`tag` is the element name:
/// `Vor`/`Ndb`/`Dme`/`Tcn`). `region` is supplied by the parser (the
/// dataset's ICAO region, e.g. `"LF"` for France) since AIXM carries no
/// FAA-style region code on the feature. Returns `None` for an unmodeled
/// tag or missing id/coordinates.
pub fn navaid_from_fields(tag: &str, f: &Fields, region: &str) -> Option<Navaid> {
    let navaid_type = navaid_type(tag)?;
    let ident = get(f, "codeId")?.to_string();
    let lat = parse_lat(get(f, "geoLat")?)?;
    let lon = parse_lon(get(f, "geoLong")?)?;
    Some(Navaid {
        ident,
        navaid_type,
        lat,
        lon,
        elevation_ft: elevation_ft(f),
        freq_khz: freq_khz(f),
        region: region.to_string(),
    })
}

/// Builds a [`Waypoint`] from a `Dpn` (DesignatedPoint) field-map.
pub fn waypoint_from_fields(f: &Fields, region: &str) -> Option<Waypoint> {
    let ident = get(f, "codeId")?.to_string();
    let lat = parse_lat(get(f, "geoLat")?)?;
    let lon = parse_lon(get(f, "geoLong")?)?;
    Some(Waypoint {
        ident,
        lat,
        lon,
        region: region.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields(pairs: &[(&str, &str)]) -> Fields {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn airport_from_fields_maps_identity_name_coords_elev() {
        let f = fields(&[
            ("codeId", "LFPG"),
            ("txtName", "PARIS CHARLES DE GAULLE"),
            ("codeIcao", "LFPG"),
            ("codeIata", "CDG"),
            ("codeType", "AD"),
            ("geoLat", "490042.00N"),
            ("geoLong", "0023259.00E"),
            ("valElev", "119"),
            ("uomDistVer", "FT"),
        ]);
        let a = airport_from_fields(&f).unwrap();
        assert_eq!(a.icao, "LFPG");
        assert_eq!(a.iata.as_deref(), Some("CDG"));
        assert_eq!(a.airport_type, AirportType::Airport);
        assert_eq!(a.elevation_ft, 119);
        assert!((a.lat - (49.0 + 42.0 / 3600.0)).abs() < 1e-6);
    }

    #[test]
    fn airport_elevation_in_meters_is_converted() {
        let f = fields(&[
            ("codeId", "LFXX"),
            ("geoLat", "490000.00N"),
            ("geoLong", "0020000.00E"),
            ("valElev", "100"),
            ("uomDistVer", "M"),
        ]);
        let a = airport_from_fields(&f).unwrap();
        assert_eq!(a.elevation_ft, 328); // 100 m
    }

    #[test]
    fn airport_without_coords_or_id_is_skipped() {
        assert!(airport_from_fields(&fields(&[("codeId", "LFXX")])).is_none());
        assert!(airport_from_fields(&fields(&[("geoLat", "490000.00N"), ("geoLong", "0020000.00E")])).is_none());
    }

    #[test]
    fn heliport_code_type_maps_to_heliport() {
        let f = fields(&[
            ("codeId", "LFPH"),
            ("codeType", "HP"),
            ("geoLat", "485000.00N"),
            ("geoLong", "0022000.00E"),
        ]);
        assert_eq!(airport_from_fields(&f).unwrap().airport_type, AirportType::Heliport);
    }

    #[test]
    fn vor_freq_mhz_to_khz_ndb_stays_khz() {
        let vor = fields(&[
            ("codeId", "PON"),
            ("geoLat", "490900.00N"),
            ("geoLong", "0020200.00E"),
            ("valFreq", "111.6"),
            ("uomFreq", "MHZ"),
        ]);
        let n = navaid_from_fields("Vor", &vor, "LF").unwrap();
        assert_eq!(n.navaid_type, NavaidType::Vor);
        assert_eq!(n.region, "LF");
        assert_eq!(n.freq_khz, Some(111_600));

        let ndb = fields(&[
            ("codeId", "AB"),
            ("geoLat", "490900.00N"),
            ("geoLong", "0020200.00E"),
            ("valFreq", "380"),
            ("uomFreq", "KHZ"),
        ]);
        let n = navaid_from_fields("Ndb", &ndb, "LF").unwrap();
        assert_eq!(n.navaid_type, NavaidType::Ndb);
        assert_eq!(n.freq_khz, Some(380));
    }

    #[test]
    fn unmodeled_navaid_tag_is_skipped() {
        let f = fields(&[("codeId", "X"), ("geoLat", "490000.00N"), ("geoLong", "0020000.00E")]);
        assert!(navaid_from_fields("Marker", &f, "LF").is_none());
    }

    #[test]
    fn waypoint_from_dpn_fields() {
        let f = fields(&[
            ("codeId", "ABABA"),
            ("geoLat", "483000.00N"),
            ("geoLong", "0020600.00E"),
        ]);
        let w = waypoint_from_fields(&f, "LF").unwrap();
        assert_eq!(w.ident, "ABABA");
        assert_eq!(w.region, "LF");
    }
}
