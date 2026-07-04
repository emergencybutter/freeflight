// Expands a token route (flight-plan-string style: `KSFO FOO V123 BAR
// KLAX`) into the flat point sequence the nav log and map actually
// consume. Pure function — no fetching; airway tokens carry their
// AirwayDetail from insert time.
import type { RouteToken, RouteWaypoint } from "../types";

export interface ExpandedRoute {
  points: RouteWaypoint[];
  /** One human-readable warning per airway token that couldn't expand
   * (missing neighbor, neighbor not on the airway, adjacent airways). */
  warnings: string[];
}

/** An airway token expands to the fixes strictly *between* its entry
 * fix (the previous point token) and exit fix (the next point token),
 * in entry→exit order along the airway — reversed traversal falls out
 * naturally when the exit fix sits earlier in the airway's sequence.
 * A token whose neighbors don't pin it down (route edge, neighbor not
 * on the airway, two airways back-to-back) expands to nothing and
 * contributes a warning instead; the neighbors themselves still render,
 * so a half-built route stays usable while it's being typed. */
export function expandRoute(tokens: RouteToken[]): ExpandedRoute {
  const points: RouteWaypoint[] = [];
  const warnings: string[] = [];

  for (let i = 0; i < tokens.length; i++) {
    const token = tokens[i];
    if (token.kind === "point") {
      points.push(token.point);
      continue;
    }

    const prev = tokens[i - 1];
    const next = tokens[i + 1];
    if (prev?.kind !== "point" || next?.kind !== "point") {
      warnings.push(`${token.ident}: needs a fix before and after it to know where to join and leave`);
      continue;
    }

    const legFixes = token.detail.legs.map((l) => l.fix_ident);
    const entryIndex = legFixes.indexOf(prev.point.ident);
    const exitIndex = legFixes.indexOf(next.point.ident);
    if (entryIndex === -1 || exitIndex === -1) {
      const missing = entryIndex === -1 ? prev.point.ident : next.point.ident;
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
