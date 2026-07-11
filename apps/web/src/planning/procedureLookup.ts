// SID/STAR lookup for the route builder's explicit "Set SID"/"Set STAR"
// pickers (departure/arrival airports are chosen first, so which
// airport's procedures to browse is always already known — no text
// parsing or neighbor-guessing needed, unlike an airway ident).
import { fetchAirportProcedures } from "../data";
import type { Procedure, ProcedureDetail, ProcedureLeg, ProcedureTransitionDetail, ResolvedProcedureRef, RouteWaypoint } from "../types";

/** The airport's SIDs or STARs, for the first picker step. */
export async function fetchProcedureOptions(airportIcao: string, kind: "SID" | "STAR"): Promise<Procedure[]> {
  const all = await fetchAirportProcedures(airportIcao);
  return all.filter((p) => p.kind === kind);
}

/** The procedure's named enroute transitions, for the second picker
 * step — real transitions can be a runway-specific label (e.g. HOPEA3's
 * single "RW22") as much as a named enroute fix (e.g. MIP4's "PSB"),
 * there's no way to tell apart from the ident alone, so all of them are
 * offered. */
export function transitionOptions(detail: ProcedureDetail): ProcedureTransitionDetail[] {
  return detail.transitions.filter((t) => t.kind === "ENROUTE");
}

function legsToPoints(legs: ProcedureLeg[], fixes: ProcedureDetail["fixes"]): RouteWaypoint[] {
  const points: RouteWaypoint[] = [];
  for (const leg of legs) {
    if (!leg.fix_ident) continue;
    const coord = fixes[leg.fix_ident];
    if (!coord) continue;
    points.push({ ident: leg.fix_ident, name: null, lat: coord.lat, lon: coord.lon });
  }
  return points;
}

/** The segment that continues on from `primary` toward the airport
 * (STAR) or away from it (SID's runway/common departure legs) — a
 * literal `COMMON`-kind transition if the procedure has one, else
 * whichever other enroute transition picks up at `primary`'s far end
 * (matching real data like MIP4's STAR, which has no COMMON transition
 * at all — its continuation is itself named "ALL", chained purely by
 * MIP being both PSB's last fix and ALL's first). Returns legs with
 * the shared fix removed so callers don't have to dedupe a seam. */
function findContinuation(
  transitions: ProcedureTransitionDetail[],
  primary: ProcedureTransitionDetail,
  end: "before" | "after",
): ProcedureLeg[] {
  const common = transitions.find((t) => t.kind === "COMMON");
  if (common) return common.legs;

  const seam = end === "after" ? primary.legs[primary.legs.length - 1]?.fix_ident : primary.legs[0]?.fix_ident;
  if (!seam) return [];
  for (const t of transitions.filter((tr) => tr.kind === "ENROUTE" && tr.id !== primary.id)) {
    if (end === "after" && t.legs[0]?.fix_ident === seam) return t.legs.slice(1);
    if (end === "before" && t.legs[t.legs.length - 1]?.fix_ident === seam) return t.legs.slice(0, -1);
  }
  return [];
}

/** Builds the final ordered point sequence for a chosen SID/STAR +
 * transition, from an already-fetched `ProcedureDetail` (the picker UI
 * fetches it once to populate the transition list, then passes it back
 * here rather than re-fetching). STAR order is transition-then-
 * continuation (arrive via the transition, then the core/common legs
 * funnel to the airport); SID order is continuation-then-transition
 * (depart via the runway/common legs, then the transition heads out). */
export function buildResolvedProcedure(
  airportIcao: string,
  kind: "SID" | "STAR",
  detail: ProcedureDetail,
  transitionId: string,
): ResolvedProcedureRef | null {
  const primary = detail.transitions.find((t) => t.id === transitionId);
  if (!primary) return null;

  const points =
    kind === "STAR"
      ? [
          ...legsToPoints(primary.legs, detail.fixes),
          ...legsToPoints(findContinuation(detail.transitions, primary, "after"), detail.fixes),
        ]
      : [
          ...legsToPoints(findContinuation(detail.transitions, primary, "before"), detail.fixes),
          ...legsToPoints(primary.legs, detail.fixes),
        ];

  return {
    airportIcao,
    kind,
    procedureIdent: detail.ident,
    transitionIdent: primary.ident,
    points,
    chartUrl: detail.chart_url,
  };
}
