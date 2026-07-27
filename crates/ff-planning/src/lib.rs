//! Route/nav-log math, wind triangle, and basic weight & balance
//! (DESIGN.md §5, §9.3).

pub mod flight;
pub mod geo;
pub mod magvar;
pub mod performance;
pub mod route;
pub mod vertical;
pub mod weight_balance;
pub mod wind;

pub use flight::{plan_flight, FlightPlanSummary, FuelSummary, ValueSource};
pub use geo::{distance_nm, initial_bearing_deg, intermediate_point};
pub use magvar::declination_deg;
pub use performance::{AircraftPerformance, PerformancePoint};
pub use route::{
    plan_leg, plan_route, AircraftProfile, RouteLegPlan, RoutePlanSummary, RoutePoint,
};
pub use vertical::{plan_vertical, VerticalPoint, VerticalProfile};
pub use weight_balance::{
    check as check_weight_balance, WeightAtStation, WeightBalanceEnvelope, WeightBalanceResult,
};
pub use wind::{solve as solve_wind_triangle, Wind, WindTriangleResult};
