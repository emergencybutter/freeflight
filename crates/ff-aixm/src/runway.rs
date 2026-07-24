//! Runway assembly from AIXM 4.5 `Rwy` (physical runway) + `Rdn` (runway
//! direction) features — a two-feature join, the AIXM analog of the way
//! `ff-cifp`/`ff-nasr` build a runway from separate end records.
//!
//! `Rwy` carries the physical strip (designator `"10/28"`, length, width,
//! surface) with the airport ICAO nested at `RwyUid/AhpUid/codeId`. Each
//! `Rdn` is one *direction* (`"10"`, `"28"`) with its own threshold
//! `geoLat`/`geoLong` and true bearing, linked back to its runway by
//! `RdnUid/RwyUid/txtDesig`. Because these identity fields are nested
//! several levels deep — and `Rdn` has two `txtDesig` at different depths
//! (runway vs direction) — they're read with the *path-keyed* reader
//! ([`crate::parser`]), not the flat one used for the simpler features.
use crate::coord::{parse_lat, parse_lon};
use ff_core::{Runway, RunwayEnd, RunwaySurface};
use std::collections::HashMap;

const METERS_TO_FEET: f64 = 3.280_839_895;

type Fields = HashMap<String, String>;

fn get<'a>(f: &'a Fields, key: &str) -> Option<&'a str> {
    f.get(key).map(String::as_str).filter(|s| !s.is_empty())
}

/// One physical runway from an `Rwy` feature.
pub(crate) struct RwyRaw {
    pub airport_icao: String,
    /// Both-ends designator as published, e.g. `"10/28"` or `"09L/27R"`.
    pub designator: String,
    pub length_ft: u32,
    pub width_ft: u32,
    pub surface: RunwaySurface,
}

/// One runway direction from an `Rdn` feature.
pub(crate) struct RdnRaw {
    pub airport_icao: String,
    /// The parent runway's designator (`"10/28"`) — the join key.
    pub rwy_designator: String,
    /// This direction's designator (`"10"`).
    pub dir_designator: String,
    pub lat: f64,
    pub lon: f64,
    pub heading_deg: f64,
    /// False when the source `Rdn` had no `geoLat`/`geoLong` (~14% of real
    /// rows) — the ident/heading are still used, coordinates fall back to 0.
    pub has_position: bool,
}

/// VERIFY: AIXM 4.5 `codeComposition`. Confirmed values in the real SIA
/// export: `GRASS`, `ASPH`, `CONC+ASPH`, `MACADAM`, `CONC`, `BITUM`,
/// `WATER`. Compound codes take their first-listed material (as `ff-nasr`
/// does); `MACADAM`/unknown → `Other`.
fn surface(code: Option<&str>) -> RunwaySurface {
    let Some(code) = code else {
        return RunwaySurface::Other;
    };
    let c = code.to_ascii_uppercase();
    if c.starts_with("ASPH") || c.starts_with("BITUM") || c.starts_with("TARMAC") {
        RunwaySurface::Asphalt
    } else if c.starts_with("CONC") {
        RunwaySurface::Concrete
    } else if c.starts_with("GRAS") {
        RunwaySurface::Turf
    } else if c.starts_with("GRAV") || c.starts_with("GVL") {
        RunwaySurface::Gravel
    } else if c.starts_with("WATER") {
        RunwaySurface::Water
    } else {
        RunwaySurface::Other
    }
}

/// Dimension (length/width) → feet, honoring `uomDimRwy` (all `M` in the
/// real SIA export, but `FT` handled too).
fn dim_ft(f: &Fields, key: &str) -> u32 {
    let val: f64 = get(f, key).and_then(|s| s.parse().ok()).unwrap_or(0.0);
    let ft = match get(f, "uomDimRwy") {
        Some("FT") => val,
        _ => val * METERS_TO_FEET,
    };
    ft.round() as u32
}

pub(crate) fn rwy_raw_from_paths(f: &Fields) -> Option<RwyRaw> {
    Some(RwyRaw {
        airport_icao: get(f, "RwyUid/AhpUid/codeId")?.to_string(),
        designator: get(f, "RwyUid/txtDesig")?.to_string(),
        length_ft: dim_ft(f, "valLen"),
        width_ft: dim_ft(f, "valWid"),
        surface: surface(get(f, "codeComposition")),
    })
}

pub(crate) fn rdn_raw_from_paths(f: &Fields) -> Option<RdnRaw> {
    let lat = get(f, "geoLat").and_then(parse_lat);
    let lon = get(f, "geoLong").and_then(parse_lon);
    Some(RdnRaw {
        airport_icao: get(f, "RdnUid/RwyUid/AhpUid/codeId")?.to_string(),
        rwy_designator: get(f, "RdnUid/RwyUid/txtDesig")?.to_string(),
        dir_designator: get(f, "RdnUid/txtDesig")?.to_string(),
        lat: lat.unwrap_or(0.0),
        lon: lon.unwrap_or(0.0),
        heading_deg: get(f, "valTrueBrg").and_then(|s| s.parse().ok()).unwrap_or(0.0),
        has_position: lat.is_some() && lon.is_some(),
    })
}

fn make_end(dir: Option<&&RdnRaw>, fallback_ident: &str) -> RunwayEnd {
    match dir {
        Some(r) => RunwayEnd {
            ident: r.dir_designator.clone(),
            lat: if r.has_position { r.lat } else { 0.0 },
            lon: if r.has_position { r.lon } else { 0.0 },
            heading_deg: r.heading_deg,
        },
        None => RunwayEnd {
            ident: fallback_ident.to_string(),
            lat: 0.0,
            lon: 0.0,
            heading_deg: 0.0,
        },
    }
}

/// Joins physical runways to their directions. Directions are matched on
/// `(airport_icao, runway designator)`; the designator's two halves
/// (`"10/28"` → `"10"`,`"28"`) fix low/high-end order and provide idents
/// for any missing direction — the same degrade-don't-drop handling
/// `ff-nasr` applies to runway ends without coordinates.
pub(crate) fn assemble_runways(rwys: &[RwyRaw], rdns: &[RdnRaw]) -> Vec<Runway> {
    let mut by_key: HashMap<(&str, &str), Vec<&RdnRaw>> = HashMap::new();
    for r in rdns {
        by_key
            .entry((r.airport_icao.as_str(), r.rwy_designator.as_str()))
            .or_default()
            .push(r);
    }

    rwys.iter()
        .map(|rwy| {
            let dirs = by_key
                .get(&(rwy.airport_icao.as_str(), rwy.designator.as_str()))
                .map(Vec::as_slice)
                .unwrap_or(&[]);

            let mut halves = rwy.designator.split('/');
            let low_desig = halves.next().unwrap_or("").trim();
            let high_desig = halves.next().unwrap_or("").trim();

            let find = |d: &str| dirs.iter().find(|r| r.dir_designator == d);

            Runway {
                airport_icao: rwy.airport_icao.clone(),
                ident: rwy.designator.clone(),
                length_ft: rwy.length_ft,
                width_ft: rwy.width_ft,
                surface: rwy.surface,
                low_end: make_end(find(low_desig), low_desig),
                high_end: make_end(find(high_desig), high_desig),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rwy(icao: &str, desig: &str) -> RwyRaw {
        RwyRaw {
            airport_icao: icao.into(),
            designator: desig.into(),
            length_ft: 8005,
            width_ft: 148,
            surface: RunwaySurface::Concrete,
        }
    }
    fn rdn(icao: &str, rwy_d: &str, dir: &str, lat: f64, lon: f64, hdg: f64, pos: bool) -> RdnRaw {
        RdnRaw {
            airport_icao: icao.into(),
            rwy_designator: rwy_d.into(),
            dir_designator: dir.into(),
            lat,
            lon,
            heading_deg: hdg,
            has_position: pos,
        }
    }

    #[test]
    fn surface_maps_real_composition_codes() {
        assert_eq!(surface(Some("GRASS")), RunwaySurface::Turf);
        assert_eq!(surface(Some("ASPH")), RunwaySurface::Asphalt);
        assert_eq!(surface(Some("BITUM")), RunwaySurface::Asphalt);
        assert_eq!(surface(Some("CONC")), RunwaySurface::Concrete);
        assert_eq!(surface(Some("CONC+ASPH")), RunwaySurface::Concrete); // first material
        assert_eq!(surface(Some("MACADAM")), RunwaySurface::Other);
        assert_eq!(surface(Some("WATER")), RunwaySurface::Water);
        assert_eq!(surface(None), RunwaySurface::Other);
    }

    #[test]
    fn assembles_two_directions_in_low_high_order() {
        let rwys = [rwy("LFRC", "10/28")];
        let rdns = [
            rdn("LFRC", "10/28", "28", 49.648, -1.4536, 280.774, true),
            rdn("LFRC", "10/28", "10", 49.652, -1.4869, 100.749, true),
        ];
        let out = assemble_runways(&rwys, &rdns);
        assert_eq!(out.len(), 1);
        let r = &out[0];
        assert_eq!(r.ident, "10/28");
        // Low end is the first designator half regardless of Rdn order.
        assert_eq!(r.low_end.ident, "10");
        assert_eq!(r.low_end.heading_deg, 100.749);
        assert!((r.low_end.lon - (-1.4869)).abs() < 1e-6);
        assert_eq!(r.high_end.ident, "28");
        assert_eq!(r.high_end.heading_deg, 280.774);
    }

    #[test]
    fn missing_direction_falls_back_to_designator_half() {
        let rwys = [rwy("LFXX", "05/23")];
        let rdns = [rdn("LFXX", "05/23", "05", 40.0, 2.0, 50.0, true)];
        let out = assemble_runways(&rwys, &rdns);
        let r = &out[0];
        assert_eq!(r.low_end.ident, "05");
        assert_eq!(r.high_end.ident, "23"); // no Rdn — ident from designator
        assert_eq!(r.high_end.lat, 0.0);
    }

    #[test]
    fn direction_without_coordinates_keeps_ident_and_heading() {
        let rwys = [rwy("LFYY", "18/36")];
        let rdns = [rdn("LFYY", "18/36", "18", 0.0, 0.0, 180.0, false)];
        let out = assemble_runways(&rwys, &rdns);
        let r = &out[0];
        assert_eq!(r.low_end.ident, "18");
        assert_eq!(r.low_end.heading_deg, 180.0);
        assert_eq!(r.low_end.lat, 0.0); // no position in source
    }
}
