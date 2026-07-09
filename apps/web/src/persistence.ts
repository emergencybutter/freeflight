// Session persistence for the flight plan. The web client still has no
// server-side/account storage (DESIGN.md §8 — cross-device sync is Phase
// 4), but keeping the current plan in localStorage means a refresh, an
// accidental tab close, or a phone locking the browser no longer wipes
// the route and aircraft profile you were part-way through building.
//
// Best-effort throughout: localStorage can be unavailable (private mode,
// disabled) or hold stale/incompatible JSON from an older build, so every
// read falls back to defaults and every write is swallowed on failure —
// persistence never blocks the app.
import type { AircraftProfile } from "./planning/wasm";
import type { RouteState } from "./types";

// Versioned key: bump the suffix if the persisted shape ever changes in a
// way old data can't satisfy, so a stale blob is simply ignored rather
// than half-restored.
const STORAGE_KEY = "freeflight.plan.v1";

export interface PersistedPlan {
  route?: RouteState;
  profile?: AircraftProfile;
  userWaypointCount?: number;
}

export function loadPlan(): PersistedPlan {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw) as PersistedPlan;
    return parsed && typeof parsed === "object" ? parsed : {};
  } catch {
    return {};
  }
}

export function savePlan(plan: PersistedPlan): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(plan));
  } catch {
    // Storage full or unavailable — persistence is best-effort.
  }
}
