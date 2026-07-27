//! openAIP wire types → `ff-core` types, and the decoding of openAIP's
//! numeric enums.
//!
//! # Every enum here was derived, not documented
//!
//! openAIP publishes no schema or enum endpoint (`/api/{docs,schema,
//! enums,openapi.json}` all 404 as of 2026-07-26). Each mapping below
//! was derived by correlating live data against facts that are
//! independently known — a German CTR is Class D, an NDB transmits in
//! kHz, EDDF's runway 18 is 4,000 m long — and the evidence is recorded
//! beside it. Tests assert those same real-world facts, so a wrong or
//! changed mapping fails loudly instead of quietly mislabelling data in
//! a flight-planning tool.
//!
//! Where the evidence does not support a distinction, this module maps
//! coarsely and says so rather than guessing precisely. **Unknown codes
//! are never guessed**: they become the conservative value, or the
//! feature is skipped.

use crate::model;
use ff_core::airport::{Airport, AirportType};
use ff_core::airspace::{AirspaceClass, AirspaceVolume, AltitudeLimit, Polygon, SpecialUseKind};
use ff_core::navaid::{Navaid, NavaidType};

/// Unit codes seen on `Measure.unit`.
///
/// Evidence: `elevation.unit = 0` on EDDF whose field elevation is ~111 m;
/// EDDF runway 18 `length {value: 3999, unit: 0}` against its real
/// 4,000 m. Airspace limits use `1` with values like 4500 (feet) and `6`
/// with values like 100 (i.e. FL100).
pub struct UnitCode;

impl UnitCode {
    pub const METRES: i32 = 0;
    pub const FEET: i32 = 1;
    /// Frequencies: `2` = MHz (VHF, "115.800"), `1` = kHz (NDB, "341.000").
    pub const MHZ: i32 = 2;
    pub const KHZ: i32 = 1;
    pub const FLIGHT_LEVEL: i32 = 6;
}

/// `Measure.referenceDatum`. Evidence: the three combinations that occur
/// in German airspace are `(unit 1, datum 0)` for heights like 2500,
/// `(unit 1, datum 1)` for altitudes like 4500, and `(unit 6, datum 2)`
/// for FL100 — i.e. ground, mean sea level, and standard pressure.
pub struct DatumCode;

impl DatumCode {
    pub const GND: i32 = 0;
    pub const MSL: i32 = 1;
    pub const STD: i32 = 2;
}

const METRES_TO_FEET: f64 = 3.280_839_895;

/// A length in feet, converting from metres when that is what the API
/// sent. Returns `None` for an unknown unit rather than assuming — a
/// silently mis-scaled elevation is exactly the failure this crate must
/// not produce.
pub fn length_ft(m: &model::Measure) -> Option<f64> {
    let value = m.as_f64()?;
    match m.unit {
        Some(UnitCode::METRES) => Some(value * METRES_TO_FEET),
        Some(UnitCode::FEET) => Some(value),
        _ => None,
    }
}

/// Airport `type`. openAIP's codes are finer-grained than `ff-core`'s
/// four, and the exact code list is undocumented; what is safe to say is
/// that everything in the feed is a place aircraft operate from. Mapping
/// is therefore coarse and deliberately conservative: only the codes with
/// clear evidence are special-cased.
///
/// VERIFY: heliport/seaplane/ultralight codes are not yet pinned down, so
/// everything currently lands on `Airport`. That understates variety but
/// never misrepresents a field as something it is not.
pub fn airport_type(_code: Option<i32>) -> AirportType {
    AirportType::Airport
}

/// Converts an airport. `None` when it has no usable position, or no
/// identifier at all.
///
/// **Identity**: `ff-core::Airport::icao` is the primary key, but most
/// openAIP entries have no ICAO code (of 1364 German airports only a
/// minority do). Rather than drop them — which would discard most of the
/// country — an entry without one is keyed on its openAIP id, prefixed so
/// a synthetic key can never be mistaken for a real ICAO code.
pub fn airport(a: &model::Airport, fallback_id: &str) -> Option<Airport> {
    let (lat, lon) = a.geometry.point()?;
    let icao = match a.icao_code.as_deref().map(str::trim) {
        Some(code) if !code.is_empty() => code.to_uppercase(),
        _ => {
            if fallback_id.is_empty() {
                return None;
            }
            format!("OAIP:{fallback_id}")
        }
    };
    Some(Airport {
        icao,
        faa_id: None,
        iata: a
            .iata_code
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_uppercase),
        name: a.name.trim().to_string(),
        lat,
        lon,
        elevation_ft: a
            .elevation
            .as_ref()
            .and_then(length_ft)
            .map(|ft| ft.round() as i32)
            .unwrap_or(0),
        airport_type: airport_type(a.kind),
        fuel_types: Vec::new(),
    })
}

/// Navaid `type`, decoded as far as the evidence goes.
///
/// Evidence from live German data:
/// - `2` transmits in **kHz** (341.000, 330.000) and carries no TACAN
///   `channel` → **NDB**. High confidence.
/// - Codes with no `channel` on a VHF frequency → a **VOR** with no
///   co-located ranging aid.
/// - Codes *with* a TACAN `channel` (`"105X"`, `"88X"`) → a VOR-family
///   station with a co-located DME/TACAN → **VOR-DME**.
///
/// VERIFY: openAIP distinguishes codes 0/1/4/7/8 among the
/// channel-carrying stations, which presumably separates VOR-DME from
/// VORTAC and DME-only. That split could not be established from the data
/// (all are VHF with a channel), so they are mapped to the common
/// `VorDme`. This mirrors `ff-aixm`'s deferred co-location merge: a
/// coarse type is honest, an invented one is not.
pub fn navaid_type(n: &model::Navaid) -> Option<NavaidType> {
    let unit = n.frequency.as_ref().and_then(|f| f.unit);
    if unit == Some(UnitCode::KHZ) {
        return Some(NavaidType::Ndb);
    }
    if unit != Some(UnitCode::MHZ) {
        // Neither a known NDB nor a known VHF station: skip rather than
        // invent a type.
        return None;
    }
    Some(match n.channel.as_deref().map(str::trim) {
        Some(ch) if !ch.is_empty() => NavaidType::VorDme,
        _ => NavaidType::Vor,
    })
}

/// Converts a navaid. `region` is stamped by the caller from the dataset
/// (e.g. `"ED"` for Germany) — openAIP carries a country code, not an
/// ICAO region, and the two differ.
pub fn navaid(n: &model::Navaid, region: &str) -> Option<Navaid> {
    let (lat, lon) = n.geometry.point()?;
    let ident = n
        .identifier
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())?;
    let navaid_type = navaid_type(n)?;
    // ff-core stores frequency in kHz; openAIP sends MHz for VHF.
    let freq_khz = n.frequency.as_ref().and_then(|f| {
        let value = f.as_f64()?;
        match f.unit {
            Some(UnitCode::KHZ) => Some(value.round() as u32),
            Some(UnitCode::MHZ) => Some((value * 1000.0).round() as u32),
            _ => None,
        }
    });
    Some(Navaid {
        ident: ident.to_uppercase(),
        navaid_type,
        lat,
        lon,
        elevation_ft: n
            .elevation
            .as_ref()
            .and_then(length_ft)
            .map(|ft| ft.round() as i32),
        freq_khz,
        region: region.to_string(),
    })
}

/// `icaoClass` → `ff-core`'s class. **Safety-relevant**: this drives
/// §9.3's VFR Class B/C/D warnings.
///
/// Evidence (live German data, 2026-07-26): `CTR ANSBACH`/`AUGSBURG`/
/// `BERLIN` all report 3, and German CTRs are Class D. 87 entries report
/// 2 and are all TMAs, which in Germany are Class C. 110 report 4 (Class
/// E, Germany's bulk controlled airspace). 397 report 8 and are `ED-R`
/// restricted areas, `ED-D` danger areas, RMZs and glider sectors —
/// things with no ICAO class at all. Consistent with the natural
/// `0=A … 6=G` ordering.
///
/// Confirmed against two further states, which is what turned the German
/// inference into a settled mapping:
/// - **UK**: 107 volumes report 0 and are CTAs (`BERRY HEAD CTA`) — UK
///   CTAs are Class **A**. Class A is the airspace VFR traffic may not
///   enter, so mapping it is not optional.
/// - **Canada**: 279 volumes report 1 and are named `A1 AIRWAY` /
///   `A10 AIRWAY` — Canadian high-level airways are Class **B**. 85
///   report 5 (Class **F**, Canada's advisory areas), and control zones
///   (`ABBOTSFORD CZ`) report 2/3 as expected.
///
/// So the natural `0=A, 1=B, 2=C, 3=D, 4=E, 5=F, 6=G` ordering holds
/// across Germany, the UK, Canada and Greenland, with `8` meaning "no
/// ICAO class".
///
/// Returns `None` for anything unrecognised. It must never be promoted to
/// a class the data did not claim.
pub fn airspace_class(icao_class: Option<i32>) -> Option<AirspaceClass> {
    match icao_class? {
        0 => Some(AirspaceClass::A),
        1 => Some(AirspaceClass::B),
        2 => Some(AirspaceClass::C),
        3 => Some(AirspaceClass::D),
        4 => Some(AirspaceClass::E),
        5 => Some(AirspaceClass::F),
        6 => Some(AirspaceClass::G),
        _ => None,
    }
}

/// An altitude limit, from the `(unit, referenceDatum)` pair.
///
/// Evidence: only three combinations occur in German airspace — feet
/// above ground, feet above MSL, and flight levels. Ground at zero is
/// `Surface`, which is how the FAA source expresses it too.
pub fn altitude_limit(m: Option<&model::Measure>) -> AltitudeLimit {
    let Some(m) = m else {
        return AltitudeLimit::Unlimited;
    };
    let Some(value) = m.as_f64() else {
        return AltitudeLimit::Unlimited;
    };
    let rounded = value.max(0.0).round() as u32;
    match (m.unit, m.reference_datum) {
        (Some(UnitCode::FLIGHT_LEVEL), _) | (_, Some(DatumCode::STD)) => {
            AltitudeLimit::FlightLevel(rounded)
        }
        (Some(UnitCode::FEET), Some(DatumCode::GND)) if rounded == 0 => AltitudeLimit::Surface,
        (Some(UnitCode::FEET), Some(DatumCode::GND)) => AltitudeLimit::Agl(rounded),
        (Some(UnitCode::FEET), Some(DatumCode::MSL)) => AltitudeLimit::Msl(rounded),
        (Some(UnitCode::METRES), datum) => {
            let ft = (value * METRES_TO_FEET).max(0.0).round() as u32;
            if datum == Some(DatumCode::GND) {
                if ft == 0 {
                    AltitudeLimit::Surface
                } else {
                    AltitudeLimit::Agl(ft)
                }
            } else {
                AltitudeLimit::Msl(ft)
            }
        }
        _ => AltitudeLimit::Unlimited,
    }
}

/// Airspace `type` codes, for volumes that carry no ICAO class.
///
/// Evidence (live German data): every one of the 143 `type: 1` entries is
/// an `ED-R…` restricted area and all 18 `type: 2` are `ED-D…` danger
/// areas; `type: 4` is CTR, `7` TMA, `6` RMZ, `21` glider sector, `28`
/// parachute area.
pub struct AirspaceTypeCode;

impl AirspaceTypeCode {
    pub const RESTRICTED: i32 = 1;
    pub const DANGER: i32 = 2;
    /// UK `P611 COULPORT/FASLANE`, `P813 DOUNREAY` — the "P" designator
    /// is a prohibited area (a nuclear submarine base and a nuclear
    /// site). Absolute no-entry, so of everything here this is the one
    /// that must not be dropped.
    pub const PROHIBITED: i32 = 3;
    pub const CTR: i32 = 4;
    /// `LONDON STANSTED TMZ 1` — transponder mandatory.
    pub const TMZ: i32 = 5;
    /// `RMZ EDAB`, `HAWARDEN RMZ 1` — radio mandatory.
    pub const RMZ: i32 = 6;
    pub const TMA: i32 = 7;
    /// `EDGG`/`EDMM`/`EDWW` — German FIR/UIR boundaries. Deliberately
    /// *not* imported: they blanket the whole country, so drawing them as
    /// airspace would bury every real warning under a country-sized
    /// polygon.
    pub const FIR: i32 = 10;
    /// `ATZ ESSEN-MUELHEIM` — aerodrome traffic zone.
    pub const ATZ: i32 = 13;
    /// `LFA 1 NIEDERSACHSEN` — military low flying area.
    pub const LOW_FLYING: i32 = 25;
    /// German `UGR` sectors, UK national soaring areas.
    pub const GLIDER_SECTOR: i32 = 21;
    /// German entries are literally named `PARA …`, which is what pins
    /// this code down; the 194 UK entries share it.
    pub const PARACHUTE: i32 = 28;
}

/// Converts an airspace volume. `None` when it has no polygon, or when
/// its class cannot be established.
pub fn airspace(a: &model::Airspace) -> Option<AirspaceVolume> {
    let points = a.geometry.outer_ring()?;
    if points.len() < 3 {
        return None;
    }
    let class = airspace_class(a.icao_class).or_else(|| special_use_from_type(a.kind))?;
    Some(AirspaceVolume {
        id: format!("OAIP:{}", a.id),
        name: a.name.trim().to_string(),
        class,
        floor: altitude_limit(a.lower_limit.as_ref()),
        ceiling: altitude_limit(a.upper_limit.as_ref()),
        boundary: Polygon { points },
    })
}

/// Special-use kind for volumes openAIP reports as having no ICAO class
/// (`icaoClass` 8), keyed on the **type code** rather than the name.
///
/// Name matching was the first approach here and was wrong: 17 German
/// restricted areas are named `EDR1 …` without the hyphen, so a
/// `contains("ED-R")` test silently dropped them. The type code is
/// structured data and catches all 143.
///
/// `ff-core` has no Danger kind — the FAA sources have no equivalent — so
/// danger areas map to `Warning`, the closest analogue (a hazard advisory
/// area). Anything else returns `None` and is skipped rather than shown
/// under an invented classification; see the crate docs for what that
/// currently excludes.
pub fn special_use_from_type(kind: Option<i32>) -> Option<AirspaceClass> {
    Some(AirspaceClass::SpecialUse(match kind? {
        AirspaceTypeCode::RESTRICTED => SpecialUseKind::Restricted,
        AirspaceTypeCode::DANGER => SpecialUseKind::Warning,
        AirspaceTypeCode::PROHIBITED => SpecialUseKind::Prohibited,
        AirspaceTypeCode::RMZ => SpecialUseKind::Rmz,
        AirspaceTypeCode::TMZ => SpecialUseKind::Tmz,
        AirspaceTypeCode::ATZ => SpecialUseKind::Atz,
        AirspaceTypeCode::GLIDER_SECTOR => SpecialUseKind::Glider,
        AirspaceTypeCode::PARACHUTE => SpecialUseKind::Parachute,
        AirspaceTypeCode::LOW_FLYING => SpecialUseKind::LowFlying,
        // Deliberately dropped, not overlooked: FIR/UIR boundaries
        // (`AirspaceTypeCode::FIR`) are country-sized and would bury
        // every real warning, and FIS sectors (33) are an information
        // service rather than a restriction. UK type 18 (91 volumes) is
        // dropped because its meaning could not be established — skipped
        // rather than guessed.
        _ => return None,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn measure(value: f64, unit: i32, datum: Option<i32>) -> model::Measure {
        model::Measure {
            value: serde_json::json!(value),
            unit: Some(unit),
            reference_datum: datum,
        }
    }

    #[test]
    fn metres_become_feet_and_unknown_units_are_refused() {
        // EDDF runway 18 is 3999 m in the feed; ~13,120 ft.
        let m = measure(3999.0, UnitCode::METRES, None);
        let ft = length_ft(&m).expect("metres convert");
        assert!((ft - 13119.1).abs() < 1.0, "got {ft}");

        // Already feet: unchanged.
        assert_eq!(
            length_ft(&measure(4500.0, UnitCode::FEET, None)),
            Some(4500.0)
        );

        // An unrecognised unit must not be assumed — a mis-scaled
        // elevation is invisible once it reaches the planner.
        assert_eq!(length_ft(&measure(100.0, 99, None)), None);
    }

    #[test]
    fn frequencies_arrive_as_strings() {
        let m = model::Measure {
            value: serde_json::json!("115.800"),
            unit: Some(UnitCode::MHZ),
            reference_datum: None,
        };
        assert_eq!(m.as_f64(), Some(115.8));
    }

    /// The mapping that drives VFR airspace warnings. These are real
    /// German airspace classes, not invented fixtures.
    #[test]
    fn icao_class_matches_known_real_world_airspace() {
        // Germany: CTR ANSBACH/AUGSBURG/BERLIN report 3, and German CTRs
        // are Class D. German TMAs report 2 and are Class C.
        assert_eq!(airspace_class(Some(3)), Some(AirspaceClass::D));
        assert_eq!(airspace_class(Some(2)), Some(AirspaceClass::C));
        assert_eq!(airspace_class(Some(4)), Some(AirspaceClass::E));
        assert_eq!(airspace_class(Some(6)), Some(AirspaceClass::G));

        // UK: 107 volumes report 0 and are CTAs (BERRY HEAD CTA), which
        // are Class A. Getting this wrong hides the one class VFR traffic
        // may not enter at all.
        assert_eq!(airspace_class(Some(0)), Some(AirspaceClass::A));

        // Canada: 279 report 1 and are named "A1 AIRWAY" — Canadian
        // high-level airways are Class B. 85 report 5, Canada's Class F
        // advisory areas.
        assert_eq!(airspace_class(Some(1)), Some(AirspaceClass::B));
        assert_eq!(airspace_class(Some(5)), Some(AirspaceClass::F));

        // 8 = "no ICAO class" (ED-R/ED-D/RMZ). Must NOT become a class.
        assert_eq!(airspace_class(Some(8)), None);
        // Unknown codes are never guessed.
        assert_eq!(airspace_class(Some(42)), None);
        assert_eq!(airspace_class(None), None);
    }

    #[test]
    fn special_use_comes_from_the_type_code_not_the_name() {
        assert_eq!(
            special_use_from_type(Some(AirspaceTypeCode::RESTRICTED)),
            Some(AirspaceClass::SpecialUse(SpecialUseKind::Restricted))
        );
        assert_eq!(
            special_use_from_type(Some(AirspaceTypeCode::DANGER)),
            Some(AirspaceClass::SpecialUse(SpecialUseKind::Warning))
        );
        // Regression: 17 German restricted areas are named "EDR1 …"
        // without the hyphen. A name-based `contains("ED-R")` test
        // dropped them; the type code does not care how it is spelled.
        let unhyphenated = model::Airspace {
            id: "x".into(),
            name: "EDR1 GARCHING H24".into(),
            country: "DE".into(),
            kind: Some(AirspaceTypeCode::RESTRICTED),
            icao_class: Some(8),
            upper_limit: None,
            lower_limit: None,
            geometry: model::Geometry::Polygon {
                coordinates: vec![vec![[10.0, 48.0], [10.1, 48.0], [10.1, 48.1], [10.0, 48.0]]],
            },
        };
        let converted = airspace(&unhyphenated).expect("restricted area converts");
        assert_eq!(
            converted.class,
            AirspaceClass::SpecialUse(SpecialUseKind::Restricted)
        );

        // UK prohibited areas (P611 Coulport/Faslane, P813 Dounreay).
        // Absolute no-entry: of everything here this is the one that
        // must never be dropped.
        assert_eq!(
            special_use_from_type(Some(AirspaceTypeCode::PROHIBITED)),
            Some(AirspaceClass::SpecialUse(SpecialUseKind::Prohibited))
        );

        // European kinds, each landing on its own type rather than a
        // generic bucket.
        for (code, expected) in [
            (AirspaceTypeCode::RMZ, SpecialUseKind::Rmz),
            (AirspaceTypeCode::TMZ, SpecialUseKind::Tmz),
            (AirspaceTypeCode::ATZ, SpecialUseKind::Atz),
            (AirspaceTypeCode::GLIDER_SECTOR, SpecialUseKind::Glider),
            (AirspaceTypeCode::PARACHUTE, SpecialUseKind::Parachute),
            (AirspaceTypeCode::LOW_FLYING, SpecialUseKind::LowFlying),
        ] {
            assert_eq!(
                special_use_from_type(Some(code)),
                Some(AirspaceClass::SpecialUse(expected)),
                "code {code}"
            );
        }

        // FIR boundaries are country-sized: importing them would bury
        // every real warning under one polygon.
        assert_eq!(special_use_from_type(Some(AirspaceTypeCode::FIR)), None);
        // Unknown codes stay unknown.
        assert_eq!(special_use_from_type(Some(18)), None);
    }

    #[test]
    fn altitude_limits_cover_the_three_real_combinations() {
        // (unit 1, datum 1) — 4500 ft MSL.
        assert_eq!(
            altitude_limit(Some(&measure(4500.0, UnitCode::FEET, Some(DatumCode::MSL)))),
            AltitudeLimit::Msl(4500)
        );
        // (unit 1, datum 0) — 2500 ft AGL, and 0 is the surface.
        assert_eq!(
            altitude_limit(Some(&measure(2500.0, UnitCode::FEET, Some(DatumCode::GND)))),
            AltitudeLimit::Agl(2500)
        );
        assert_eq!(
            altitude_limit(Some(&measure(0.0, UnitCode::FEET, Some(DatumCode::GND)))),
            AltitudeLimit::Surface
        );
        // (unit 6, datum 2) — FL100, NOT 100 ft.
        assert_eq!(
            altitude_limit(Some(&measure(
                100.0,
                UnitCode::FLIGHT_LEVEL,
                Some(DatumCode::STD)
            ))),
            AltitudeLimit::FlightLevel(100)
        );
        assert_eq!(altitude_limit(None), AltitudeLimit::Unlimited);
    }

    #[test]
    fn navaids_are_typed_from_frequency_band_and_channel() {
        let base = |unit: i32, channel: Option<&str>| model::Navaid {
            name: "TEST".into(),
            identifier: Some("TST".into()),
            kind: Some(0),
            country: "DE".into(),
            frequency: Some(model::Measure {
                value: serde_json::json!("341.000"),
                unit: Some(unit),
                reference_datum: None,
            }),
            elevation: None,
            geometry: model::Geometry::Point {
                coordinates: [10.0, 48.0],
            },
            channel: channel.map(str::to_string),
        };
        // kHz → NDB (ALLGAEU 341.000 kHz).
        assert_eq!(
            navaid_type(&base(UnitCode::KHZ, None)),
            Some(NavaidType::Ndb)
        );
        // VHF without a TACAN channel → plain VOR (MOENCHENGLADBACH).
        assert_eq!(
            navaid_type(&base(UnitCode::MHZ, None)),
            Some(NavaidType::Vor)
        );
        // VHF with a channel → co-located ranging aid (BERLIN "88X").
        assert_eq!(
            navaid_type(&base(UnitCode::MHZ, Some("88X"))),
            Some(NavaidType::VorDme)
        );
        // Unknown band: skipped, not guessed.
        assert_eq!(navaid_type(&base(42, None)), None);
    }

    #[test]
    fn navaid_frequency_normalises_to_khz() {
        let vhf = model::Navaid {
            name: "BERLIN".into(),
            identifier: Some("bbi".into()),
            kind: Some(7),
            country: "DE".into(),
            frequency: Some(model::Measure {
                value: serde_json::json!("114.100"),
                unit: Some(UnitCode::MHZ),
                reference_datum: None,
            }),
            elevation: Some(measure(100.0, UnitCode::METRES, Some(DatumCode::MSL))),
            geometry: model::Geometry::Point {
                coordinates: [13.5, 52.4],
            },
            channel: Some("88X".into()),
        };
        let n = navaid(&vhf, "ED").expect("converts");
        assert_eq!(n.ident, "BBI", "identifiers are upper-cased");
        assert_eq!(n.freq_khz, Some(114_100));
        assert_eq!(n.region, "ED");
        assert_eq!(n.elevation_ft, Some(328));
    }

    #[test]
    fn airports_without_an_icao_code_are_kept_under_a_marked_synthetic_key() {
        let mut a = model::Airport {
            name: "FRANKFURT BGU".into(),
            icao_code: None,
            iata_code: None,
            country: "DE".into(),
            kind: Some(7),
            geometry: model::Geometry::Point {
                coordinates: [8.5, 50.1],
            },
            elevation: Some(measure(207.0, UnitCode::METRES, Some(DatumCode::MSL))),
            runways: Vec::new(),
            frequencies: Vec::new(),
        };
        // Most German entries look like this; dropping them would discard
        // most of the country.
        let converted = airport(&a, "62614a341eacded7b7bbdc95").expect("converts");
        assert!(
            converted.icao.starts_with("OAIP:"),
            "a synthetic key must be unmistakable, got {}",
            converted.icao
        );
        assert_eq!(converted.elevation_ft, 679, "207 m ≈ 679 ft");

        // A real ICAO code wins and is upper-cased.
        a.icao_code = Some("eddf".into());
        assert_eq!(airport(&a, "x").expect("converts").icao, "EDDF");
    }
}
