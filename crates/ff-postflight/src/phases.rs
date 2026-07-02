use crate::track::TrackPointWithSpeed;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

/// Heuristic groundspeed thresholds (knots) for phase classification.
/// Deliberately coarse (DESIGN.md §9.4) — good enough to separate
/// ground/taxi/airborne for logbook-style summaries, not a certified
/// flight-data-recorder analysis.
pub const TAXI_SPEED_THRESHOLD_KT: f64 = 5.0;
pub const AIRBORNE_SPEED_THRESHOLD_KT: f64 = 40.0;

/// A touch-and-go's ground segment is expected to be brief; anything
/// longer than this is treated as a full-stop landing followed by a
/// separate later departure.
pub const TOUCH_AND_GO_MAX_GROUND_SECONDS: i64 = 90;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PhaseKind {
    Ground,
    Taxi,
    Airborne,
}

fn classify(ground_speed_kt: f64) -> PhaseKind {
    if ground_speed_kt < TAXI_SPEED_THRESHOLD_KT {
        PhaseKind::Ground
    } else if ground_speed_kt < AIRBORNE_SPEED_THRESHOLD_KT {
        PhaseKind::Taxi
    } else {
        PhaseKind::Airborne
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PhaseSegment {
    pub kind: PhaseKind,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

impl PhaseSegment {
    pub fn duration(&self) -> Duration {
        self.end - self.start
    }
}

/// Collapse a per-point phase classification into contiguous segments.
pub fn segment_phases(points: &[TrackPointWithSpeed]) -> Vec<PhaseSegment> {
    let mut segments: Vec<PhaseSegment> = Vec::new();
    for p in points {
        let kind = classify(p.ground_speed_kt);
        let ts = p.point.ts;
        match segments.last_mut() {
            Some(seg) if seg.kind == kind => seg.end = ts,
            _ => segments.push(PhaseSegment {
                kind,
                start: ts,
                end: ts,
            }),
        }
    }
    segments
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LandingKind {
    TouchAndGo,
    FullStop,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FlightSummary {
    pub total_time: Duration,
    pub taxi_time: Duration,
    pub airborne_time: Duration,
    pub landings: Vec<LandingKind>,
}

/// Summarize a segmented track: total/taxi/airborne time and a landing
/// count, classifying each Airborne→non-Airborne transition as a
/// touch-and-go or full stop by how long the aircraft then stayed on the
/// ground before the next Airborne segment (or track end).
pub fn summarize(segments: &[PhaseSegment]) -> FlightSummary {
    let mut taxi_time = Duration::zero();
    let mut airborne_time = Duration::zero();
    for seg in segments {
        match seg.kind {
            PhaseKind::Taxi => taxi_time += seg.duration(),
            PhaseKind::Airborne => airborne_time += seg.duration(),
            PhaseKind::Ground => {}
        }
    }

    let mut landings = Vec::new();
    for (i, seg) in segments.iter().enumerate() {
        let is_landing_transition = seg.kind == PhaseKind::Airborne
            && segments
                .get(i + 1)
                .is_some_and(|next| next.kind != PhaseKind::Airborne);
        if !is_landing_transition {
            continue;
        }
        // Ground time until the next Airborne segment (may span more
        // than one non-airborne segment, e.g. Ground then Taxi).
        let mut ground_duration = Duration::zero();
        let mut j = i + 1;
        while let Some(next) = segments.get(j) {
            if next.kind == PhaseKind::Airborne {
                break;
            }
            ground_duration += next.duration();
            j += 1;
        }
        let kind = if ground_duration <= Duration::seconds(TOUCH_AND_GO_MAX_GROUND_SECONDS)
            && j < segments.len()
        {
            LandingKind::TouchAndGo
        } else {
            LandingKind::FullStop
        };
        landings.push(kind);
    }

    let total_time = match (segments.first(), segments.last()) {
        (Some(first), Some(last)) => last.end - first.start,
        _ => Duration::zero(),
    };

    FlightSummary {
        total_time,
        taxi_time,
        airborne_time,
        landings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn t(minute: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(1_700_000_000 + minute * 60, 0).unwrap()
    }

    fn seg(kind: PhaseKind, start_min: i64, end_min: i64) -> PhaseSegment {
        PhaseSegment {
            kind,
            start: t(start_min),
            end: t(end_min),
        }
    }

    #[test]
    fn counts_a_touch_and_go_then_a_full_stop() {
        let segments = vec![
            seg(PhaseKind::Ground, 0, 2), // taxi out (classified Ground here for brevity)
            seg(PhaseKind::Taxi, 2, 5),
            seg(PhaseKind::Airborne, 5, 20),  // first circuit
            seg(PhaseKind::Ground, 20, 21),   // touch and go, brief
            seg(PhaseKind::Airborne, 21, 40), // second circuit
            seg(PhaseKind::Taxi, 40, 45),     // full stop + taxi in
            seg(PhaseKind::Ground, 45, 46),
        ];
        let summary = summarize(&segments);
        assert_eq!(
            summary.landings,
            vec![LandingKind::TouchAndGo, LandingKind::FullStop]
        );
        assert_eq!(summary.airborne_time, Duration::minutes(15 + 19));
    }

    #[test]
    fn classifies_ground_taxi_and_airborne_by_speed() {
        use crate::track::{TrackPoint, TrackPointWithSpeed};
        let mk = |min: i64, gs: f64| TrackPointWithSpeed {
            point: TrackPoint {
                ts: t(min),
                lat: 0.0,
                lon: 0.0,
                alt_ft: 0.0,
            },
            ground_speed_kt: gs,
        };
        let points = vec![
            mk(0, 0.0),
            mk(1, 3.0),
            mk(2, 20.0),
            mk(3, 90.0),
            mk(4, 95.0),
        ];
        let segments = segment_phases(&points);
        assert_eq!(segments.len(), 3);
        assert_eq!(segments[0].kind, PhaseKind::Ground);
        assert_eq!(segments[1].kind, PhaseKind::Taxi);
        assert_eq!(segments[2].kind, PhaseKind::Airborne);
    }
}
