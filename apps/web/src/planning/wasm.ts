// Thin wrapper around the generated wasm-bindgen glue (src/wasm/, built
// by `npm run build:wasm` — see package.json's predev/prebuild hooks)
// so the rest of the app never imports the raw generated module
// directly. `ff-wasm` exposes `ff-planning`'s nav-log and weight &
// balance math (DESIGN.md §5) as JSON-in/JSON-out functions; the types
// below mirror the Rust structs on the other side of that boundary
// one-to-one (see crates/ff-planning/src/route.rs, weight_balance.rs).
import init, {
  check_weight_balance_json,
  distance_nm,
  plan_flight_json,
  plan_route_json,
  plan_vertical_json,
} from "../wasm/ff_wasm.js";

let ready: Promise<unknown> | null = null;

/** Loads the wasm binary — safe to call more than once, does the actual
 * work only the first time. Every exported function below awaits this,
 * so callers never have to think about init order. */
function ensureReady(): Promise<unknown> {
  if (!ready) ready = init();
  return ready;
}

export interface RoutePoint {
  lat: number;
  lon: number;
}

export interface AircraftProfile {
  name: string;
  cruise_tas_kt: number;
  fuel_burn_gph: number;
  max_gross_weight_lb: number | null;
  forward_cg_limit_in: number | null;
  aft_cg_limit_in: number | null;
  /** Picks which winds-aloft altitude level applies per leg (see
   * planning/windsAloft.ts) — it never affects the wind-triangle math
   * itself, only which real wind gets fed into it — and is the altitude
   * the vertical profile (`planVertical`) climbs to and descends from. */
  cruise_altitude_ft: number | null;
  /** Climb/descent performance, for the vertical profile only. A missing
   * *rate* means that end has no top of climb/descent; a missing TAS
   * falls back to `cruise_tas_kt` (see ff-planning's AircraftProfile). */
  climb_rate_fpm: number | null;
  climb_tas_kt: number | null;
  descent_rate_fpm: number | null;
  descent_tas_kt: number | null;
  /** Per-phase burn and fuel policy, for `planFlight`'s phase-aware
   * total. All optional — absent falls back to `fuel_burn_gph`. */
  climb_fuel_gph?: number | null;
  descent_fuel_gph?: number | null;
  taxi_fuel_gal?: number | null;
  fuel_capacity_gal?: number | null;
  reserve_minutes?: number | null;
  /** Real POH tables from a saved aircraft (DESIGN.md §9.5.6). When they
   * cover the planned altitude they win over the scalars above. */
  performance?: AircraftPerformance | null;
  /** Which cruise power setting to plan at, when the table records more
   * than one. Without it an ambiguous table is not guessed at — the
   * scalars are used instead. */
  cruise_power_setting?: string | null;
}

/** One row of a performance table, as ff-planning reads it. */
export interface PerformancePoint {
  pressure_altitude_ft: number;
  tas_kt: number;
  fuel_gph: number;
  vertical_speed_fpm: number | null;
  power_setting: string;
}

export interface AircraftPerformance {
  climb: PerformancePoint[];
  cruise: PerformancePoint[];
  descent: PerformancePoint[];
}

/** Where a planning figure came from, so the UI can distinguish a number
 * interpolated from the pilot's own table from a typed-in fallback. */
export type ValueSource = "table" | "scalar";

export interface FuelSummary {
  taxi_gal: number;
  climb_gal: number;
  cruise_gal: number;
  descent_gal: number;
  /** taxi + climb + cruise + descent. */
  trip_gal: number;
  reserve_gal: number;
  /** What must be in the tanks: trip + reserve. */
  required_gal: number;
  capacity_gal: number | null;
  within_capacity: boolean | null;
  /** The phase times used — they come from the vertical profile, not the
   * nav log, so a client can show both rather than hide the difference. */
  climb_minutes: number;
  cruise_hours: number;
  descent_minutes: number;
  /** False when there was no vertical profile and the whole flight was
   * charged at cruise burn. */
  phase_aware: boolean;
}

/** Nav log, vertical profile and fuel from one pass — see `planFlight`. */
export interface FlightPlanSummary {
  legs: RouteLegPlan[];
  total_distance_nm: number;
  total_ete_hours: number;
  vertical: VerticalProfile | null;
  fuel: FuelSummary;
  cruise_tas_kt: number;
  cruise_tas_source: ValueSource;
  cruise_fuel_gph: number;
  cruise_fuel_source: ValueSource;
}

/** Plan the whole flight in one pass: legs, top of climb/descent, and a
 * phase-aware fuel total.
 *
 * Supersedes calling `planRoute` and `planVertical` separately — fuel can
 * only be decomposed by phase if one pass knows both the legs and the
 * climb/descent times, and cruise TAS is looked up from the profile's
 * performance tables here rather than taken from its scalar field. */
export async function planFlight(
  points: RoutePoint[],
  profile: AircraftProfile,
  winds: (PlanningWind | null)[],
  departureElevationFt: number | null,
  arrivalElevationFt: number | null,
): Promise<FlightPlanSummary> {
  await ensureReady();
  const json = plan_flight_json(
    JSON.stringify(points),
    JSON.stringify(profile),
    JSON.stringify(winds),
    departureElevationFt ?? NaN,
    arrivalElevationFt ?? NaN,
    currentDecimalYear(),
  );
  return JSON.parse(json) as FlightPlanSummary;
}

export interface PlanningWind {
  direction_true_deg: number;
  speed_kt: number;
}

export async function distanceNm(lat1: number, lon1: number, lat2: number, lon2: number): Promise<number> {
  await ensureReady();
  return distance_nm(lat1, lon1, lat2, lon2);
}

export interface RouteLegPlan {
  distance_nm: number;
  true_course_deg: number;
  true_heading_deg: number;
  /** WMM magnetic declination at the leg midpoint, +East. */
  magnetic_variation_deg: number;
  /** True course/heading converted to magnetic (what the pilot flies). */
  magnetic_course_deg: number;
  magnetic_heading_deg: number;
  ground_speed_kt: number;
  ete_hours: number;
  fuel_gal: number;
}

/** Current date as a decimal year (e.g. 2026.54) for the magnetic model. */
function currentDecimalYear(): number {
  const now = new Date();
  const y = now.getUTCFullYear();
  const start = Date.UTC(y, 0, 1);
  const end = Date.UTC(y + 1, 0, 1);
  return y + (now.getTime() - start) / (end - start);
}

export interface RoutePlanSummary {
  legs: RouteLegPlan[];
  total_distance_nm: number;
  total_ete_hours: number;
  total_fuel_gal: number;
}

/** `winds` is one entry per leg (`points.length - 1`), `null` where no
 * wind data applies (falls back to no-wind for that leg) — see
 * planning/windsAloft.ts for how these get resolved from a real
 * winds-aloft bulletin. */
export async function planRoute(
  points: RoutePoint[],
  profile: AircraftProfile,
  winds: (PlanningWind | null)[],
): Promise<RoutePlanSummary> {
  await ensureReady();
  const json = plan_route_json(
    JSON.stringify(points),
    JSON.stringify(profile),
    JSON.stringify(winds),
    currentDecimalYear(),
  );
  return JSON.parse(json) as RoutePlanSummary;
}

/** Top of climb or top of descent — a point on the route, not just a
 * distance, so it can be drawn on the map. */
export interface VerticalPoint {
  distance_from_departure_nm: number;
  distance_to_arrival_nm: number;
  lat: number;
  lon: number;
  /** Index of the leg it falls on, and how far along that leg (0-1). */
  leg_index: number;
  leg_fraction: number;
  altitude_ft: number;
  /** Minutes spent climbing to this point, or descending from it. */
  time_min: number;
}

export interface VerticalProfile {
  top_of_climb: VerticalPoint | null;
  top_of_descent: VerticalPoint | null;
  cruise_altitude_ft: number;
  /** False when the route is too short to reach the cruise altitude —
   * both points then sit on the single crossover point, at
   * `peak_altitude_ft`. */
  cruise_reached: boolean;
  peak_altitude_ft: number;
  cruise_distance_nm: number;
  total_distance_nm: number;
}

/** Top of climb / top of descent for the route, at the profile's cruise
 * altitude and climb/descent performance, using the same per-leg winds
 * the nav log does. `departureElevationFt`/`arrivalElevationFt` are the
 * real field elevations — pass `null` for an airport whose elevation
 * isn't known and that end is left out rather than assumed to be at sea
 * level. Resolves to `null` when there's nothing to compute (no cruise
 * altitude set, no elevation/rate pair, fewer than two points). */
export async function planVertical(
  points: RoutePoint[],
  profile: AircraftProfile,
  winds: (PlanningWind | null)[],
  departureElevationFt: number | null,
  arrivalElevationFt: number | null,
): Promise<VerticalProfile | null> {
  await ensureReady();
  // NaN is the "unknown" carrier for the two elevations — the binding
  // takes plain f64s, which can't be null (see ff-wasm's lib.rs).
  const json = plan_vertical_json(
    JSON.stringify(points),
    JSON.stringify(profile),
    JSON.stringify(winds),
    departureElevationFt ?? NaN,
    arrivalElevationFt ?? NaN,
  );
  return JSON.parse(json) as VerticalProfile | null;
}

export interface WeightAtStation {
  weight_lb: number;
  arm_in: number;
}

export interface WeightBalanceEnvelope {
  max_gross_weight_lb: number;
  forward_cg_limit_in: number;
  aft_cg_limit_in: number;
}

export interface WeightBalanceResult {
  total_weight_lb: number;
  cg_in: number;
  within_weight_limit: boolean;
  within_cg_limits: boolean;
}

export async function checkWeightBalance(
  items: WeightAtStation[],
  envelope: WeightBalanceEnvelope,
): Promise<WeightBalanceResult> {
  await ensureReady();
  const json = check_weight_balance_json(JSON.stringify(items), JSON.stringify(envelope));
  return JSON.parse(json) as WeightBalanceResult;
}
