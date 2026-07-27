//! Vertical profile for a planned route: top of climb and top of descent
//! (DESIGN.md §9.3).
//!
//! The horizontal nav log ([`crate::route`]) flies the whole route at
//! cruise TAS. That's the standard simplification, but it leaves the two
//! points a VFR pilot actually wants marked on the chart undefined: where
//! the climb levels off, and where to start down to arrive at pattern
//! altitude without a dive. This module adds them, using the same
//! per-leg wind triangle the nav log does — climb and descent are flown
//! at their own TAS, so they get their own groundspeeds and therefore
//! their own ground distances.
//!
//! Both points are computed against real field elevations, walking the
//! route leg by leg rather than assuming one average groundspeed, and
//! come back as positions (interpolated along the great circle) as well
//! as distances, so the client can draw them.
//!
//! On a short hop the climb and descent can overlap — the requested
//! cruise altitude simply doesn't fit. Rather than report a top of
//! descent *before* the top of climb, this finds where the two profiles
//! cross and reports that single point with the altitude actually
//! achievable there (`cruise_reached: false`).

use crate::geo::{distance_nm, initial_bearing_deg, intermediate_point};
use crate::route::{AircraftProfile, RoutePoint};
use crate::wind::{solve as solve_wind_triangle, Wind};
use serde::{Deserialize, Serialize};

/// One leg reduced to what the vertical profile needs: how far it is, and
/// how fast the aircraft covers it in the phase of flight being modelled
/// (climb and descent have different groundspeeds over the same leg).
#[derive(Debug, Clone, Copy)]
struct VerticalLeg {
    distance_nm: f64,
    ground_speed_kt: f64,
}

/// Feet gained (or lost) per nautical mile at `rate_fpm` while making good
/// `ground_speed_kt`. Infinite when the aircraft isn't advancing at all —
/// wind at or above TAS, which the wind triangle clamps to a zero or
/// negative groundspeed — since it's then changing altitude without
/// covering any ground.
fn gradient_ft_per_nm(rate_fpm: f64, ground_speed_kt: f64) -> f64 {
    if ground_speed_kt <= 0.0 {
        f64::INFINITY
    } else {
        rate_fpm * 60.0 / ground_speed_kt
    }
}

/// Distance along `legs` needed to change altitude by `altitude_ft` at
/// `rate_fpm`. `None` when the route runs out first — i.e. the aircraft
/// can't complete the climb (or descent) within the route as planned.
fn distance_for_altitude_change(
    legs: &[VerticalLeg],
    altitude_ft: f64,
    rate_fpm: f64,
) -> Option<f64> {
    // Already at (or above) the target altitude: nothing to fly.
    if altitude_ft <= 0.0 {
        return Some(0.0);
    }
    if rate_fpm <= 0.0 {
        return None;
    }
    let mut remaining_ft = altitude_ft;
    let mut travelled_nm = 0.0;
    for leg in legs {
        let gradient = gradient_ft_per_nm(rate_fpm, leg.ground_speed_kt);
        if !gradient.is_finite() {
            // Climbing in place — the rest of the change happens here.
            return Some(travelled_nm);
        }
        let change_ft = gradient * leg.distance_nm;
        if change_ft >= remaining_ft {
            return Some(travelled_nm + remaining_ft / gradient);
        }
        remaining_ft -= change_ft;
        travelled_nm += leg.distance_nm;
    }
    None
}

/// Altitude reached after travelling `distance_nm` along `legs` from
/// `start_alt_ft`, changing at `rate_fpm`. Distances past the end of
/// `legs` just stop at the end.
fn altitude_after(legs: &[VerticalLeg], start_alt_ft: f64, rate_fpm: f64, distance_nm: f64) -> f64 {
    let mut altitude_ft = start_alt_ft;
    let mut remaining_nm = distance_nm;
    for leg in legs {
        if remaining_nm <= 0.0 {
            break;
        }
        let step_nm = remaining_nm.min(leg.distance_nm);
        if step_nm <= 0.0 {
            continue;
        }
        let gradient = gradient_ft_per_nm(rate_fpm, leg.ground_speed_kt);
        if !gradient.is_finite() {
            return f64::INFINITY;
        }
        altitude_ft += gradient * step_nm;
        remaining_nm -= step_nm;
    }
    altitude_ft
}

/// Where the climb profile (from the departure end) meets the descent
/// profile (from the arrival end), as a distance from departure. Only
/// meaningful when the two overlap — see [`VerticalProfile::cruise_reached`].
fn crossover_distance_nm(
    climb_legs: &[VerticalLeg],
    descent_legs_reversed: &[VerticalLeg],
    departure_elevation_ft: f64,
    climb_rate_fpm: f64,
    arrival_elevation_ft: f64,
    descent_rate_fpm: f64,
    total_distance_nm: f64,
) -> f64 {
    // climb altitude − descent altitude at the same point: strictly
    // increasing in `d` (one profile rises as the other falls), so it
    // crosses zero exactly once and a plain bisection is enough. Solving
    // it analytically would mean merging both profiles' leg breakpoints,
    // which buys nothing here — 100 halvings of a route is well past f64
    // precision.
    let difference_at = |d: f64| {
        altitude_after(climb_legs, departure_elevation_ft, climb_rate_fpm, d)
            - altitude_after(
                descent_legs_reversed,
                arrival_elevation_ft,
                descent_rate_fpm,
                total_distance_nm - d,
            )
    };
    if difference_at(0.0) >= 0.0 {
        return 0.0;
    }
    if difference_at(total_distance_nm) <= 0.0 {
        return total_distance_nm;
    }
    let (mut low, mut high) = (0.0, total_distance_nm);
    for _ in 0..100 {
        let mid = (low + high) / 2.0;
        if difference_at(mid) < 0.0 {
            low = mid;
        } else {
            high = mid;
        }
    }
    (low + high) / 2.0
}

/// A point on the route where the vertical profile changes: the top of
/// climb or the top of descent.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct VerticalPoint {
    pub distance_from_departure_nm: f64,
    pub distance_to_arrival_nm: f64,
    /// Position, interpolated along the great circle of the leg it falls
    /// on — ready to drop on the map.
    pub lat: f64,
    pub lon: f64,
    /// Which leg it falls on (0 = the first leg, departure → next fix)
    /// and how far along that leg it is, 0.0-1.0 — so the client can say
    /// "12 nm after FOO" without repeating the walk.
    pub leg_index: usize,
    pub leg_fraction: f64,
    /// Altitude here (ft MSL): the cruise altitude, unless it was never
    /// reachable (see [`VerticalProfile::cruise_reached`]).
    pub altitude_ft: f64,
    /// Minutes spent climbing to this point, or descending from it.
    pub time_min: f64,
}

/// The route's vertical profile. `top_of_climb`/`top_of_descent` are
/// independently optional: each needs its own field elevation and rate,
/// so a plan with only a departure elevation still gets a top of climb.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VerticalProfile {
    pub top_of_climb: Option<VerticalPoint>,
    pub top_of_descent: Option<VerticalPoint>,
    /// The requested cruise altitude (ft MSL), whether or not it fits.
    pub cruise_altitude_ft: f64,
    /// False when the route is too short to reach cruise. Both points
    /// then collapse onto the single crossover point, at
    /// `peak_altitude_ft` rather than `cruise_altitude_ft` — the plan
    /// needs a lower cruise altitude.
    pub cruise_reached: bool,
    /// Highest altitude actually achievable on this route (equals
    /// `cruise_altitude_ft` whenever `cruise_reached`).
    pub peak_altitude_ft: f64,
    /// Level distance between the two points; 0.0 when cruise isn't reached.
    pub cruise_distance_nm: f64,
    pub total_distance_nm: f64,
}

/// Compute the top of climb and top of descent for a route.
///
/// `winds` follows [`crate::route::plan_route`]: one entry per leg
/// (`points.len() - 1`), `None` where no wind data applies. Elevations are
/// optional because the client may not know both airports' field
/// elevations — each phase is computed only if its own end is known.
///
/// Returns `None` when there's nothing to compute: fewer than two points,
/// no cruise altitude in the profile, or neither phase has both the field
/// elevation and the climb/descent rate it needs.
pub fn plan_vertical(
    points: &[RoutePoint],
    profile: &AircraftProfile,
    winds: Option<&[Option<Wind>]>,
    departure_elevation_ft: Option<f64>,
    arrival_elevation_ft: Option<f64>,
) -> Option<VerticalProfile> {
    if points.len() < 2 {
        return None;
    }
    let cruise_altitude_ft = profile.cruise_altitude_ft?;
    // A missing climb/descent TAS falls back to cruise TAS; a missing
    // *rate* is what makes a phase uncomputable (see AircraftProfile).
    let climb_tas_kt = profile.climb_tas_kt.unwrap_or(profile.cruise_tas_kt);
    let descent_tas_kt = profile.descent_tas_kt.unwrap_or(profile.cruise_tas_kt);
    let climb_input = departure_elevation_ft.zip(profile.climb_rate_fpm);
    let descent_input = arrival_elevation_ft.zip(profile.descent_rate_fpm);
    if climb_input.is_none() && descent_input.is_none() {
        return None;
    }

    // Per-leg geometry once, then a groundspeed per phase over it.
    let leg_distances_nm: Vec<f64> = points
        .windows(2)
        .map(|pair| distance_nm((pair[0].lat, pair[0].lon), (pair[1].lat, pair[1].lon)))
        .collect();
    let ground_speed_on = |leg_index: usize, true_airspeed_kt: f64| -> f64 {
        let pair = &points[leg_index..leg_index + 2];
        match winds.and_then(|w| w.get(leg_index).copied().flatten()) {
            Some(wind) => {
                let course =
                    initial_bearing_deg((pair[0].lat, pair[0].lon), (pair[1].lat, pair[1].lon));
                solve_wind_triangle(course, true_airspeed_kt, wind).ground_speed_kt
            }
            None => true_airspeed_kt,
        }
    };
    let legs_at = |true_airspeed_kt: f64| -> Vec<VerticalLeg> {
        leg_distances_nm
            .iter()
            .enumerate()
            .map(|(i, &distance_nm)| VerticalLeg {
                distance_nm,
                ground_speed_kt: ground_speed_on(i, true_airspeed_kt),
            })
            .collect()
    };
    let climb_legs = legs_at(climb_tas_kt);
    // Descent is flown from the arrival end backwards, so its legs walk
    // the route in reverse.
    let mut descent_legs = legs_at(descent_tas_kt);
    descent_legs.reverse();

    let total_distance_nm: f64 = leg_distances_nm.iter().sum();

    let toc_distance_nm = climb_input.and_then(|(elevation_ft, rate_fpm)| {
        distance_for_altitude_change(&climb_legs, cruise_altitude_ft - elevation_ft, rate_fpm)
    });
    let tod_distance_nm = descent_input.and_then(|(elevation_ft, rate_fpm)| {
        distance_for_altitude_change(&descent_legs, cruise_altitude_ft - elevation_ft, rate_fpm)
            .map(|from_arrival_nm| total_distance_nm - from_arrival_nm)
    });

    let cruise_reached = match (toc_distance_nm, tod_distance_nm) {
        (Some(toc), Some(tod)) => toc <= tod,
        // Only one phase was asked for: reaching cruise on that phase is
        // all there is to check.
        (Some(_), None) => descent_input.is_none(),
        (None, Some(_)) => climb_input.is_none(),
        (None, None) => false,
    };

    let point_at =
        |distance_from_departure_nm: f64, altitude_ft: f64, time_min: f64| -> VerticalPoint {
            let clamped_nm = distance_from_departure_nm.clamp(0.0, total_distance_nm);
            let mut remaining_nm = clamped_nm;
            let last_leg = leg_distances_nm.len() - 1;
            let (mut leg_index, mut leg_fraction) = (last_leg, 1.0);
            for (i, &leg_nm) in leg_distances_nm.iter().enumerate() {
                if remaining_nm <= leg_nm || i == last_leg {
                    leg_index = i;
                    leg_fraction = if leg_nm > 0.0 {
                        (remaining_nm / leg_nm).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    break;
                }
                remaining_nm -= leg_nm;
            }
            let (lat, lon) = intermediate_point(
                (points[leg_index].lat, points[leg_index].lon),
                (points[leg_index + 1].lat, points[leg_index + 1].lon),
                leg_fraction,
            );
            VerticalPoint {
                distance_from_departure_nm: clamped_nm,
                distance_to_arrival_nm: total_distance_nm - clamped_nm,
                lat,
                lon,
                leg_index,
                leg_fraction,
                altitude_ft,
                time_min,
            }
        };
    // Time to climb to / descend from an altitude, given the phase's end
    // elevation and rate — the vertical leg is what sets it, not the
    // ground distance.
    let phase_minutes = |altitude_ft: f64, elevation_ft: f64, rate_fpm: f64| {
        ((altitude_ft - elevation_ft) / rate_fpm).max(0.0)
    };

    if cruise_reached {
        let top_of_climb = toc_distance_nm
            .zip(climb_input)
            .map(|(d, (elevation, rate))| {
                point_at(
                    d,
                    cruise_altitude_ft,
                    phase_minutes(cruise_altitude_ft, elevation, rate),
                )
            });
        let top_of_descent = tod_distance_nm
            .zip(descent_input)
            .map(|(d, (elevation, rate))| {
                point_at(
                    d,
                    cruise_altitude_ft,
                    phase_minutes(cruise_altitude_ft, elevation, rate),
                )
            });
        let cruise_distance_nm = match (&top_of_climb, &top_of_descent) {
            (Some(toc), Some(tod)) => {
                (tod.distance_from_departure_nm - toc.distance_from_departure_nm).max(0.0)
            }
            // Only one end modelled — the rest of the route is level as
            // far as this profile knows.
            (Some(toc), None) => toc.distance_to_arrival_nm,
            (None, Some(tod)) => tod.distance_from_departure_nm,
            (None, None) => 0.0,
        };
        return Some(VerticalProfile {
            top_of_climb,
            top_of_descent,
            cruise_altitude_ft,
            cruise_reached: true,
            peak_altitude_ft: cruise_altitude_ft,
            cruise_distance_nm,
            total_distance_nm,
        });
    }

    // Cruise doesn't fit. With both ends modelled the climb and descent
    // profiles cross somewhere — that crossing is the real top of climb
    // *and* top of descent, at a lower peak altitude.
    if let (Some((departure_elevation, climb_rate)), Some((arrival_elevation, descent_rate))) =
        (climb_input, descent_input)
    {
        let crossover_nm = crossover_distance_nm(
            &climb_legs,
            &descent_legs,
            departure_elevation,
            climb_rate,
            arrival_elevation,
            descent_rate,
            total_distance_nm,
        );
        let peak_altitude_ft =
            altitude_after(&climb_legs, departure_elevation, climb_rate, crossover_nm)
                .min(cruise_altitude_ft);
        let point = point_at(
            crossover_nm,
            peak_altitude_ft,
            phase_minutes(peak_altitude_ft, departure_elevation, climb_rate),
        );
        return Some(VerticalProfile {
            top_of_climb: Some(point),
            top_of_descent: Some(VerticalPoint {
                time_min: phase_minutes(peak_altitude_ft, arrival_elevation, descent_rate),
                ..point
            }),
            cruise_altitude_ft,
            cruise_reached: false,
            peak_altitude_ft,
            cruise_distance_nm: 0.0,
            total_distance_nm,
        });
    }

    // Only one phase is modelled and it doesn't finish inside the route:
    // report how high it gets, with no point to mark.
    let peak_altitude_ft = match (climb_input, descent_input) {
        (Some((elevation, rate)), _) => {
            altitude_after(&climb_legs, elevation, rate, total_distance_nm).min(cruise_altitude_ft)
        }
        (_, Some((elevation, rate))) => {
            altitude_after(&descent_legs, elevation, rate, total_distance_nm)
                .min(cruise_altitude_ft)
        }
        (None, None) => unreachable!("returned early when neither phase has inputs"),
    };
    Some(VerticalProfile {
        top_of_climb: None,
        top_of_descent: None,
        cruise_altitude_ft,
        cruise_reached: false,
        peak_altitude_ft,
        cruise_distance_nm: 0.0,
        total_distance_nm,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const KSFO: RoutePoint = RoutePoint {
        lat: 37.6188,
        lon: -122.375,
    };
    const KSJC: RoutePoint = RoutePoint {
        lat: 37.363,
        lon: -121.929,
    };
    const KMRY: RoutePoint = RoutePoint {
        lat: 36.5844,
        lon: -121.8429,
    };
    /// Field elevations (ft) for the airports above.
    const KSFO_ELEV: f64 = 13.0;
    const KMRY_ELEV: f64 = 257.0;

    /// A C172 with the vertical performance route.rs's own fixture leaves
    /// unset: 700 fpm at 75 kt up, 500 fpm at 110 kt down.
    fn cessna_172(cruise_altitude_ft: Option<f64>) -> AircraftProfile {
        AircraftProfile {
            name: "C172".into(),
            cruise_tas_kt: 110.0,
            fuel_burn_gph: 8.5,
            max_gross_weight_lb: Some(2450.0),
            forward_cg_limit_in: Some(35.0),
            aft_cg_limit_in: Some(47.3),
            cruise_altitude_ft,
            climb_rate_fpm: Some(700.0),
            climb_tas_kt: Some(75.0),
            descent_rate_fpm: Some(500.0),
            descent_tas_kt: Some(110.0),
            // Fuel and table-driven performance belong to flight.rs's
            // tests; this fixture is about the vertical geometry.
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
    fn computes_top_of_climb_and_descent_with_no_wind() {
        let points = [KSFO, KMRY];
        let profile = cessna_172(Some(6500.0));
        let vertical =
            plan_vertical(&points, &profile, None, Some(KSFO_ELEV), Some(KMRY_ELEV)).unwrap();

        assert!(vertical.cruise_reached);
        assert_eq!(vertical.peak_altitude_ft, 6500.0);

        // Climb: (6500 − 13) ft at 700 fpm = 9.267 min; at 75 kt with no
        // wind that's 9.267/60 * 75 = 11.58 nm.
        let toc = vertical.top_of_climb.unwrap();
        assert!((toc.time_min - 9.267).abs() < 0.01, "{} min", toc.time_min);
        assert!(
            (toc.distance_from_departure_nm - 11.58).abs() < 0.05,
            "TOC at {} nm",
            toc.distance_from_departure_nm
        );
        assert_eq!(toc.altitude_ft, 6500.0);

        // Descent: (6500 − 257) ft at 500 fpm = 12.486 min; at 110 kt
        // that's 22.89 nm back from the arrival airport.
        let tod = vertical.top_of_descent.unwrap();
        assert!((tod.time_min - 12.486).abs() < 0.01, "{} min", tod.time_min);
        assert!(
            (tod.distance_to_arrival_nm - 22.89).abs() < 0.05,
            "TOD {} nm out",
            tod.distance_to_arrival_nm
        );

        assert!(toc.distance_from_departure_nm < tod.distance_from_departure_nm);
        let expected_cruise = tod.distance_from_departure_nm - toc.distance_from_departure_nm;
        assert!((vertical.cruise_distance_nm - expected_cruise).abs() < 1e-9);
    }

    #[test]
    fn the_points_lie_on_the_route() {
        let points = [KSFO, KSJC, KMRY];
        let vertical = plan_vertical(
            &points,
            &cessna_172(Some(6500.0)),
            None,
            Some(KSFO_ELEV),
            Some(KMRY_ELEV),
        )
        .unwrap();

        // The interpolated position really is that far along the route:
        // TOC lands inside the first leg, so its distance from KSFO is
        // just the great-circle distance to it.
        let toc = vertical.top_of_climb.unwrap();
        assert_eq!(toc.leg_index, 0);
        let straight_line_nm = distance_nm((KSFO.lat, KSFO.lon), (toc.lat, toc.lon));
        assert!(
            (straight_line_nm - toc.distance_from_departure_nm).abs() < 0.01,
            "{straight_line_nm} nm vs {} nm",
            toc.distance_from_departure_nm
        );

        // TOD is ~23 nm from KMRY, which is on the second leg (KSJC → KMRY
        // is ~47 nm), and the distance back to the arrival matches too.
        let tod = vertical.top_of_descent.unwrap();
        assert_eq!(tod.leg_index, 1);
        let to_arrival_nm = distance_nm((tod.lat, tod.lon), (KMRY.lat, KMRY.lon));
        assert!(
            (to_arrival_nm - tod.distance_to_arrival_nm).abs() < 0.01,
            "{to_arrival_nm} nm vs {} nm",
            tod.distance_to_arrival_nm
        );
    }

    #[test]
    fn a_headwind_moves_the_top_of_climb_closer() {
        let points = [KSFO, KMRY];
        let profile = cessna_172(Some(6500.0));
        let course = initial_bearing_deg((KSFO.lat, KSFO.lon), (KMRY.lat, KMRY.lon));
        let headwind = [Some(Wind {
            direction_true_deg: course, // straight down the leg, on the nose
            speed_kt: 25.0,
        })];

        let still_air =
            plan_vertical(&points, &profile, None, Some(KSFO_ELEV), Some(KMRY_ELEV)).unwrap();
        let into_wind = plan_vertical(
            &points,
            &profile,
            Some(&headwind),
            Some(KSFO_ELEV),
            Some(KMRY_ELEV),
        )
        .unwrap();

        // Same time to climb either way — it's the ground distance that
        // shrinks (75 kt TAS − 25 kt headwind = 50 kt over the ground).
        let (calm_toc, windy_toc) = (
            still_air.top_of_climb.unwrap(),
            into_wind.top_of_climb.unwrap(),
        );
        assert!((calm_toc.time_min - windy_toc.time_min).abs() < 1e-9);
        assert!(
            windy_toc.distance_from_departure_nm < calm_toc.distance_from_departure_nm,
            "{} should be under {}",
            windy_toc.distance_from_departure_nm,
            calm_toc.distance_from_departure_nm
        );
        assert!((windy_toc.distance_from_departure_nm - 9.267 / 60.0 * 50.0).abs() < 0.05);

        // The descent runs down the same course into the same headwind,
        // so it too covers less ground: start down later, closer in.
        assert!(
            into_wind.top_of_descent.unwrap().distance_to_arrival_nm
                < still_air.top_of_descent.unwrap().distance_to_arrival_nm
        );
    }

    #[test]
    fn a_short_hop_never_reaches_cruise() {
        // KSFO → KSJC is ~26 nm: 11.6 nm of climb plus 22.9 nm of descent
        // doesn't fit, so 6,500 ft is unachievable.
        let points = [KSFO, KSJC];
        let vertical = plan_vertical(
            &points,
            &cessna_172(Some(6500.0)),
            None,
            Some(KSFO_ELEV),
            Some(62.0), // KSJC field elevation
        )
        .unwrap();

        assert!(!vertical.cruise_reached);
        assert_eq!(vertical.cruise_distance_nm, 0.0);
        assert!(
            vertical.peak_altitude_ft > 62.0 && vertical.peak_altitude_ft < 6500.0,
            "peak was {}",
            vertical.peak_altitude_ft
        );

        // Both points collapse onto the crossover, and the climb and
        // descent profiles agree on the altitude there.
        let toc = vertical.top_of_climb.unwrap();
        let tod = vertical.top_of_descent.unwrap();
        assert!((toc.distance_from_departure_nm - tod.distance_from_departure_nm).abs() < 1e-6);
        assert_eq!(toc.altitude_ft, vertical.peak_altitude_ft);
        // Climbing at 700 fpm for `time_min` from the field really does
        // reach the reported peak.
        assert!(
            (KSFO_ELEV + toc.time_min * 700.0 - vertical.peak_altitude_ft).abs() < 0.5,
            "{} min of climb didn't reach {}",
            toc.time_min,
            vertical.peak_altitude_ft
        );
        assert!((62.0 + tod.time_min * 500.0 - vertical.peak_altitude_ft).abs() < 0.5);
    }

    #[test]
    fn a_cruise_altitude_below_the_field_needs_no_climb() {
        // Cruise below the departure elevation: the climb is already done
        // at the runway, so TOC sits at zero distance.
        let points = [KSFO, KMRY];
        let vertical = plan_vertical(
            &points,
            &cessna_172(Some(10.0)),
            None,
            Some(KSFO_ELEV),
            Some(KMRY_ELEV),
        )
        .unwrap();
        let toc = vertical.top_of_climb.unwrap();
        assert_eq!(toc.distance_from_departure_nm, 0.0);
        assert_eq!(toc.time_min, 0.0);
        assert_eq!((toc.lat, toc.lon), (KSFO.lat, KSFO.lon));
    }

    #[test]
    fn one_known_elevation_still_gives_that_end() {
        let points = [KSFO, KMRY];
        let vertical = plan_vertical(
            &points,
            &cessna_172(Some(6500.0)),
            None,
            Some(KSFO_ELEV),
            None,
        )
        .unwrap();
        assert!(vertical.top_of_climb.is_some());
        assert!(vertical.top_of_descent.is_none());
        assert!(vertical.cruise_reached);
    }

    #[test]
    fn returns_nothing_without_the_inputs_it_needs() {
        let points = [KSFO, KMRY];
        // No cruise altitude.
        assert!(plan_vertical(
            &points,
            &cessna_172(None),
            None,
            Some(KSFO_ELEV),
            Some(KMRY_ELEV)
        )
        .is_none());
        // No field elevations.
        assert!(plan_vertical(&points, &cessna_172(Some(6500.0)), None, None, None).is_none());
        // No climb/descent rates.
        let mut no_rates = cessna_172(Some(6500.0));
        no_rates.climb_rate_fpm = None;
        no_rates.descent_rate_fpm = None;
        assert!(
            plan_vertical(&points, &no_rates, None, Some(KSFO_ELEV), Some(KMRY_ELEV)).is_none()
        );
        // Too few points.
        assert!(plan_vertical(
            &[KSFO],
            &cessna_172(Some(6500.0)),
            None,
            Some(KSFO_ELEV),
            Some(KMRY_ELEV)
        )
        .is_none());
    }
}
