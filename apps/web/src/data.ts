// Typed wrappers over ff-api's /data/* and /cycles/latest endpoints
// (DESIGN.md §4.1) — the web client's only source of cycle data under
// the thin-client design (§8): no local SQLite, no offline cache.
import { fetchJson } from "./api";
import type {
  Airport,
  AirportDetail,
  AirspaceVolume,
  ChartCatalogEntry,
  CycleManifest,
  Procedure,
  ProcedureDetail,
} from "./types";

export async function fetchCycleManifest(): Promise<CycleManifest> {
  return fetchJson<CycleManifest>("/cycles/latest");
}

/** Airports within `minLon,minLat,maxLon,maxLat` — bundles are
 * nationwide (~13k airports), so the map always asks for a bounding box
 * rather than everything. */
export async function fetchAirportsInBbox(bbox: string): Promise<Airport[]> {
  return fetchJson<Airport[]>(`/data/airports?bbox=${encodeURIComponent(bbox)}`);
}

export async function searchAirports(q: string): Promise<Airport[]> {
  return fetchJson<Airport[]>(`/data/search?q=${encodeURIComponent(q)}`);
}

export async function fetchAirportDetail(icao: string): Promise<AirportDetail> {
  return fetchJson<AirportDetail>(`/data/airports/${encodeURIComponent(icao)}`);
}

export async function fetchAirportProcedures(icao: string): Promise<Procedure[]> {
  return fetchJson<Procedure[]>(`/data/airports/${encodeURIComponent(icao)}/procedures`);
}

export async function fetchProcedureDetail(id: string): Promise<ProcedureDetail> {
  return fetchJson<ProcedureDetail>(`/data/procedures/${encodeURIComponent(id)}`);
}

export async function fetchCharts(): Promise<ChartCatalogEntry[]> {
  return fetchJson<ChartCatalogEntry[]>("/data/charts");
}

/** Airspace boundaries within `minLon,minLat,maxLon,maxLat` — a
 * nationwide cycle has ~2800 rows (one per shelf/sector, not one per
 * named airspace) across Class B/C/D + Special Use Airspace, so the map
 * asks for a bounding box rather than everything, same as airports. */
export async function fetchAirspaceInBbox(bbox: string): Promise<AirspaceVolume[]> {
  return fetchJson<AirspaceVolume[]>(`/data/airspace?bbox=${encodeURIComponent(bbox)}`);
}
