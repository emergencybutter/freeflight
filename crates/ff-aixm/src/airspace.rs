//! Airspace assembly from AIXM 4.5 `Ase` (airspace) + `Abd` (airspace
//! border) features — the last, and geometrically the hardest, join.
//!
//! `Ase` carries identity/type/class and vertical limits; `Abd` carries
//! the boundary, linked by `AbdUid/AseUid/codeId`. A boundary is either a
//! sequence of `<Avx>` vertices or a single `<Circle>` (confirmed against
//! the real export). Vertices are mostly straight (`GRC`/`RHL`/`FNT`) but
//! ~9% include `CWA`/`CCA` arcs (center + radius); circles give a center +
//! radius. Both are expanded into the point list `ff-core`'s [`Polygon`]
//! wants.
//!
//! Two honest simplifications, both fine under the app's "not for
//! navigation" scope:
//! - **Geometry is planar/equirectangular** (local `cos(lat)` longitude
//!   scaling), not geodesic — good enough to draw, not to navigate.
//! - **Class mapping is lossy.** `ff-core`'s [`AirspaceClass`] is
//!   US-shaped (B/C/D/E/G + MOA/R/P/W/A); European airspace has classes
//!   A–G and many types (TMA/CTR/FIR/…). We map the SUA types and the
//!   B/C/D/E/G-classed control areas, and **skip** what doesn't fit (class
//!   A/F, FIR/UIR/OCA/UTA, ATC sectors) rather than mislabel it.
use crate::coord::{parse_lat, parse_lon};
use ff_core::{AirspaceClass, AirspaceVolume, AltitudeLimit, Polygon, SpecialUseKind};
use std::collections::HashMap;

type Fields = HashMap<String, String>;

fn get<'a>(f: &'a Fields, key: &str) -> Option<&'a str> {
    f.get(key).map(String::as_str).filter(|s| !s.is_empty())
}

const FEET_PER_METER: f64 = 3.280_839_895;
/// Degrees of latitude per nautical mile (1 NM = 1 minute of arc).
const NM_PER_DEG: f64 = 60.0;

pub(crate) struct AseRaw {
    /// The AIXM feature `mid` — unique per airspace and shared with its
    /// `Abd`, so it's used as both the join key and the volume's id.
    /// `codeId` (e.g. `"LFR92"`) is NOT unique — multi-part airspaces reuse
    /// it — so it can't be the primary key.
    pub id: String,
    pub code_type: String,
    pub name: String,
    pub code_class: Option<String>,
    pub upper: (Option<String>, Option<String>, Option<String>), // (code, val, uom)
    pub lower: (Option<String>, Option<String>, Option<String>),
}

/// Raw `Abd` boundary: either a vertex list or a circle (mutually
/// exclusive in the source). Field-maps are resolved into a [`Polygon`] by
/// [`build_polygon`].
pub(crate) struct AbdRaw {
    pub airspace_id: String,
    pub vertices: Vec<Fields>,
    pub circle: Option<Fields>,
}

fn owned(f: &Fields, k: &str) -> Option<String> {
    get(f, k).map(str::to_string)
}

pub(crate) fn ase_from_paths(f: &Fields) -> Option<AseRaw> {
    // Name from the designation (codeId, e.g. "LFR92") — the label pilots
    // use — falling back to txtName; the mid is the id, not the name.
    let designation = get(f, "AseUid/codeId");
    Some(AseRaw {
        id: get(f, "AseUid@mid")?.to_string(),
        code_type: get(f, "AseUid/codeType")?.to_string(),
        name: designation.or_else(|| get(f, "txtName")).unwrap_or("").to_string(),
        code_class: owned(f, "codeClass"),
        upper: (
            owned(f, "codeDistVerUpper"),
            owned(f, "valDistVerUpper"),
            owned(f, "uomDistVerUpper"),
        ),
        lower: (
            owned(f, "codeDistVerLower"),
            owned(f, "valDistVerLower"),
            owned(f, "uomDistVerLower"),
        ),
    })
}

/// Maps AIXM airspace type + ICAO class onto `ff-core`'s [`AirspaceClass`].
/// Returns `None` for airspace this US-shaped enum can't represent
/// (class A/F, FIR/UIR/OCA/UTA, ATC sectors), so the caller skips it — see
/// the module note. VERIFY: the SUA-type buckets (`D`→Warning, `TRA`→
/// Restricted) are best-effort fits.
fn airspace_class(code_type: &str, code_class: Option<&str>) -> Option<AirspaceClass> {
    use SpecialUseKind::*;
    match code_type {
        "R" => Some(AirspaceClass::SpecialUse(Restricted)),
        "P" | "UIR-P" => Some(AirspaceClass::SpecialUse(Prohibited)),
        "D" | "D-OTHER" => Some(AirspaceClass::SpecialUse(Warning)),
        "TRA" => Some(AirspaceClass::SpecialUse(Restricted)),
        "TMA" | "CTR" | "CTA" => match code_class {
            Some("B") => Some(AirspaceClass::B),
            Some("C") => Some(AirspaceClass::C),
            Some("D") => Some(AirspaceClass::D),
            Some("E") => Some(AirspaceClass::E),
            Some("G") => Some(AirspaceClass::G),
            _ => None, // class A/F/unspecified: not representable
        },
        _ => None, // FIR/UIR/OCA/UTA/SECTOR/RAS: not chart airspace here
    }
}

fn to_feet(val: f64, uom: Option<&str>) -> u32 {
    let ft = match uom {
        Some("M") => val * FEET_PER_METER,
        _ => val, // FT or unspecified
    };
    ft.round() as u32
}

/// Vertical limit from an AIXM `codeDistVer`/`valDistVer`/`uomDistVer`
/// triple: `STD`→flight level, `ALT`→MSL, `HEI`→AGL, `SFC`/`UNL` the
/// obvious. Unknown code with a value falls back to MSL.
fn altitude_limit(triple: &(Option<String>, Option<String>, Option<String>)) -> AltitudeLimit {
    let (code, val, uom) = (triple.0.as_deref(), triple.1.as_deref(), triple.2.as_deref());
    match code {
        Some("SFC") => AltitudeLimit::Surface,
        Some("UNL") => AltitudeLimit::Unlimited,
        _ => {
            let v: f64 = val.and_then(|s| s.parse().ok()).unwrap_or(0.0);
            match code {
                Some("STD") => AltitudeLimit::FlightLevel(v.round() as u32),
                Some("HEI") => AltitudeLimit::Agl(to_feet(v, uom)),
                _ => AltitudeLimit::Msl(to_feet(v, uom)), // ALT or unknown
            }
        }
    }
}

fn radius_to_nm(val: &str, uom: Option<&str>) -> Option<f64> {
    let v: f64 = val.parse().ok()?;
    Some(match uom {
        Some("KM") => v / 1.852,
        Some("M") => v / 1852.0,
        _ => v, // NM
    })
}

/// Appends points of a circle (`step`-degree spacing) around `center` at
/// `radius_nm`, using a local equirectangular approximation.
fn circle_points(center: (f64, f64), radius_nm: f64) -> Vec<(f64, f64)> {
    let (lat_c, lon_c) = center;
    let r_deg = radius_nm / NM_PER_DEG;
    let coslat = lat_c.to_radians().cos().max(1e-6);
    (0..64)
        .map(|i| {
            let a = std::f64::consts::TAU * (i as f64) / 64.0;
            (lat_c + r_deg * a.sin(), lon_c + r_deg * a.cos() / coslat)
        })
        .collect()
}

/// Appends interpolated points of an arc from `from` to `to` around
/// `center`, clockwise when `cw`. Endpoint included, start excluded.
fn append_arc(points: &mut Vec<(f64, f64)>, from: (f64, f64), to: (f64, f64), center: (f64, f64), cw: bool) {
    let (lat_c, lon_c) = center;
    let coslat = lat_c.to_radians().cos().max(1e-6);
    // Local planar coordinates relative to the arc center (degrees).
    let proj = |p: (f64, f64)| ((p.1 - lon_c) * coslat, p.0 - lat_c);
    let (x0, y0) = proj(from);
    let (x1, y1) = proj(to);
    let r = (x0.hypot(y0) + x1.hypot(y1)) / 2.0;
    let a0 = y0.atan2(x0);
    let a1 = y1.atan2(x1);

    let tau = std::f64::consts::TAU;
    let mut sweep = a1 - a0;
    if cw {
        while sweep >= 0.0 {
            sweep -= tau;
        }
    } else {
        while sweep <= 0.0 {
            sweep += tau;
        }
    }

    let steps = (sweep.abs() / (std::f64::consts::PI / 18.0)).ceil().max(2.0) as usize;
    for i in 1..=steps {
        let a = a0 + sweep * (i as f64) / (steps as f64);
        let x = r * a.cos();
        let y = r * a.sin();
        points.push((lat_c + y, lon_c + x / coslat));
    }
}

/// Builds a boundary [`Polygon`] from an [`AbdRaw`], expanding arcs and
/// circles. Vertices with unparseable coordinates are skipped; returns
/// `None` if fewer than 3 points result (not a drawable area).
pub(crate) fn build_polygon(abd: &AbdRaw) -> Option<Polygon> {
    if let Some(c) = &abd.circle {
        let lat = parse_lat(get(c, "geoLatCen")?)?;
        let lon = parse_lon(get(c, "geoLongCen")?)?;
        let radius = radius_to_nm(get(c, "valRadius")?, get(c, "uomRadius"))?;
        return Some(Polygon {
            points: circle_points((lat, lon), radius),
        });
    }

    let mut points: Vec<(f64, f64)> = Vec::new();
    let mut prev: Option<(f64, f64)> = None;
    for v in &abd.vertices {
        let (Some(lat), Some(lon)) = (
            get(v, "geoLat").and_then(parse_lat),
            get(v, "geoLong").and_then(parse_lon),
        ) else {
            continue;
        };
        let code_type = get(v, "codeType").unwrap_or("GRC");
        let arc_center = match (
            get(v, "geoLatArc").and_then(parse_lat),
            get(v, "geoLongArc").and_then(parse_lon),
        ) {
            (Some(la), Some(lo)) => Some((la, lo)),
            _ => None,
        };
        match (code_type, prev, arc_center) {
            ("CWA", Some(p), Some(c)) => append_arc(&mut points, p, (lat, lon), c, true),
            ("CCA", Some(p), Some(c)) => append_arc(&mut points, p, (lat, lon), c, false),
            _ => points.push((lat, lon)),
        }
        prev = Some((lat, lon));
    }

    (points.len() >= 3).then_some(Polygon { points })
}

/// Joins airspaces to their borders (by `codeId`), keeping only those with
/// a class `ff-core` can represent and a drawable boundary.
pub(crate) fn assemble_airspaces(ases: &[AseRaw], abds: &[AbdRaw]) -> Vec<AirspaceVolume> {
    let mut border: HashMap<&str, &AbdRaw> = HashMap::new();
    for a in abds {
        border.entry(a.airspace_id.as_str()).or_insert(a);
    }

    ases.iter()
        .filter_map(|ase| {
            let class = airspace_class(&ase.code_type, ase.code_class.as_deref())?;
            // Single-point "activity" zones (many D-OTHER) have no polygon
            // border and `build_polygon` returns None — correctly skipped.
            let boundary = build_polygon(border.get(ase.id.as_str())?)?;
            Some(AirspaceVolume {
                id: ase.id.clone(),
                name: ase.name.clone(),
                class,
                floor: altitude_limit(&ase.lower),
                ceiling: altitude_limit(&ase.upper),
                boundary,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn triple(code: &str, val: &str, uom: &str) -> (Option<String>, Option<String>, Option<String>) {
        (Some(code.into()), Some(val.into()), Some(uom.into()))
    }

    #[test]
    fn class_mapping_covers_sua_and_classed_control_skips_the_rest() {
        use AirspaceClass::*;
        use SpecialUseKind::*;
        assert_eq!(airspace_class("R", None), Some(SpecialUse(Restricted)));
        assert_eq!(airspace_class("P", None), Some(SpecialUse(Prohibited)));
        assert_eq!(airspace_class("D-OTHER", None), Some(SpecialUse(Warning)));
        assert_eq!(airspace_class("TMA", Some("D")), Some(D));
        assert_eq!(airspace_class("CTR", Some("C")), Some(C));
        assert_eq!(airspace_class("TMA", Some("A")), None); // class A not representable
        assert_eq!(airspace_class("FIR", Some("A")), None); // not chart airspace
        assert_eq!(airspace_class("SECTOR", Some("D")), None);
    }

    #[test]
    fn altitude_limits_map_the_aixm_codes() {
        assert_eq!(altitude_limit(&triple("STD", "195", "FL")), AltitudeLimit::FlightLevel(195));
        assert_eq!(altitude_limit(&triple("ALT", "1100", "FT")), AltitudeLimit::Msl(1100));
        assert_eq!(altitude_limit(&triple("HEI", "0", "FT")), AltitudeLimit::Agl(0));
        assert_eq!(altitude_limit(&triple("ALT", "300", "M")), AltitudeLimit::Msl(984)); // 300 m
        assert_eq!(
            altitude_limit(&(Some("SFC".into()), None, None)),
            AltitudeLimit::Surface
        );
    }

    #[test]
    fn straight_polygon_keeps_its_vertices() {
        let v = |lat: &str, lon: &str| {
            let mut f = Fields::new();
            f.insert("codeType".into(), "GRC".into());
            f.insert("geoLat".into(), lat.into());
            f.insert("geoLong".into(), lon.into());
            f
        };
        let abd = AbdRaw {
            airspace_id: "LFR92".into(),
            vertices: vec![
                v("483533.00N", "0054448.00E"),
                v("483140.00N", "0054830.00E"),
                v("482900.00N", "0054530.00E"),
                v("483215.00N", "0054005.00E"),
            ],
            circle: None,
        };
        let poly = build_polygon(&abd).unwrap();
        assert_eq!(poly.points.len(), 4);
        assert!((poly.points[0].0 - (48.0 + 35.0 / 60.0 + 33.0 / 3600.0)).abs() < 1e-6);
    }

    #[test]
    fn circle_expands_to_a_ring_around_its_center() {
        let mut c = Fields::new();
        c.insert("geoLatCen".into(), "483241.00N".into());
        c.insert("geoLongCen".into(), "0023444.00E".into());
        c.insert("valRadius".into(), "1".into());
        c.insert("uomRadius".into(), "NM".into());
        let abd = AbdRaw { airspace_id: "LFP46".into(), vertices: vec![], circle: Some(c) };
        let poly = build_polygon(&abd).unwrap();
        assert_eq!(poly.points.len(), 64);
        // Every ring point is ~1 NM (1/60 deg) from the center.
        let (lat_c, lon_c): (f64, f64) =
            (48.0 + 32.0 / 60.0 + 41.0 / 3600.0, 2.0 + 34.0 / 60.0 + 44.0 / 3600.0);
        let coslat = lat_c.to_radians().cos();
        for (lat, lon) in &poly.points {
            let d = (((lon - lon_c) * coslat).powi(2) + (lat - lat_c).powi(2)).sqrt();
            assert!((d - 1.0 / 60.0).abs() < 1e-4, "point off-radius: {d}");
        }
    }

    #[test]
    fn assemble_skips_unmapped_class_and_missing_geometry() {
        let ase = |id: &str, ct: &str| AseRaw {
            id: id.into(),
            code_type: ct.into(),
            name: "x".into(),
            code_class: None,
            upper: triple("HEI", "1000", "FT"),
            lower: triple("HEI", "0", "FT"),
        };
        let mut vf = Fields::new();
        vf.insert("codeType".into(), "GRC".into());
        vf.insert("geoLat".into(), "480000.00N".into());
        vf.insert("geoLong".into(), "0020000.00E".into());
        let abd = AbdRaw {
            airspace_id: "R1".into(),
            vertices: vec![vf.clone(), {
                let mut f = vf.clone();
                f.insert("geoLat".into(), "481000.00N".into());
                f
            }, {
                let mut f = vf.clone();
                f.insert("geoLong".into(), "0021000.00E".into());
                f
            }],
            circle: None,
        };
        // R1 maps (Restricted) and has geometry → kept; FIR1 class unmapped → skipped;
        // R2 mapped but no border → skipped.
        let out = assemble_airspaces(
            &[ase("R1", "R"), ase("FIR1", "FIR"), ase("R2", "R")],
            &[abd],
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, "R1");
        assert_eq!(out[0].ceiling, AltitudeLimit::Agl(1000));
    }
}
