// Picks which real winds-aloft station/altitude applies to each route
// leg — the actual "simple wind correction" DESIGN.md §9.3 describes.
// Nothing here touches ff-planning's wind-triangle math (that stays in
// wasm.ts/ff-wasm); this only resolves a bulletin reading into the
// {direction_true_deg, speed_kt} shape that math expects.
import type { RouteWaypoint, WindsAloftBulletin, StationWindsAloft } from "../types";
import { distanceNm, type PlanningWind } from "./wasm";

/** Nearest reporting station to `lat`/`lon` among stations the bulletin
 * resolved a coordinate for (see the ff-api enrichment in
 * services/ff-api/src/routes/weather.rs) — stations are sparse (176
 * nationwide vs. ~13k airports), so this is never an exact match, just
 * whichever is closest; winds aloft are a regional-scale forecast, so
 * no distance cap. */
async function nearestStation(lat: number, lon: number, stations: StationWindsAloft[]): Promise<StationWindsAloft | null> {
  let best: StationWindsAloft | null = null;
  let bestDistance = Infinity;
  for (const station of stations) {
    if (station.lat === null || station.lon === null) continue;
    const distance = await distanceNm(lat, lon, station.lat, station.lon);
    if (distance < bestDistance) {
      bestDistance = distance;
      best = station;
    }
  }
  return best;
}

/** The wind for one leg: nearest station to its midpoint, nearest
 * altitude *level* within that station (no interpolation between
 * levels — DESIGN.md §9.3 scopes this as simple, not a certified
 * forecast tool). `null` covers every "no correction for this leg"
 * case uniformly — no station within reach, no levels at all, or a
 * "light and variable" reading (under 5kt, not worth correcting for) —
 * matching `ff-planning::plan_route`'s existing per-leg `None`
 * fallback (heading = course, groundspeed = TAS). */
export async function legWind(
  from: RouteWaypoint,
  to: RouteWaypoint,
  altitudeFt: number,
  bulletin: WindsAloftBulletin,
): Promise<PlanningWind | null> {
  const midLat = (from.lat + to.lat) / 2;
  const midLon = (from.lon + to.lon) / 2;
  const station = await nearestStation(midLat, midLon, bulletin.stations);
  if (!station || station.levels.length === 0) return null;

  let nearestLevel = station.levels[0];
  for (const level of station.levels) {
    if (Math.abs(level.altitude_ft - altitudeFt) < Math.abs(nearestLevel.altitude_ft - altitudeFt)) {
      nearestLevel = level;
    }
  }

  if (nearestLevel.wind === "LightAndVariable") return null;
  return { direction_true_deg: nearestLevel.wind.Directional.direction_deg, speed_kt: nearestLevel.wind.Directional.speed_kt };
}

/** One entry per leg (`points.length - 1`) — see `legWind`. */
export async function windsForRoute(
  points: RouteWaypoint[],
  altitudeFt: number,
  bulletin: WindsAloftBulletin,
): Promise<(PlanningWind | null)[]> {
  const winds: (PlanningWind | null)[] = [];
  for (let i = 0; i < points.length - 1; i++) {
    winds.push(await legWind(points[i], points[i + 1], altitudeFt, bulletin));
  }
  return winds;
}
