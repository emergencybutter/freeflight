use crate::geo::{distance_nm, initial_bearing_deg};
use crate::wind::{solve as solve_wind_triangle, Wind};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AircraftProfile {
    pub name: String,
    pub cruise_tas_kt: f64,
    pub fuel_burn_gph: f64,
    /// Weight & balance fields (see `crate::weight_balance`) — optional
    /// since a profile with no W&B data can still plan a nav log.
    /// Mirrors `ff-storage`'s `aircraft_profile` table columns of the
    /// same names, which are nullable for the same reason.
    #[serde(default)]
    pub max_gross_weight_lb: Option<f64>,
    #[serde(default)]
    pub forward_cg_limit_in: Option<f64>,
    #[serde(default)]
    pub aft_cg_limit_in: Option<f64>,
}

/// A single point in a route: an airport, navaid, or plain waypoint,
/// identified by its coordinates. Kept intentionally minimal here — the
/// client resolves human-entered idents (airport/navaid/fix identifiers)
/// against the local cycle database before calling into `ff-planning`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RoutePoint {
    pub lat: f64,
    pub lon: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouteLegPlan {
    pub distance_nm: f64,
    pub true_course_deg: f64,
    pub true_heading_deg: f64,
    pub ground_speed_kt: f64,
    pub ete_hours: f64,
    pub fuel_gal: f64,
}

/// Plan a single leg: distance/course from great-circle geometry, then
/// (if wind is known) a heading/groundspeed/time/fuel estimate. Without
/// wind, TAS is assumed to equal groundspeed (DESIGN.md §9.3 — a "simple"
/// nav log, not a certified performance tool).
pub fn plan_leg(
    from: RoutePoint,
    to: RoutePoint,
    profile: &AircraftProfile,
    wind: Option<Wind>,
) -> RouteLegPlan {
    let distance_nm = distance_nm((from.lat, from.lon), (to.lat, to.lon));
    let true_course_deg = initial_bearing_deg((from.lat, from.lon), (to.lat, to.lon));

    let (true_heading_deg, ground_speed_kt) = match wind {
        Some(wind) => {
            let result = solve_wind_triangle(true_course_deg, profile.cruise_tas_kt, wind);
            (result.true_heading_deg, result.ground_speed_kt)
        }
        None => (true_course_deg, profile.cruise_tas_kt),
    };

    let ete_hours = if ground_speed_kt > 0.0 {
        distance_nm / ground_speed_kt
    } else {
        f64::INFINITY
    };
    let fuel_gal = if ete_hours.is_finite() {
        ete_hours * profile.fuel_burn_gph
    } else {
        f64::INFINITY
    };

    RouteLegPlan {
        distance_nm,
        true_course_deg,
        true_heading_deg,
        ground_speed_kt,
        ete_hours,
        fuel_gal,
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoutePlanSummary {
    pub legs: Vec<RouteLegPlan>,
    pub total_distance_nm: f64,
    pub total_ete_hours: f64,
    pub total_fuel_gal: f64,
}

/// Plan every leg of a route (a sequence of points) in order. `winds`, if
/// given, must have one entry per leg (i.e. `points.len() - 1`); pass
/// `None` for a leg with no wind data.
pub fn plan_route(
    points: &[RoutePoint],
    profile: &AircraftProfile,
    winds: Option<&[Option<Wind>]>,
) -> RoutePlanSummary {
    let legs: Vec<RouteLegPlan> = points
        .windows(2)
        .enumerate()
        .map(|(i, pair)| {
            let wind = winds.and_then(|w| w.get(i).copied().flatten());
            plan_leg(pair[0], pair[1], profile, wind)
        })
        .collect();

    let total_distance_nm = legs.iter().map(|l| l.distance_nm).sum();
    let total_ete_hours = legs.iter().map(|l| l.ete_hours).sum();
    let total_fuel_gal = legs.iter().map(|l| l.fuel_gal).sum();

    RoutePlanSummary {
        legs,
        total_distance_nm,
        total_ete_hours,
        total_fuel_gal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cessna_172() -> AircraftProfile {
        AircraftProfile {
            name: "C172".into(),
            cruise_tas_kt: 110.0,
            fuel_burn_gph: 8.5,
            // Same envelope as weight_balance.rs's own tests.
            max_gross_weight_lb: Some(2450.0),
            forward_cg_limit_in: Some(35.0),
            aft_cg_limit_in: Some(47.3),
        }
    }

    #[test]
    fn plans_a_two_leg_route_with_no_wind() {
        let points = [
            RoutePoint {
                lat: 37.6188,
                lon: -122.375,
            }, // KSFO
            RoutePoint {
                lat: 37.363,
                lon: -121.929,
            }, // KSJC-ish
            RoutePoint {
                lat: 36.5844,
                lon: -121.8429,
            }, // KMRY
        ];
        let summary = plan_route(&points, &cessna_172(), None);
        assert_eq!(summary.legs.len(), 2);
        assert!(summary.total_distance_nm > 0.0);
        assert!((summary.total_ete_hours - summary.total_distance_nm / 110.0).abs() < 1e-6);
    }
}
