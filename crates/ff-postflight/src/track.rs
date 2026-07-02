use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TrackPoint {
    pub ts: DateTime<Utc>,
    pub lat: f64,
    pub lon: f64,
    pub alt_ft: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FlightTrack {
    pub points: Vec<TrackPoint>,
}

/// One point of `points` plus its derived groundspeed from the previous
/// point (nautical miles/hour). The first point has `ground_speed_kt = 0`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TrackPointWithSpeed {
    pub point: TrackPoint,
    pub ground_speed_kt: f64,
}

/// Derive groundspeed between consecutive points from position + time,
/// since most raw GPS logs (NMEA, Android FusedLocationProvider, browser
/// Geolocation) don't reliably report speed directly.
pub fn derive_ground_speed(points: &[TrackPoint]) -> Vec<TrackPointWithSpeed> {
    let mut out = Vec::with_capacity(points.len());
    for (i, point) in points.iter().enumerate() {
        let ground_speed_kt = if i == 0 {
            0.0
        } else {
            let prev = &points[i - 1];
            let dist_nm = ff_planning::distance_nm((prev.lat, prev.lon), (point.lat, point.lon));
            let dt_hours = (point.ts - prev.ts).num_milliseconds() as f64 / 3_600_000.0;
            if dt_hours > 0.0 {
                dist_nm / dt_hours
            } else {
                0.0
            }
        };
        out.push(TrackPointWithSpeed {
            point: *point,
            ground_speed_kt,
        });
    }
    out
}
