use crate::geo::{distance_nm, initial_bearing_deg};
use crate::magvar::declination_deg;
use crate::performance::AircraftPerformance;
use crate::wind::{solve as solve_wind_triangle, Wind};
use serde::{Deserialize, Serialize};

/// Normalize a bearing to `[0, 360)` degrees.
fn norm360(deg: f64) -> f64 {
    let d = deg % 360.0;
    if d < 0.0 {
        d + 360.0
    } else {
        d
    }
}

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
    /// Planned cruise altitude (ft MSL). Doesn't affect the horizontal
    /// nav log — it selects which winds-aloft level the client feeds in
    /// per leg, and it's the altitude [`crate::vertical`] climbs to and
    /// descends from.
    #[serde(default)]
    pub cruise_altitude_ft: Option<f64>,
    /// Climb/descent performance for the vertical profile
    /// ([`crate::vertical`]) — all optional, since a profile with none of
    /// it can still fly a nav log. A missing *rate* means no top of
    /// climb/descent can be computed at all; a missing *TAS* falls back
    /// to `cruise_tas_kt`, which is the usual rough VFR approximation for
    /// a descent and an over-estimate for a climb.
    #[serde(default)]
    pub climb_rate_fpm: Option<f64>,
    #[serde(default)]
    pub climb_tas_kt: Option<f64>,
    #[serde(default)]
    pub descent_rate_fpm: Option<f64>,
    #[serde(default)]
    pub descent_tas_kt: Option<f64>,
    /// Per-phase fuel burn, for the phase-aware fuel total
    /// ([`crate::flight`]). Falls back to `fuel_burn_gph` when unset.
    #[serde(default)]
    pub climb_fuel_gph: Option<f64>,
    #[serde(default)]
    pub descent_fuel_gph: Option<f64>,
    /// A fixed allowance in gallons for start, taxi and run-up — not a
    /// rate, since it does not scale with the length of the flight.
    #[serde(default)]
    pub taxi_fuel_gal: Option<f64>,
    #[serde(default)]
    pub fuel_capacity_gal: Option<f64>,
    /// Minutes of cruise-burn reserve required on arrival.
    #[serde(default)]
    pub reserve_minutes: Option<i32>,
    /// Real POH tables (DESIGN.md §9.5.6). When present, these are
    /// preferred over the scalar fields above; the scalars remain the
    /// fallback for any phase the tables don't cover.
    #[serde(default)]
    pub performance: Option<AircraftPerformance>,
    /// Which cruise power setting to plan at, when the table holds more
    /// than one (see [`AircraftPerformance::cruise_at`]).
    #[serde(default)]
    pub cruise_power_setting: Option<String>,
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
    /// Magnetic declination at the leg midpoint, positive East (WMM, see
    /// [`crate::magvar`]).
    pub magnetic_variation_deg: f64,
    /// True course/heading converted to magnetic (= true − variation) —
    /// what the pilot flies off the compass/DG.
    pub magnetic_course_deg: f64,
    pub magnetic_heading_deg: f64,
    pub ground_speed_kt: f64,
    pub ete_hours: f64,
    pub fuel_gal: f64,
}

/// Plan a single leg: distance/course from great-circle geometry, a
/// wind-triangle heading/groundspeed (if wind is known; without it TAS is
/// assumed to equal groundspeed — DESIGN.md §9.3), and magnetic
/// course/heading from the WMM variation at the leg midpoint.
/// `decimal_year` (e.g. 2026.5) dates the magnetic model.
pub fn plan_leg(
    from: RoutePoint,
    to: RoutePoint,
    profile: &AircraftProfile,
    wind: Option<Wind>,
    decimal_year: f64,
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

    // Variation at the leg midpoint (short VFR legs — a single value per
    // leg is plenty). Magnetic = true − variation ("east is least").
    let mid_lat = (from.lat + to.lat) / 2.0;
    let mid_lon = (from.lon + to.lon) / 2.0;
    let magnetic_variation_deg = declination_deg(mid_lat, mid_lon, 0.0, decimal_year);
    let magnetic_course_deg = norm360(true_course_deg - magnetic_variation_deg);
    let magnetic_heading_deg = norm360(true_heading_deg - magnetic_variation_deg);

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
        magnetic_variation_deg,
        magnetic_course_deg,
        magnetic_heading_deg,
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
    decimal_year: f64,
) -> RoutePlanSummary {
    let legs: Vec<RouteLegPlan> = points
        .windows(2)
        .enumerate()
        .map(|(i, pair)| {
            let wind = winds.and_then(|w| w.get(i).copied().flatten());
            plan_leg(pair[0], pair[1], profile, wind, decimal_year)
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
            // No cruise altitude/vertical performance: these tests are
            // about the horizontal nav log. See vertical.rs for a profile
            // that fills them in.
            cruise_altitude_ft: None,
            climb_rate_fpm: None,
            climb_tas_kt: None,
            descent_rate_fpm: None,
            descent_tas_kt: None,
            climb_fuel_gph: None,
            descent_fuel_gph: None,
            taxi_fuel_gal: None,
            fuel_capacity_gal: None,
            reserve_minutes: None,
            performance: None,
            cruise_power_setting: None,
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
        let summary = plan_route(&points, &cessna_172(), None, 2026.5);
        assert_eq!(summary.legs.len(), 2);
        assert!(summary.total_distance_nm > 0.0);
        assert!((summary.total_ete_hours - summary.total_distance_nm / 110.0).abs() < 1e-6);
        // California has ~12-13° east variation, so magnetic course is
        // ~12° less than true (and heading == course with no wind).
        let leg = &summary.legs[0];
        assert!(leg.magnetic_variation_deg > 10.0 && leg.magnetic_variation_deg < 15.0);
        assert!(
            (leg.magnetic_heading_deg
                - super::norm360(leg.true_heading_deg - leg.magnetic_variation_deg))
            .abs()
                < 1e-9
        );
    }

    #[test]
    fn magnetic_course_is_true_minus_east_variation() {
        // A due-north true course in a +10° (east) variation region gives
        // a magnetic course of 350°.
        let leg = plan_leg(
            RoutePoint {
                lat: 34.0,
                lon: -118.0,
            },
            RoutePoint {
                lat: 35.0,
                lon: -118.0,
            }, // due north
            &cessna_172(),
            None,
            2026.5,
        );
        assert!(
            (leg.true_course_deg - 0.0).abs() < 0.5 || (leg.true_course_deg - 360.0).abs() < 0.5
        );
        assert!(
            leg.magnetic_variation_deg > 8.0,
            "expected east var, got {}",
            leg.magnetic_variation_deg
        );
        // MC = 360 - var  ≈ 349-351
        assert!(
            leg.magnetic_course_deg > 347.0 && leg.magnetic_course_deg < 353.0,
            "MC {}",
            leg.magnetic_course_deg
        );
    }
}
