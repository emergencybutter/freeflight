// Thin wrapper around the generated wasm-bindgen glue (src/wasm/, built
// by `npm run build:wasm` — see package.json's predev/prebuild hooks)
// so the rest of the app never imports the raw generated module
// directly. `ff-wasm` exposes `ff-planning`'s nav-log and weight &
// balance math (DESIGN.md §5) as JSON-in/JSON-out functions; the types
// below mirror the Rust structs on the other side of that boundary
// one-to-one (see crates/ff-planning/src/route.rs, weight_balance.rs).
import init, { check_weight_balance_json, distance_nm, plan_route_json } from "../wasm/ff_wasm.js";

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
  /** Client-side only — picks which winds-aloft altitude level applies
   * per leg (see planning/windsAloft.ts); ff-planning's Rust
   * AircraftProfile has no such field and silently ignores it (no
   * `#[serde(deny_unknown_fields)]`), since it never affects the
   * wind-triangle math itself, only which real wind gets fed into it. */
  cruise_altitude_ft: number | null;
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
