// Encodes/decodes a shareable link for the current flight plan + map view
// (App.tsx's Share button, MapView's chart-kind/center/zoom). Everything
// goes into one `?p=` query param as base64url-encoded JSON — a single
// opaque blob rather than a param per field, so there's no ad-hoc
// delimiter/escaping scheme to get wrong for idents or airport names that
// might contain unusual characters.
//
// SID/STAR are shared as their identifying triple (airport/procedure/
// transition), not the full expanded point list — re-resolving through
// the same fetchProcedureOptions -> fetchProcedureDetail ->
// buildResolvedProcedure path a manual "Set SID" pick already uses keeps
// the link short and guarantees the restored route is exactly what
// picking it by hand would produce (including chartUrl), rather than a
// second, parallel serialization of ResolvedProcedureRef to keep in sync.
import { fetchAirwayDetail, fetchProcedureDetail } from "./data";
import { buildResolvedProcedure, fetchProcedureOptions } from "./planning/procedureLookup";
import type { RouteState, RouteToken, RouteWaypoint } from "./types";

const QUERY_PARAM = "p";

interface SharedProcedureRef {
  airportIcao: string;
  kind: "SID" | "STAR";
  procedureIdent: string;
  transitionIdent: string;
}

type SharedRouteToken =
  | { kind: "point"; point: RouteWaypoint }
  | { kind: "airway"; ident: string };

interface SharedRoute {
  departure: RouteWaypoint | null;
  arrival: RouteWaypoint | null;
  sid: SharedProcedureRef | null;
  star: SharedProcedureRef | null;
  middleTokens: SharedRouteToken[];
}

export interface SharedMapView {
  /** Null means "no chart" was explicitly selected — distinct from the
   * param being absent entirely, which leaves MapView's own default. */
  chartKind: string | null;
  center: { lat: number; lon: number };
  zoom: number;
}

export interface SharedPlan {
  route: SharedRoute;
  map: SharedMapView;
}

function toBase64Url(json: string): string {
  const bytes = new TextEncoder().encode(json);
  let binary = "";
  for (const b of bytes) binary += String.fromCharCode(b);
  return btoa(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

function fromBase64Url(encoded: string): string {
  const b64 = encoded.replace(/-/g, "+").replace(/_/g, "/");
  const padded = b64 + "=".repeat((4 - (b64.length % 4)) % 4);
  const binary = atob(padded);
  const bytes = Uint8Array.from(binary, (c) => c.charCodeAt(0));
  return new TextDecoder().decode(bytes);
}

function toSharedToken(token: RouteToken): SharedRouteToken {
  return token.kind === "point" ? { kind: "point", point: token.point } : { kind: "airway", ident: token.ident };
}

/** Builds the shareable URL for the current route + map view, against the
 * page's own origin/path so it works the same whether the app is served
 * from a dev server or the production domain. */
export function buildShareUrl(route: RouteState, map: SharedMapView): string {
  const plan: SharedPlan = {
    route: {
      departure: route.departure,
      arrival: route.arrival,
      sid: route.sid && {
        airportIcao: route.sid.airportIcao,
        kind: route.sid.kind,
        procedureIdent: route.sid.procedureIdent,
        transitionIdent: route.sid.transitionIdent,
      },
      star: route.star && {
        airportIcao: route.star.airportIcao,
        kind: route.star.kind,
        procedureIdent: route.star.procedureIdent,
        transitionIdent: route.star.transitionIdent,
      },
      middleTokens: route.middleTokens.map(toSharedToken),
    },
    map,
  };
  const url = new URL(window.location.href);
  url.search = "";
  url.hash = "";
  url.searchParams.set(QUERY_PARAM, toBase64Url(JSON.stringify(plan)));
  return url.toString();
}

/** Reads and decodes the `?p=` param from the current page URL, if any.
 * Never throws — a missing, stale, or malformed param just means "no
 * shared plan", not a broken page load. */
export function readSharedPlanFromUrl(): SharedPlan | null {
  const raw = new URLSearchParams(window.location.search).get(QUERY_PARAM);
  if (!raw) return null;
  try {
    const parsed: unknown = JSON.parse(fromBase64Url(raw));
    if (parsed && typeof parsed === "object" && "route" in parsed && "map" in parsed) {
      return parsed as SharedPlan;
    }
    return null;
  } catch {
    return null;
  }
}

/** Removes the `?p=` param from the visible URL without a navigation/
 * reload, once its plan has been loaded — keeps the address bar clean
 * and stops a page refresh from silently re-applying a stale shared plan
 * over whatever the user has since changed. */
export function clearSharedPlanFromUrl(): void {
  const url = new URL(window.location.href);
  url.searchParams.delete(QUERY_PARAM);
  window.history.replaceState(null, "", url.toString());
}

/** Resolves a decoded [`SharedRoute`] back into a real `RouteState` —
 * re-fetches whatever a manual pick would have (SID/STAR detail, airway
 * detail); a token that no longer resolves (a since-removed procedure,
 * cycle changed) is dropped rather than failing the whole restore. */
export async function hydrateSharedRoute(shared: SharedRoute): Promise<RouteState> {
  const resolveProcedure = async (ref: SharedProcedureRef) => {
    try {
      const options = await fetchProcedureOptions(ref.airportIcao, ref.kind);
      const procedure = options.find((p) => p.ident === ref.procedureIdent);
      if (!procedure) return null;
      const detail = await fetchProcedureDetail(procedure.id);
      const transition = detail.transitions.find(
        (t) => t.kind === "ENROUTE" && t.ident === ref.transitionIdent,
      );
      if (!transition) return null;
      return buildResolvedProcedure(ref.airportIcao, ref.kind, detail, transition.id);
    } catch {
      return null;
    }
  };
  const resolveToken = async (token: SharedRouteToken): Promise<RouteToken | null> => {
    if (token.kind === "point") return { kind: "point", point: token.point };
    try {
      const detail = await fetchAirwayDetail(token.ident);
      return { kind: "airway", ident: token.ident, detail };
    } catch {
      return null;
    }
  };

  const [sid, star, middleTokens] = await Promise.all([
    shared.sid ? resolveProcedure(shared.sid) : Promise.resolve(null),
    shared.star ? resolveProcedure(shared.star) : Promise.resolve(null),
    Promise.all(shared.middleTokens.map(resolveToken)),
  ]);

  return {
    departure: shared.departure,
    arrival: shared.arrival,
    sid,
    star,
    middleTokens: middleTokens.filter((t): t is RouteToken => t !== null),
  };
}
