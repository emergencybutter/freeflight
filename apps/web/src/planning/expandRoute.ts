// Expands the route builder's reorderable middle list (fixes and
// airways strung between the departure/SID and STAR/arrival — see
// App.tsx) into the flat point sequence the nav log and map actually
// consume. Pure function — no fetching; airway tokens carry their
// AirwayDetail from insert time (see planning/procedureLookup.ts for
// how the SID/STAR endpoints App.tsx concatenates around this are
// resolved instead).
import type { RouteToken, RouteWaypoint } from "../types";

export interface ExpandedRoute {
  points: RouteWaypoint[];
  /** One human-readable warning per airway token that couldn't expand
   * (missing neighbor, neighbor not on the airway, adjacent airways). */
  warnings: string[];
}

/** An airway token expands to the fixes strictly *between* its entry
 * fix (the previous point) and exit fix (the next point), in
 * entry→exit order along the airway — reversed traversal falls out
 * naturally when the exit fix sits earlier in the airway's sequence.
 * `before`/`after` stand in for whatever comes immediately outside the
 * middle list itself (the departure/SID's last point, or the STAR's
 * first point/arrival) so an airway placed at either edge still
 * resolves against it, exactly as if it were another token here. A
 * token whose neighbors don't pin it down (list edge with no boundary
 * point, neighbor not on the airway, two airways back-to-back) expands
 * to nothing and contributes a warning instead; the neighbors
 * themselves still render, so a half-built route stays usable while
 * it's being typed. */
export function expandRoute(tokens: RouteToken[], before: RouteWaypoint | null, after: RouteWaypoint | null): ExpandedRoute {
  const points: RouteWaypoint[] = [];
  const warnings: string[] = [];

  for (let i = 0; i < tokens.length; i++) {
    const token = tokens[i];

    if (token.kind === "point") {
      points.push(token.point);
      continue;
    }

    const prevToken = tokens[i - 1];
    const nextToken = tokens[i + 1];
    const prevPoint = prevToken?.kind === "point" ? prevToken.point : i === 0 ? before : null;
    const nextPoint = nextToken?.kind === "point" ? nextToken.point : i === tokens.length - 1 ? after : null;
    if (!prevPoint || !nextPoint) {
      warnings.push(`${token.ident}: needs a fix before and after it to know where to join and leave`);
      continue;
    }

    const legFixes = token.detail.legs.map((l) => l.fix_ident);
    const entryIndex = legFixes.indexOf(prevPoint.ident);
    const exitIndex = legFixes.indexOf(nextPoint.ident);
    if (entryIndex === -1 || exitIndex === -1) {
      const missing = entryIndex === -1 ? prevPoint.ident : nextPoint.ident;
      warnings.push(`${token.ident}: ${missing} is not on this airway`);
      continue;
    }

    const forward = entryIndex <= exitIndex;
    const [from, to] = forward ? [entryIndex + 1, exitIndex] : [exitIndex + 1, entryIndex];
    const between = token.detail.legs.slice(from, to);
    if (!forward) between.reverse();

    for (const leg of between) {
      const coord = token.detail.fixes[leg.fix_ident];
      // Unresolved fixes (no waypoint/navaid row in the bundle) are
      // skipped, same tolerance procedure rendering has.
      if (!coord) continue;
      points.push({ ident: leg.fix_ident, name: null, lat: coord.lat, lon: coord.lon });
    }
  }

  return { points, warnings };
}
