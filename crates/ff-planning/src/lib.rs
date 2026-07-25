//! Route/nav-log math, wind triangle, and basic weight & balance
//! (DESIGN.md §5, §9.3).

pub mod geo;
pub mod magvar;
pub mod route;
pub mod weight_balance;
pub mod wind;

pub use geo::{distance_nm, initial_bearing_deg};
pub use magvar::declination_deg;
pub use route::{
    plan_leg, plan_route, AircraftProfile, RouteLegPlan, RoutePlanSummary, RoutePoint,
};
pub use weight_balance::{
    check as check_weight_balance, WeightAtStation, WeightBalanceEnvelope, WeightBalanceResult,
};
pub use wind::{solve as solve_wind_triangle, Wind, WindTriangleResult};
