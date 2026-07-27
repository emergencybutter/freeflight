//! One planning pass over a route: nav log, vertical profile, and a
//! phase-aware fuel total (DESIGN.md §9.5.6).
//!
//! [`crate::route::plan_route`] and [`crate::vertical::plan_vertical`]
//! were independent calls, which was fine while they shared nothing. Fuel
//! changes that: burn is only phase-aware if the *same* pass knows how
//! long the climb and descent take, which is precisely what the vertical
//! profile computes. Planning them together also lets cruise TAS come out
//! of a real performance table instead of a single typed-in number.
//!
//! ## What this fixes about fuel
//!
//! §9.3 totalled fuel as cruise GPH × total ETE. A 172 burns ~11 gph
//! climbing and ~5.5 gph descending against ~7.9 in cruise, so on a short
//! hop — where climb and descent are most of the flight — that total is
//! simply the wrong shape. Here it decomposes:
//!
//! ```text
//! taxi allowance
//!   + climb minutes   × climb GPH
//!   + cruise hours    × cruise GPH
//!   + descent minutes × descent GPH
//!   + reserve minutes × cruise GPH
//! ```
//!
//! ## What it deliberately does not fix
//!
//! **Leg ETE is still cruise-based.** A leg that physically spans the
//! climb is still timed at cruise TAS in the nav log. Apportioning every
//! leg across phases means modelling climb rate falling with altitude
//! along each leg — a rewrite of the nav log's core, and out of scope
//! here (§9.5.6). The consequence is visible rather than hidden:
//! [`FuelSummary`] reports the phase times it actually used, so a client
//! can show them next to the nav log's total and the difference is on
//! screen instead of buried.

use crate::performance::AircraftPerformance;
use crate::route::{plan_route, AircraftProfile, RouteLegPlan, RoutePoint};
use crate::vertical::{plan_vertical, VerticalProfile};
use crate::wind::Wind;
use serde::{Deserialize, Serialize};

/// Where a planning figure came from, so a client can say "from your
/// performance table" rather than presenting a book number and a typed-in
/// one identically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueSource {
    /// Interpolated from the aircraft's performance table.
    Table,
    /// The profile's single scalar field.
    Scalar,
}

/// Fuel required for the flight, by phase.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FuelSummary {
    pub taxi_gal: f64,
    pub climb_gal: f64,
    pub cruise_gal: f64,
    pub descent_gal: f64,
    /// Fuel to fly the route: taxi + climb + cruise + descent.
    pub trip_gal: f64,
    /// Reserve at cruise burn, from the profile's `reserve_minutes`.
    pub reserve_gal: f64,
    /// What must actually be in the tanks: `trip_gal + reserve_gal`.
    pub required_gal: f64,
    /// `None` when the profile records no tank capacity.
    pub capacity_gal: Option<f64>,
    pub within_capacity: Option<bool>,
    /// The phase times this total was built from. Reported because they
    /// come from the vertical profile rather than the nav log, and a
    /// client should be able to show the difference.
    pub climb_minutes: f64,
    pub cruise_hours: f64,
    pub descent_minutes: f64,
    /// False when there was no vertical profile to decompose against, so
    /// the whole flight was charged at cruise burn — the §9.3 behaviour,
    /// flagged rather than silently assumed.
    pub phase_aware: bool,
}

/// A complete plan: the nav log, where the climb tops out and the descent
/// begins, and what it all costs in fuel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FlightPlanSummary {
    pub legs: Vec<RouteLegPlan>,
    pub total_distance_nm: f64,
    pub total_ete_hours: f64,
    /// `None` when the vertical profile could not be computed (no cruise
    /// altitude, no field elevation, no climb/descent rate).
    pub vertical: Option<VerticalProfile>,
    pub fuel: FuelSummary,
    /// The cruise TAS actually planned with, and where it came from.
    pub cruise_tas_kt: f64,
    pub cruise_tas_source: ValueSource,
    pub cruise_fuel_gph: f64,
    pub cruise_fuel_source: ValueSource,
}

/// Resolve a value from the performance table, falling back to a scalar.
fn resolve(from_table: Option<f64>, scalar: f64) -> (f64, ValueSource) {
    match from_table {
        Some(value) => (value, ValueSource::Table),
        None => (scalar, ValueSource::Scalar),
    }
}

/// Plan a route end to end.
///
/// `winds` and `decimal_year` are as [`plan_route`] takes them; the
/// elevations are as [`plan_vertical`] takes them. The profile's
/// performance tables, when present, override its scalar cruise figures
/// and supply per-phase fuel burn.
pub fn plan_flight(
    points: &[RoutePoint],
    profile: &AircraftProfile,
    winds: Option<&[Option<Wind>]>,
    departure_elevation_ft: Option<f64>,
    arrival_elevation_ft: Option<f64>,
    decimal_year: f64,
) -> FlightPlanSummary {
    let performance = profile.performance.as_ref();
    let cruise_altitude_ft = profile.cruise_altitude_ft;

    // --- cruise figures, table first -------------------------------------
    let cruise_point = cruise_altitude_ft.and_then(|altitude| {
        performance?.cruise_at(altitude, profile.cruise_power_setting.as_deref())
    });
    let (cruise_tas_kt, cruise_tas_source) = resolve(
        cruise_point.as_ref().map(|p| p.tas_kt),
        profile.cruise_tas_kt,
    );
    let (cruise_fuel_gph, cruise_fuel_source) = resolve(
        cruise_point.as_ref().map(|p| p.fuel_gph),
        profile.fuel_burn_gph,
    );

    // The rest of the plan runs against a profile carrying the resolved
    // figures, so the nav log's legs and the vertical profile's
    // groundspeeds agree with each other and with the fuel total.
    let mut effective = profile.clone();
    effective.cruise_tas_kt = cruise_tas_kt;
    effective.fuel_burn_gph = cruise_fuel_gph;
    apply_table_to_phases(
        &mut effective,
        performance,
        departure_elevation_ft,
        arrival_elevation_ft,
    );

    let route = plan_route(points, &effective, winds, decimal_year);
    let vertical = plan_vertical(
        points,
        &effective,
        winds,
        departure_elevation_ft,
        arrival_elevation_ft,
    );

    let fuel = summarize_fuel(
        &effective,
        performance,
        vertical.as_ref(),
        route.total_distance_nm,
        route.total_ete_hours,
        cruise_fuel_gph,
        departure_elevation_ft,
        arrival_elevation_ft,
    );

    FlightPlanSummary {
        legs: route.legs,
        total_distance_nm: route.total_distance_nm,
        total_ete_hours: route.total_ete_hours,
        vertical,
        fuel,
        cruise_tas_kt,
        cruise_tas_source,
        cruise_fuel_gph,
        cruise_fuel_source,
    }
}

/// Fill the climb/descent rate and TAS from the tables where the profile
/// has none, so the vertical profile benefits from real data too.
///
/// Evaluated at the *midpoint* of each phase, since a climb spans a range
/// of altitudes and one number has to stand for all of it. Averaging the
/// ends would be no better and reads as less obviously an approximation.
fn apply_table_to_phases(
    profile: &mut AircraftProfile,
    performance: Option<&AircraftPerformance>,
    departure_elevation_ft: Option<f64>,
    arrival_elevation_ft: Option<f64>,
) {
    let Some(performance) = performance else {
        return;
    };
    let Some(cruise_altitude) = profile.cruise_altitude_ft else {
        return;
    };
    if let Some(point) = performance.climb_at(midpoint(
        departure_elevation_ft.unwrap_or(0.0),
        cruise_altitude,
    )) {
        profile.climb_rate_fpm = profile.climb_rate_fpm.or(point.vertical_speed_fpm);
        profile.climb_tas_kt = profile.climb_tas_kt.or(Some(point.tas_kt));
        profile.climb_fuel_gph = profile.climb_fuel_gph.or(Some(point.fuel_gph));
    }
    if let Some(point) = performance.descent_at(midpoint(
        arrival_elevation_ft.unwrap_or(0.0),
        cruise_altitude,
    )) {
        profile.descent_rate_fpm = profile.descent_rate_fpm.or(point.vertical_speed_fpm);
        profile.descent_tas_kt = profile.descent_tas_kt.or(Some(point.tas_kt));
        profile.descent_fuel_gph = profile.descent_fuel_gph.or(Some(point.fuel_gph));
    }
}

fn midpoint(a: f64, b: f64) -> f64 {
    (a + b) / 2.0
}

#[allow(clippy::too_many_arguments)]
fn summarize_fuel(
    profile: &AircraftProfile,
    performance: Option<&AircraftPerformance>,
    vertical: Option<&VerticalProfile>,
    total_distance_nm: f64,
    total_ete_hours: f64,
    cruise_fuel_gph: f64,
    departure_elevation_ft: Option<f64>,
    arrival_elevation_ft: Option<f64>,
) -> FuelSummary {
    let taxi_gal = profile.taxi_fuel_gal.unwrap_or(0.0);
    let reserve_gal = profile.reserve_minutes.unwrap_or(0).max(0) as f64 / 60.0 * cruise_fuel_gph;
    let capacity_gal = profile.fuel_capacity_gal;

    let phase_gph = |scalar: Option<f64>, from_table: Option<f64>| -> f64 {
        scalar.or(from_table).unwrap_or(cruise_fuel_gph)
    };

    // Without a vertical profile there are no phase times to charge
    // against — fall back to §9.3's whole-flight-at-cruise-burn, flagged.
    let Some(vertical) = vertical else {
        let cruise_gal = total_ete_hours * cruise_fuel_gph;
        let trip_gal = taxi_gal + cruise_gal;
        return FuelSummary {
            taxi_gal,
            climb_gal: 0.0,
            cruise_gal,
            descent_gal: 0.0,
            trip_gal,
            reserve_gal,
            required_gal: trip_gal + reserve_gal,
            capacity_gal,
            within_capacity: capacity_gal.map(|c| trip_gal + reserve_gal <= c),
            climb_minutes: 0.0,
            cruise_hours: total_ete_hours,
            descent_minutes: 0.0,
            phase_aware: false,
        };
    };

    let climb_minutes = vertical.top_of_climb.map(|p| p.time_min).unwrap_or(0.0);
    let descent_minutes = vertical.top_of_descent.map(|p| p.time_min).unwrap_or(0.0);

    // Cruise time from the level distance at the nav log's own average
    // groundspeed, so the two models stay reconcilable: the nav log timed
    // the whole route at cruise, and this charges cruise burn only over
    // the part that is actually level.
    let average_ground_speed_kt = if total_ete_hours > 0.0 {
        total_distance_nm / total_ete_hours
    } else {
        0.0
    };
    let cruise_hours = if average_ground_speed_kt > 0.0 {
        vertical.cruise_distance_nm / average_ground_speed_kt
    } else {
        0.0
    };

    let climb_gph = phase_gph(
        profile.climb_fuel_gph,
        performance
            .and_then(|p| {
                p.climb_at(midpoint(
                    departure_elevation_ft.unwrap_or(0.0),
                    vertical.peak_altitude_ft,
                ))
            })
            .map(|p| p.fuel_gph),
    );
    let descent_gph = phase_gph(
        profile.descent_fuel_gph,
        performance
            .and_then(|p| {
                p.descent_at(midpoint(
                    arrival_elevation_ft.unwrap_or(0.0),
                    vertical.peak_altitude_ft,
                ))
            })
            .map(|p| p.fuel_gph),
    );

    let climb_gal = climb_minutes / 60.0 * climb_gph;
    let cruise_gal = cruise_hours * cruise_fuel_gph;
    let descent_gal = descent_minutes / 60.0 * descent_gph;
    let trip_gal = taxi_gal + climb_gal + cruise_gal + descent_gal;
    let required_gal = trip_gal + reserve_gal;

    FuelSummary {
        taxi_gal,
        climb_gal,
        cruise_gal,
        descent_gal,
        trip_gal,
        reserve_gal,
        required_gal,
        capacity_gal,
        within_capacity: capacity_gal.map(|c| required_gal <= c),
        climb_minutes,
        cruise_hours,
        descent_minutes,
        phase_aware: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::performance::PerformancePoint;

    const KSFO: RoutePoint = RoutePoint {
        lat: 37.6188,
        lon: -122.375,
    };
    const KMRY: RoutePoint = RoutePoint {
        lat: 36.5844,
        lon: -121.8429,
    };
    const KSFO_ELEV: f64 = 13.0;
    const KMRY_ELEV: f64 = 257.0;

    fn base_profile() -> AircraftProfile {
        AircraftProfile {
            name: "C172".into(),
            cruise_tas_kt: 110.0,
            fuel_burn_gph: 7.9,
            max_gross_weight_lb: Some(2550.0),
            forward_cg_limit_in: None,
            aft_cg_limit_in: None,
            cruise_altitude_ft: Some(6500.0),
            climb_rate_fpm: Some(700.0),
            climb_tas_kt: Some(75.0),
            descent_rate_fpm: Some(500.0),
            descent_tas_kt: Some(110.0),
            climb_fuel_gph: Some(11.0),
            descent_fuel_gph: Some(5.5),
            taxi_fuel_gal: Some(1.4),
            fuel_capacity_gal: Some(53.0),
            reserve_minutes: Some(45),
            performance: None,
            cruise_power_setting: None,
        }
    }

    fn cruise_table() -> AircraftPerformance {
        AircraftPerformance {
            cruise: vec![
                PerformancePoint {
                    pressure_altitude_ft: 4000.0,
                    tas_kt: 108.0,
                    fuel_gph: 8.2,
                    vertical_speed_fpm: None,
                    power_setting: "65%".into(),
                },
                PerformancePoint {
                    pressure_altitude_ft: 8000.0,
                    tas_kt: 112.0,
                    fuel_gph: 7.6,
                    vertical_speed_fpm: None,
                    power_setting: "65%".into(),
                },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn fuel_is_decomposed_by_phase() {
        let plan = plan_flight(
            &[KSFO, KMRY],
            &base_profile(),
            None,
            Some(KSFO_ELEV),
            Some(KMRY_ELEV),
            2026.5,
        );
        let fuel = &plan.fuel;
        assert!(fuel.phase_aware);

        // Climb: (6500-13)/700 min at 11 gph. Derived rather than typed,
        // so the tolerance isn't testing how many decimals were written
        // down here.
        let climb_minutes = (6500.0 - KSFO_ELEV) / 700.0;
        assert!((fuel.climb_minutes - climb_minutes).abs() < 1e-9);
        assert!(
            (fuel.climb_gal - climb_minutes / 60.0 * 11.0).abs() < 1e-9,
            "{}",
            fuel.climb_gal
        );
        // Descent: (6500-257)/500 min at 5.5 gph.
        let descent_minutes = (6500.0 - KMRY_ELEV) / 500.0;
        assert!((fuel.descent_gal - descent_minutes / 60.0 * 5.5).abs() < 1e-9);
        // Taxi is a flat allowance, not a rate.
        assert!((fuel.taxi_gal - 1.4).abs() < 1e-9);
        // Reserve is 45 min at cruise burn.
        assert!((fuel.reserve_gal - 45.0 / 60.0 * 7.9).abs() < 1e-9);

        let summed = fuel.taxi_gal + fuel.climb_gal + fuel.cruise_gal + fuel.descent_gal;
        assert!((fuel.trip_gal - summed).abs() < 1e-9);
        assert!((fuel.required_gal - (fuel.trip_gal + fuel.reserve_gal)).abs() < 1e-9);
        assert_eq!(fuel.within_capacity, Some(true));

        // The whole point: charging the climb at cruise burn would
        // under-count it, since the climb burns more.
        let naive = plan.total_ete_hours * 7.9 + 1.4;
        assert!(
            fuel.trip_gal > naive,
            "phase-aware total {} should exceed the cruise-burn-only {naive}",
            fuel.trip_gal
        );
    }

    #[test]
    fn cruise_figures_come_from_the_table_when_there_is_one() {
        let mut profile = base_profile();
        profile.performance = Some(cruise_table());
        profile.cruise_altitude_ft = Some(6000.0);

        let plan = plan_flight(
            &[KSFO, KMRY],
            &profile,
            None,
            Some(KSFO_ELEV),
            Some(KMRY_ELEV),
            2026.5,
        );
        // Halfway between the 4,000 and 8,000 ft rows, not the scalar 110.
        assert_eq!(plan.cruise_tas_source, ValueSource::Table);
        assert!((plan.cruise_tas_kt - 110.0).abs() < 1e-9);
        assert_eq!(plan.cruise_fuel_source, ValueSource::Table);
        assert!((plan.cruise_fuel_gph - 7.9).abs() < 1e-9);

        // And it really drove the nav log: every leg is flown at it.
        assert!((plan.legs[0].ground_speed_kt - 110.0).abs() < 1e-9);

        // At 8,000 ft the same table gives different numbers — proving
        // the lookup is altitude-dependent rather than incidental.
        profile.cruise_altitude_ft = Some(8000.0);
        let higher = plan_flight(
            &[KSFO, KMRY],
            &profile,
            None,
            Some(KSFO_ELEV),
            Some(KMRY_ELEV),
            2026.5,
        );
        assert!((higher.cruise_tas_kt - 112.0).abs() < 1e-9);
        assert!((higher.cruise_fuel_gph - 7.6).abs() < 1e-9);
    }

    #[test]
    fn scalars_are_used_when_no_table_applies() {
        let mut profile = base_profile();
        // A table that exists but records two power settings with no
        // selection is ambiguous, so it must not be guessed at.
        let mut ambiguous = cruise_table();
        ambiguous.cruise.push(PerformancePoint {
            pressure_altitude_ft: 4000.0,
            tas_kt: 118.0,
            fuel_gph: 10.5,
            vertical_speed_fpm: None,
            power_setting: "75%".into(),
        });
        profile.performance = Some(ambiguous);

        let plan = plan_flight(
            &[KSFO, KMRY],
            &profile,
            None,
            Some(KSFO_ELEV),
            Some(KMRY_ELEV),
            2026.5,
        );
        assert_eq!(plan.cruise_tas_source, ValueSource::Scalar);
        assert!((plan.cruise_tas_kt - 110.0).abs() < 1e-9);

        // Naming the setting resolves it.
        let mut chosen = profile.clone();
        chosen.cruise_power_setting = Some("75%".into());
        chosen.cruise_altitude_ft = Some(4000.0);
        let plan = plan_flight(
            &[KSFO, KMRY],
            &chosen,
            None,
            Some(KSFO_ELEV),
            Some(KMRY_ELEV),
            2026.5,
        );
        assert_eq!(plan.cruise_tas_source, ValueSource::Table);
        assert!((plan.cruise_tas_kt - 118.0).abs() < 1e-9);
    }

    #[test]
    fn without_a_vertical_profile_fuel_falls_back_and_says_so() {
        let mut profile = base_profile();
        profile.cruise_altitude_ft = None; // no vertical profile possible

        let plan = plan_flight(&[KSFO, KMRY], &profile, None, Some(KSFO_ELEV), None, 2026.5);
        assert!(plan.vertical.is_none());
        assert!(!plan.fuel.phase_aware, "the fallback was not flagged");
        assert_eq!(plan.fuel.climb_gal, 0.0);
        // Everything charged at cruise burn, plus taxi — §9.3's behaviour.
        assert!((plan.fuel.cruise_gal - plan.total_ete_hours * 7.9).abs() < 1e-9);
        assert!((plan.fuel.trip_gal - (1.4 + plan.total_ete_hours * 7.9)).abs() < 1e-9);
    }

    #[test]
    fn a_flight_beyond_the_tanks_is_flagged() {
        let mut profile = base_profile();
        profile.fuel_capacity_gal = Some(10.0); // absurdly small
        let plan = plan_flight(
            &[KSFO, KMRY],
            &profile,
            None,
            Some(KSFO_ELEV),
            Some(KMRY_ELEV),
            2026.5,
        );
        assert_eq!(plan.fuel.within_capacity, Some(false));
        assert!(plan.fuel.required_gal > 10.0);

        // No capacity recorded means no judgement, not a false pass.
        profile.fuel_capacity_gal = None;
        let plan = plan_flight(
            &[KSFO, KMRY],
            &profile,
            None,
            Some(KSFO_ELEV),
            Some(KMRY_ELEV),
            2026.5,
        );
        assert_eq!(plan.fuel.within_capacity, None);
    }

    #[test]
    fn climb_and_descent_rates_can_come_from_the_tables() {
        let mut profile = base_profile();
        // Strip the scalars so only the table can supply them.
        profile.climb_rate_fpm = None;
        profile.climb_tas_kt = None;
        profile.climb_fuel_gph = None;
        profile.performance = Some(AircraftPerformance {
            climb: vec![
                PerformancePoint {
                    pressure_altitude_ft: 0.0,
                    tas_kt: 76.0,
                    fuel_gph: 11.0,
                    vertical_speed_fpm: Some(730.0),
                    power_setting: String::new(),
                },
                PerformancePoint {
                    pressure_altitude_ft: 8000.0,
                    tas_kt: 74.0,
                    fuel_gph: 10.0,
                    vertical_speed_fpm: Some(500.0),
                    power_setting: String::new(),
                },
            ],
            ..Default::default()
        });

        let plan = plan_flight(
            &[KSFO, KMRY],
            &profile,
            None,
            Some(KSFO_ELEV),
            Some(KMRY_ELEV),
            2026.5,
        );
        let vertical = plan.vertical.expect("a climb rate came from the table");
        let toc = vertical.top_of_climb.expect("top of climb");
        // Rate at the climb's midpoint (~3,250 ft): 730 - (500-730)*... ≈ 636 fpm.
        let expected_min = (6500.0 - KSFO_ELEV) / 636.0;
        assert!(
            (toc.time_min - expected_min).abs() < 0.5,
            "{} min vs expected ~{expected_min}",
            toc.time_min
        );
        assert!(plan.fuel.climb_gal > 0.0);
    }
}
