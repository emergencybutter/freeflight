// Typed wrappers over ff-api's /data/* and /cycles/latest endpoints
// (DESIGN.md §4.1) — the web client's only source of cycle data under
// the thin-client design (§8): no local SQLite, no offline cache.
import { fetchJson } from "./api";
import type {
  Airport,
  AirportDetail,
  ChartCatalogEntry,
  CycleManifest,
  Procedure,
  ProcedureDetail,
} from "./types";

export async function fetchCycleManifest(): Promise<CycleManifest> {
  return fetchJson<CycleManifest>("/cycles/latest");
}

export async function fetchAirports(): Promise<Airport[]> {
  return fetchJson<Airport[]>("/data/airports");
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
