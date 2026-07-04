// Thin wrapper around the generated wasm-bindgen glue (src/wasm/, built
// by `npm run build:wasm` — see package.json's predev/prebuild hooks)
// so the rest of the app never imports the raw generated module
// directly. `ff-wasm` exposes `ff-planning`'s nav-log and weight &
// balance math (DESIGN.md §5) as JSON-in/JSON-out functions; the types
// below mirror the Rust structs on the other side of that boundary
// one-to-one (see crates/ff-planning/src/route.rs, weight_balance.rs).
import init, { check_weight_balance_json, plan_route_json } from "../wasm/ff_wasm.js";

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
}

export interface RouteLegPlan {
  distance_nm: number;
  true_course_deg: number;
  true_heading_deg: number;
  ground_speed_kt: number;
  ete_hours: number;
  fuel_gal: number;
}

export interface RoutePlanSummary {
  legs: RouteLegPlan[];
  total_distance_nm: number;
  total_ete_hours: number;
  total_fuel_gal: number;
}

export async function planRoute(points: RoutePoint[], profile: AircraftProfile): Promise<RoutePlanSummary> {
  await ensureReady();
  const json = plan_route_json(JSON.stringify(points), JSON.stringify(profile));
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
