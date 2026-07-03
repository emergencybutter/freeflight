//! Fetches real Class B/C/D and Special Use Airspace (MOA, Restricted,
//! Prohibited, Warning, Alert) boundary polygons from FAA's public
//! ArcGIS Hub feature services.
//!
//! This is a different source from the 28-day NASR CSV subscription
//! (`fetch_nasr`): NASR's `CLS_ARSP.csv` only carries a per-airport
//! Class B/C/D/E *flag* (does this airport have one, yes/no), not
//! boundary geometry — confirmed by downloading a real subscription and
//! inspecting it. The actual polygons live in two separate FAA-published
//! ArcGIS FeatureServers (confirmed live, schema inspected against real
//! data): `Class_Airspace` and `Special_Use_Airspace`, both under
//! `services6.arcgis.com/ssFJjBXIUyZDrSYZ/arcgis/rest/services/`.
use ff_core::{AirspaceClass, AirspaceVolume, AltitudeLimit, Polygon, SpecialUseKind};
use serde::Deserialize;
use std::time::Duration;
use thiserror::Error;

/// Confirmed live: these queries can take much longer than a typical
/// FAA download to actually compute server-side (dense shelf-boundary
/// geometry across hundreds of rows), and `crate::fetch::http_client()`
/// sets no per-request timeout of its own — so requests here set one
/// explicitly, generous enough to ride out a slow response rather than
/// tripping a shorter default somewhere in the network path.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

const CLASS_AIRSPACE_URL: &str =
    "https://services6.arcgis.com/ssFJjBXIUyZDrSYZ/arcgis/rest/services/Class_Airspace/FeatureServer/0/query";
const SPECIAL_USE_AIRSPACE_URL: &str = "https://services6.arcgis.com/ssFJjBXIUyZDrSYZ/arcgis/rest/services/Special_Use_Airspace/FeatureServer/0/query";

/// Well under both services' actual `maxRecordCount` (2000, confirmed
/// live) — driven by response *size*, not row count: these shelf/sector
/// polygons run to hundreds of vertices each, and confirmed live that a
/// 1000-row Class D page is ~60-100MB of GeoJSON, which reliably stalls
/// partway through in this sandbox's environment (a hard ~32MiB
/// buffering threshold, consistently reproduced) even with a generous
/// client-side timeout. A 100-row page stays in the single-digit-MB
/// range and completes in seconds.
const PAGE_SIZE: u32 = 100;

#[derive(Debug, Error)]
pub enum AirspaceError {
    #[error("http request failed: {0}")]
    Request(#[from] reqwest::Error),
}

#[derive(Debug, Deserialize)]
struct FeatureCollection<P> {
    features: Vec<Feature<P>>,
}

#[derive(Debug, Deserialize)]
struct Feature<P> {
    geometry: Geometry,
    properties: P,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", content = "coordinates")]
enum Geometry {
    Polygon(Vec<Vec<(f64, f64)>>),
    MultiPolygon(Vec<Vec<Vec<(f64, f64)>>>),
}

/// The exterior ring only, as `ff-core`'s `(lat, lon)`-ordered `Polygon`
/// (GeoJSON coordinates are `(lon, lat)`, so this swaps on the way in).
/// A `MultiPolygon` takes just its first part. Both simplifications are
/// rare in practice (checked against live data: 2 of 1533 Special Use
/// Airspace features have an interior ring/hole — e.g. an excluded
/// sub-area — and none of the 1286 Class B/C/D features or SUA features
/// are `MultiPolygon`), and this app doesn't claim to be a source of
/// legal-minimum-safe-altitude truth (DESIGN.md §3's "not for
/// navigation" scope already covers that).
fn exterior_ring(geometry: &Geometry) -> Polygon {
    let first_ring = match geometry {
        Geometry::Polygon(rings) => rings.first(),
        Geometry::MultiPolygon(parts) => parts.first().and_then(|rings| rings.first()),
    };
    Polygon {
        points: first_ring
            .map(|ring| ring.iter().map(|&(lon, lat)| (lat, lon)).collect())
            .unwrap_or_default(),
    }
}

/// Shared altitude-code handling for both layers: `"SFC"` -> surface,
/// `"UNLTD"` -> unlimited (the accompanying value is a `-9998` sentinel,
/// not a real number, so it's ignored), `"STD"` -> a flight level (the
/// value is already the FL number, e.g. `180` for FL180, not raw feet),
/// `"MSL"`/missing -> MSL feet, `"AGL"` -> AGL feet. All confirmed
/// against live data for both `Class_Airspace` and `Special_Use_Airspace`.
fn altitude_limit(val: Option<f64>, code: Option<&str>) -> AltitudeLimit {
    match code {
        Some("SFC") => AltitudeLimit::Surface,
        Some("UNLTD") => AltitudeLimit::Unlimited,
        Some("STD") => AltitudeLimit::FlightLevel(val.unwrap_or(0.0).round() as u32),
        Some("AGL") => AltitudeLimit::Agl(val.unwrap_or(0.0).round() as u32),
        Some("MSL") | None => AltitudeLimit::Msl(val.unwrap_or(0.0).round() as u32),
        Some(other) => {
            tracing::warn!(
                code = other,
                "unrecognized airspace altitude code, treating as MSL"
            );
            AltitudeLimit::Msl(val.unwrap_or(0.0).round() as u32)
        }
    }
}

/// Runs one FeatureServer query, paginating with `resultOffset` until a
/// page comes back shorter than `PAGE_SIZE` (i.e. the last page).
/// `out_fields` is an explicit column list rather than `*` — confirmed
/// live that `Class_Airspace`'s `*` response (~40 mostly-unused columns
/// per row, on top of dense shelf-boundary geometry) is heavy enough to
/// trip this sandbox's egress-proxy timeout; requesting only the
/// handful of fields actually parsed avoids that.
fn fetch_all_pages<P: for<'de> Deserialize<'de>>(
    url: &str,
    where_clause: &str,
    out_fields: &str,
) -> Result<Vec<Feature<P>>, AirspaceError> {
    let client = crate::fetch::http_client();
    let mut features = Vec::new();
    let mut offset = 0u32;
    loop {
        let page: FeatureCollection<P> = client
            .get(url)
            .query(&[
                ("where", where_clause),
                ("outFields", out_fields),
                ("f", "geojson"),
                // Full coordinate precision (≈1cm) is meaningless for
                // chart display and roughly triples response size for
                // no benefit — 6 decimal places (≈11cm) is already far
                // finer than these boundaries are drawn to.
                ("geometryPrecision", "6"),
                ("resultRecordCount", &PAGE_SIZE.to_string()),
                ("resultOffset", &offset.to_string()),
            ])
            .timeout(REQUEST_TIMEOUT)
            .send()?
            .error_for_status()?
            .json()?;
        let page_len = page.features.len() as u32;
        tracing::debug!(url, offset, page_len, "fetched airspace page");
        features.extend(page.features);
        if page_len < PAGE_SIZE {
            break;
        }
        offset += PAGE_SIZE;
    }
    Ok(features)
}

#[derive(Debug, Deserialize)]
struct ClassAirspaceProps {
    #[serde(rename = "GLOBAL_ID")]
    global_id: String,
    #[serde(rename = "NAME")]
    name: String,
    #[serde(rename = "CLASS")]
    class: String,
    #[serde(rename = "UPPER_VAL")]
    upper_val: Option<f64>,
    #[serde(rename = "UPPER_CODE")]
    upper_code: Option<String>,
    #[serde(rename = "LOWER_VAL")]
    lower_val: Option<f64>,
    #[serde(rename = "LOWER_CODE")]
    lower_code: Option<String>,
}

/// Fetches every current US Class B/C/D airspace boundary (each shelf/
/// sector is its own row+polygon — a busy Class B has dozens). Filtered
/// server-side to `TYPE_CODE='CLASS'` (excluding this same service's
/// `CTR`/`TMA`/`TMA-P` rows, which are non-US ICAO-equivalent airspace
/// sharing the same feature layer — confirmed live) and one class at a
/// time rather than one combined `CLASS IN ('B','C','D')` query:
/// confirmed live that the combined query (1286 rows) reliably times out
/// against this environment's egress proxy/the server's own query time,
/// while each single-class query (as few as 340 rows) reliably succeeds
/// — three smaller, faster requests instead of one slow one.
pub fn fetch_class_airspace() -> Result<Vec<AirspaceVolume>, AirspaceError> {
    let mut volumes = Vec::new();
    for (letter, class) in [
        ("B", AirspaceClass::B),
        ("C", AirspaceClass::C),
        ("D", AirspaceClass::D),
    ] {
        let features = fetch_all_pages::<ClassAirspaceProps>(
            CLASS_AIRSPACE_URL,
            &format!("CLASS='{letter}' AND TYPE_CODE='CLASS'"),
            "GLOBAL_ID,NAME,CLASS,UPPER_VAL,UPPER_CODE,LOWER_VAL,LOWER_CODE",
        )?;
        volumes.extend(features.into_iter().map(|f| {
            debug_assert_eq!(
                f.properties.class, letter,
                "queried CLASS='{letter}' but a row came back tagged differently"
            );
            AirspaceVolume {
                id: f.properties.global_id,
                name: f.properties.name,
                class,
                floor: altitude_limit(f.properties.lower_val, f.properties.lower_code.as_deref()),
                ceiling: altitude_limit(f.properties.upper_val, f.properties.upper_code.as_deref()),
                boundary: exterior_ring(&f.geometry),
            }
        }));
    }
    Ok(volumes)
}

#[derive(Debug, Deserialize)]
struct SpecialUseAirspaceProps {
    #[serde(rename = "GLOBAL_ID")]
    global_id: String,
    #[serde(rename = "NAME")]
    name: String,
    #[serde(rename = "TYPE_CODE")]
    type_code: String,
    #[serde(rename = "UPPER_VAL")]
    upper_val: Option<String>,
    #[serde(rename = "UPPER_CODE")]
    upper_code: Option<String>,
    #[serde(rename = "LOWER_VAL")]
    lower_val: Option<String>,
    #[serde(rename = "LOWER_CODE")]
    lower_code: Option<String>,
}

/// Fetches every current US Special Use Airspace area (MOA, Restricted,
/// Prohibited, Warning, Alert — `TYPE_CODE` values confirmed live: `MOA`,
/// `R`, `P`, `W`, `A`). Unlike `Class_Airspace`, every row in this layer
/// is already US-only (confirmed: 1533/1533 sampled have
/// `COUNTRY='UNITED STATES'`), so no server-side country filter is
/// needed. Altitude values arrive as strings here (unlike
/// `Class_Airspace`'s numeric fields) — parsed defensively since the
/// `UNLTD` sentinel (`"-9998"`) isn't a real number.
pub fn fetch_special_use_airspace() -> Result<Vec<AirspaceVolume>, AirspaceError> {
    let features = fetch_all_pages::<SpecialUseAirspaceProps>(
        SPECIAL_USE_AIRSPACE_URL,
        "1=1",
        "GLOBAL_ID,NAME,TYPE_CODE,UPPER_VAL,UPPER_CODE,LOWER_VAL,LOWER_CODE",
    )?;
    Ok(features
        .into_iter()
        .filter_map(|f| {
            let kind = match f.properties.type_code.as_str() {
                "MOA" => SpecialUseKind::Moa,
                "R" => SpecialUseKind::Restricted,
                "P" => SpecialUseKind::Prohibited,
                "W" => SpecialUseKind::Warning,
                "A" => SpecialUseKind::Alert,
                other => {
                    tracing::warn!(type_code = other, name = %f.properties.name, "unrecognized special-use airspace type, skipping");
                    return None;
                }
            };
            let upper_val = f.properties.upper_val.as_deref().and_then(|s| s.parse().ok());
            let lower_val = f.properties.lower_val.as_deref().and_then(|s| s.parse().ok());
            Some(AirspaceVolume {
                id: f.properties.global_id,
                name: f.properties.name,
                class: AirspaceClass::SpecialUse(kind),
                floor: altitude_limit(lower_val, f.properties.lower_code.as_deref()),
                ceiling: altitude_limit(upper_val, f.properties.upper_code.as_deref()),
                boundary: exterior_ring(&f.geometry),
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_altitude_codes() {
        assert_eq!(
            altitude_limit(Some(0.0), Some("SFC")),
            AltitudeLimit::Surface
        );
        assert_eq!(
            altitude_limit(Some(-9998.0), Some("UNLTD")),
            AltitudeLimit::Unlimited
        );
        assert_eq!(
            altitude_limit(Some(180.0), Some("STD")),
            AltitudeLimit::FlightLevel(180)
        );
        assert_eq!(
            altitude_limit(Some(3000.0), Some("AGL")),
            AltitudeLimit::Agl(3000)
        );
        assert_eq!(
            altitude_limit(Some(7000.0), Some("MSL")),
            AltitudeLimit::Msl(7000)
        );
        assert_eq!(
            altitude_limit(Some(10000.0), None),
            AltitudeLimit::Msl(10000)
        );
    }

    #[test]
    fn takes_exterior_ring_and_swaps_lon_lat_to_lat_lon() {
        let geometry = Geometry::Polygon(vec![vec![(-122.4, 37.6), (-122.3, 37.7)]]);
        let polygon = exterior_ring(&geometry);
        assert_eq!(polygon.points, vec![(37.6, -122.4), (37.7, -122.3)]);
    }

    #[test]
    fn takes_first_part_of_a_multipolygon() {
        let geometry = Geometry::MultiPolygon(vec![
            vec![vec![(-122.4, 37.6), (-122.3, 37.7)]],
            vec![vec![(-100.0, 30.0), (-100.1, 30.1)]],
        ]);
        let polygon = exterior_ring(&geometry);
        assert_eq!(polygon.points, vec![(37.6, -122.4), (37.7, -122.3)]);
    }

    #[test]
    fn parses_a_real_class_b_geojson_feature() {
        // Excerpt shape confirmed against a live Class_Airspace query
        // response for Boston Class B.
        let json = r#"{
            "features": [{
                "geometry": {"type": "Polygon", "coordinates": [[[-71.1, 42.3], [-71.0, 42.4], [-71.05, 42.35]]]},
                "properties": {
                    "GLOBAL_ID": "1CB17E3C-6B1D-49DA-B225-188E819FC3A3",
                    "NAME": "BOSTON CLASS B",
                    "CLASS": "B",
                    "UPPER_VAL": 7000.0,
                    "UPPER_UOM": "FT",
                    "UPPER_CODE": "MSL",
                    "LOWER_VAL": 0.0,
                    "LOWER_UOM": "FT",
                    "LOWER_CODE": "SFC"
                }
            }]
        }"#;
        let parsed: FeatureCollection<ClassAirspaceProps> = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.features[0].properties.name, "BOSTON CLASS B");
        assert_eq!(parsed.features[0].properties.class, "B");
    }

    #[test]
    fn parses_a_real_sua_geojson_feature_with_string_altitude_fields() {
        // Excerpt shape confirmed against a live Special_Use_Airspace
        // query response for a Restricted area with an unlimited top.
        let json = r#"{
            "features": [{
                "geometry": {"type": "Polygon", "coordinates": [[[-115.8, 36.0], [-115.7, 36.1], [-115.75, 36.05]]]},
                "properties": {
                    "GLOBAL_ID": "80512E92-87CB-4E6B-8359-B94B44F07F30",
                    "NAME": "R-2202D",
                    "TYPE_CODE": "R",
                    "UPPER_VAL": "-9998",
                    "UPPER_UOM": null,
                    "UPPER_CODE": "UNLTD",
                    "LOWER_VAL": "310",
                    "LOWER_UOM": "FL",
                    "LOWER_CODE": "STD"
                }
            }]
        }"#;
        let parsed: FeatureCollection<SpecialUseAirspaceProps> =
            serde_json::from_str(json).unwrap();
        assert_eq!(parsed.features[0].properties.name, "R-2202D");
        assert_eq!(parsed.features[0].properties.type_code, "R");
    }
}
