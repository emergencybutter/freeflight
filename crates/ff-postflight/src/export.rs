use crate::analysis::AnalyzedTrack;
use crate::track::TrackPoint;

/// Export a series of track points to standard GPX 1.1 format.
pub fn export_gpx(points: &[TrackPoint], flight_name: &str) -> String {
    let mut out = String::with_capacity(points.len() * 128 + 256);
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str("<gpx version=\"1.1\" creator=\"freeflight\" xmlns=\"http://www.topografix.com/GPX/1/1\">\n");
    out.push_str("  <metadata>\n");
    out.push_str(&format!("    <name>{}</name>\n", escape_xml(flight_name)));
    if let Some(first) = points.first() {
        out.push_str(&format!("    <time>{}</time>\n", first.ts.to_rfc3339()));
    }
    out.push_str("  </metadata>\n");
    out.push_str("  <trk>\n");
    out.push_str(&format!("    <name>{}</name>\n", escape_xml(flight_name)));
    out.push_str("    <trkseg>\n");

    for p in points {
        // Altitude converted to meters for GPX elevation (<ele>)
        let ele_meters = p.alt_ft * 0.3048;
        out.push_str(&format!(
            "      <trkpt lat=\"{:.6}\" lon=\"{:.6}\">\n        <ele>{:.2}</ele>\n        <time>{}</time>\n      </trkpt>\n",
            p.lat,
            p.lon,
            ele_meters,
            p.ts.to_rfc3339()
        ));
    }

    out.push_str("    </trkseg>\n");
    out.push_str("  </trk>\n");
    out.push_str("</gpx>\n");
    out
}

/// Export flight summary and detailed track points to CSV format.
pub fn export_csv(analyzed: &AnalyzedTrack, flight_name: &str) -> String {
    let mut out = String::new();
    out.push_str("# freeflight Flight Log Export\n");
    out.push_str(&format!("# Flight: {}\n", flight_name));
    out.push_str(&format!("# Total Time (sec): {}\n", analyzed.total_time_seconds));
    out.push_str(&format!("# Airborne Time (sec): {}\n", analyzed.airborne_time_seconds));
    out.push_str(&format!("# Taxi Time (sec): {}\n", analyzed.taxi_time_seconds));
    out.push_str(&format!("# Touch & Go Landings: {}\n", analyzed.touch_and_go_count));
    out.push_str(&format!("# Full Stop Landings: {}\n", analyzed.full_stop_count));
    out.push_str(&format!("# Max Altitude (ft): {:.0}\n", analyzed.max_altitude_ft));
    out.push_str(&format!("# Max Groundspeed (kt): {:.1}\n", analyzed.max_ground_speed_kt));
    out.push_str(&format!("# Distance (nm): {:.1}\n\n", analyzed.distance_flown_nm));

    out.push_str("timestamp_utc,latitude,longitude,altitude_ft,ground_speed_kt\n");
    for p in &analyzed.points_with_speed {
        out.push_str(&format!(
            "{},{:.6},{:.6},{:.1},{:.1}\n",
            p.point.ts.to_rfc3339(),
            p.point.lat,
            p.point.lon,
            p.point.alt_ft,
            p.ground_speed_kt
        ));
    }

    out
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    #[test]
    fn exports_valid_gpx() {
        let t = |sec: i64| Utc.timestamp_opt(1_700_000_000 + sec, 0).unwrap();
        let points = vec![
            TrackPoint {
                ts: t(0),
                lat: 45.123456,
                lon: -73.654321,
                alt_ft: 1000.0,
            },
            TrackPoint {
                ts: t(10),
                lat: 45.133456,
                lon: -73.664321,
                alt_ft: 1200.0,
            },
        ];

        let gpx = export_gpx(&points, "Test Flight <VFR>");
        assert!(gpx.contains("<?xml"));
        assert!(gpx.contains("<gpx"));
        assert!(gpx.contains("Test Flight &lt;VFR&gt;"));
        assert!(gpx.contains("lat=\"45.123456\""));
        assert!(gpx.contains("lon=\"-73.654321\""));
        assert!(gpx.contains("<ele>304.80</ele>"));
    }

    #[test]
    fn exports_valid_csv() {
        let t = |sec: i64| Utc.timestamp_opt(1_700_000_000 + sec, 0).unwrap();
        let points = vec![
            TrackPoint {
                ts: t(0),
                lat: 45.0,
                lon: -73.0,
                alt_ft: 100.0,
            },
            TrackPoint {
                ts: t(60),
                lat: 45.1,
                lon: -73.0,
                alt_ft: 2000.0,
            },
        ];
        let analyzed = crate::analysis::analyze_track(&points);
        let csv = export_csv(&analyzed, "Test Flight");
        assert!(csv.contains("# Flight: Test Flight"));
        assert!(csv.contains("timestamp_utc,latitude,longitude,altitude_ft,ground_speed_kt"));
        assert!(csv.contains("45.000000,-73.000000"));
    }
}
