// Typed wrappers over ff-api's /data/* and /cycles/latest endpoints
// (DESIGN.md §4.1) — the web client's only source of cycle data under
// the thin-client design (§8): no local SQLite, no offline cache.
import { fetchDataJson } from "./api";
import type {
  Airport,
  AirportDetail,
  AirspaceVolume,
  AirwayDetail,
  ChartCatalogEntry,
  CycleManifest,
  IdentSearchRow,
  NearestFix,
  PreferredRouteRow,
  Procedure,
  ProcedureDetail,
} from "./types";

export async function fetchCycleManifest(): Promise<CycleManifest> {
  return fetchDataJson<CycleManifest>("/cycles/latest");
}

/** Airports within `minLon,minLat,maxLon,maxLat` — bundles are
 * nationwide (~13k airports), so the map always asks for a bounding box
 * rather than everything. */
export async function fetchAirportsInBbox(bbox: string): Promise<Airport[]> {
  return fetchDataJson<Airport[]>(`/data/airports?bbox=${encodeURIComponent(bbox)}`);
}

export async function searchAirports(q: string): Promise<Airport[]> {
  return fetchDataJson<Airport[]>(`/data/search?q=${encodeURIComponent(q)}`);
}

export async function fetchAirportDetail(icao: string): Promise<AirportDetail> {
  return fetchDataJson<AirportDetail>(`/data/airports/${encodeURIComponent(icao)}`);
}

export async function fetchAirportProcedures(icao: string): Promise<Procedure[]> {
  return fetchDataJson<Procedure[]>(`/data/airports/${encodeURIComponent(icao)}/procedures`);
}

export async function fetchProcedureDetail(id: string): Promise<ProcedureDetail> {
  return fetchDataJson<ProcedureDetail>(`/data/procedures/${encodeURIComponent(id)}`);
}

export async function fetchAirwayDetail(ident: string): Promise<AirwayDetail> {
  return fetchDataJson<AirwayDetail>(`/data/airways/${encodeURIComponent(ident)}`);
}

/** Unified ident search (airports, waypoints, navaids, airways) for the
 * route builder's single search box. */
export async function searchIdents(q: string): Promise<IdentSearchRow[]> {
  return fetchDataJson<IdentSearchRow[]>(`/data/search_idents?q=${encodeURIComponent(q)}`);
}

/** Suggested routings between two airports, once both are set on the
 * flight plan — see PreferredRouteRow. */
export async function fetchPreferredRoutes(from: string, to: string): Promise<PreferredRouteRow[]> {
  return fetchDataJson<PreferredRouteRow[]>(
    `/data/preferred_routes?from=${encodeURIComponent(from)}&to=${encodeURIComponent(to)}`,
  );
}

export async function fetchCharts(): Promise<ChartCatalogEntry[]> {
  return fetchDataJson<ChartCatalogEntry[]>("/data/charts");
}

/** Airspace boundaries within `minLon,minLat,maxLon,maxLat` — a
 * nationwide cycle has ~2800 rows (one per shelf/sector, not one per
 * named airspace) across Class B/C/D + Special Use Airspace, so the map
 * asks for a bounding box rather than everything, same as airports. */
export async function fetchAirspaceInBbox(bbox: string): Promise<AirspaceVolume[]> {
  return fetchDataJson<AirspaceVolume[]>(`/data/airspace?bbox=${encodeURIComponent(bbox)}`);
}

/** Closest waypoint/navaid to a tapped map point — backs the map's
 * "Waypoint" tap tab. */
export async function fetchNearestFix(lat: number, lon: number): Promise<NearestFix> {
  return fetchDataJson<NearestFix>(`/data/nearest_fix?lat=${lat}&lon=${lon}`);
}
