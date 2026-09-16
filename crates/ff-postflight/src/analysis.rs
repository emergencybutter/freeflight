use crate::phases::{segment_phases, summarize, LandingKind, PhaseSegment};
use crate::track::{derive_ground_speed, TrackPoint, TrackPointWithSpeed};
use serde::{Deserialize, Serialize};

/// High-level result of analyzing a GPS track for flight logging and post-flight review.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnalyzedTrack {
    pub total_time_seconds: i64,
    pub taxi_time_seconds: i64,
    pub airborne_time_seconds: i64,
    pub landings: Vec<LandingKind>,
    pub touch_and_go_count: usize,
    pub full_stop_count: usize,
    pub max_altitude_ft: f64,
    pub max_ground_speed_kt: f64,
    pub distance_flown_nm: f64,
    pub segments: Vec<PhaseSegment>,
    pub points_with_speed: Vec<TrackPointWithSpeed>,
}

/// Analyze a sequence of GPS track points, deriving speed, phase segments,
/// duration breakdown, landing counts, and flight envelope extremes.
pub fn analyze_track(points: &[TrackPoint]) -> AnalyzedTrack {
    let points_with_speed = derive_ground_speed(points);
    let segments = segment_phases(&points_with_speed);
    let summary = summarize(&segments);

    let mut max_alt: f64 = 0.0;
    let mut max_speed: f64 = 0.0;
    let mut total_distance_nm: f64 = 0.0;

    for (i, p) in points_with_speed.iter().enumerate() {
        if p.point.alt_ft > max_alt {
            max_alt = p.point.alt_ft;
        }
        if p.ground_speed_kt > max_speed {
            max_speed = p.ground_speed_kt;
        }
        if i > 0 {
            let prev = &points_with_speed[i - 1].point;
            total_distance_nm += ff_planning::distance_nm(
                (prev.lat, prev.lon),
                (p.point.lat, p.point.lon),
            );
        }
    }

    let touch_and_go_count = summary
        .landings
        .iter()
        .filter(|&&k| k == LandingKind::TouchAndGo)
        .count();
    let full_stop_count = summary
        .landings
        .iter()
        .filter(|&&k| k == LandingKind::FullStop)
        .count();

    AnalyzedTrack {
        total_time_seconds: summary.total_time.num_seconds(),
        taxi_time_seconds: summary.taxi_time.num_seconds(),
        airborne_time_seconds: summary.airborne_time.num_seconds(),
        landings: summary.landings,
        touch_and_go_count,
        full_stop_count,
        max_altitude_ft: max_alt,
        max_ground_speed_kt: max_speed,
        distance_flown_nm: total_distance_nm,
        segments,
        points_with_speed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    #[test]
    fn analyzes_complete_flight() {
        let t = |sec: i64| Utc.timestamp_opt(1_700_000_000 + sec, 0).unwrap();
        let points = vec![
            TrackPoint {
                ts: t(0),
                lat: 45.0,
                lon: -73.0,
                alt_ft: 150.0,
            },
            // taxi (30s, ~10kt)
            TrackPoint {
                ts: t(30),
                lat: 45.001,
                lon: -73.001,
                alt_ft: 150.0,
            },
            // takeoff & climb (120s, ~90kt, 2500ft)
            TrackPoint {
                ts: t(150),
                lat: 45.04,
                lon: -73.04,
                alt_ft: 2500.0,
            },
            // landing (touch and go, brief ground contact)
            TrackPoint {
                ts: t(300),
                lat: 45.0,
                lon: -73.0,
                alt_ft: 150.0,
            },
            // climb again
            TrackPoint {
                ts: t(450),
                lat: 45.04,
                lon: -73.04,
                alt_ft: 3000.0,
            },
            // full stop
            TrackPoint {
                ts: t(600),
                lat: 45.0,
                lon: -73.0,
                alt_ft: 150.0,
            },
        ];

        let analyzed = analyze_track(&points);
        assert!(analyzed.total_time_seconds > 0);
        assert!(analyzed.max_altitude_ft >= 3000.0);
        assert!(analyzed.max_ground_speed_kt > 40.0);
        assert!(analyzed.distance_flown_nm > 0.0);
    }
}
