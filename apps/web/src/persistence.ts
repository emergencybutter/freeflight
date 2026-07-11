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

// The map's own view state (camera, base chart, overlay toggles), in its
// own key/blob separate from the flight plan above — different shape,
// different write cadence (every moveend vs. explicit plan edits), so a
// version bump to one never has to touch the other. Added after an iPad
// bug report: iOS Safari force-reloads the tab when it hits the per-tab
// memory ceiling, and before this every reload silently reset the map to
// its defaults — persisting the view makes *any* reload (crash, refresh,
// tab eviction) land the user back where they were.
const MAP_VIEW_KEY = "freeflight.mapview.v1";

export interface PersistedMapView {
  center?: { lat: number; lon: number };
  zoom?: number;
  /** null = "No chart" was explicitly selected; absent = never saved. */
  chartKind?: string | null;
  overlays?: {
    airspace?: boolean;
    weatherHazards?: boolean;
    airports?: boolean;
    pireps?: boolean;
    cwas?: boolean;
  };
}

export function loadMapView(): PersistedMapView {
  try {
    const raw = localStorage.getItem(MAP_VIEW_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw) as PersistedMapView;
    return parsed && typeof parsed === "object" ? parsed : {};
  } catch {
    return {};
  }
}

/** Read-modify-write merge, so the camera saver (moveend) and the
 * chart/overlay saver (toggle changes) can each update just their slice
 * without clobbering the other's last write. */
export function saveMapView(partial: PersistedMapView): void {
  try {
    localStorage.setItem(MAP_VIEW_KEY, JSON.stringify({ ...loadMapView(), ...partial }));
  } catch {
    // Storage full or unavailable — persistence is best-effort.
  }
}
