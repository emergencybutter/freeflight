// Typed wrappers over ff-api's /aircraft endpoints (DESIGN.md §9.5.4).
//
// Unlike ./data.ts, every call here except the type catalog is
// authenticated and user-scoped, so these go out with the bearer token
// from ./auth.ts rather than through api.ts's plain `fetchJson`.
//
// Errors carry the server's message: the API answers a bad field with a
// readable sentence ("cruise_tas_kt must be greater than zero") and a
// duplicate registration with a 409, both of which belong in front of
// the user rather than swallowed into a generic failure.
import { API_BASE_URL } from "./api";
import { getToken } from "./auth";

/** One row of a performance table. `power_setting` is empty for
 * climb/descent, which have no power dimension. */
export interface PerformanceRow {
  pressure_altitude_ft: number;
  power_setting: string;
  /** Positive magnitude; the phase supplies the sign. Null for cruise. */
  vertical_speed_fpm: number | null;
  tas_kt: number;
  fuel_gph: number;
}

/** An aircraft's identity and scalar performance, as `GET /aircraft`
 * returns it. Mirrors ff-accounts' `Aircraft` one-to-one. */
export interface Aircraft {
  id: number;
  registration: string;
  serial_number: string | null;
  icao_type: string | null;
  name: string | null;
  cruise_tas_kt: number | null;
  cruise_fuel_gph: number | null;
  climb_rate_fpm: number | null;
  climb_tas_kt: number | null;
  climb_fuel_gph: number | null;
  descent_rate_fpm: number | null;
  descent_tas_kt: number | null;
  descent_fuel_gph: number | null;
  taxi_fuel_gal: number | null;
  fuel_capacity_gal: number | null;
  reserve_minutes: number | null;
  max_gross_weight_lb: number | null;
  forward_cg_limit_in: number | null;
  aft_cg_limit_in: number | null;
  template_icao: string | null;
  /** Null until the pilot confirms the numbers against their own POH —
   * surfaced wherever this feeds a plan (§9.5.3). */
  verified_at: string | null;
  created_at: string;
  updated_at: string;
}

/** `GET /aircraft/:id` — the aircraft plus all three tables. */
export interface AircraftDetail extends Aircraft {
  climb: PerformanceRow[];
  cruise: PerformanceRow[];
  descent: PerformanceRow[];
}

/** The writable half, sent by both create and replace. */
export interface AircraftInput {
  registration: string;
  serial_number?: string | null;
  icao_type?: string | null;
  name?: string | null;
  cruise_tas_kt?: number | null;
  cruise_fuel_gph?: number | null;
  climb_rate_fpm?: number | null;
  climb_tas_kt?: number | null;
  climb_fuel_gph?: number | null;
  descent_rate_fpm?: number | null;
  descent_tas_kt?: number | null;
  descent_fuel_gph?: number | null;
  taxi_fuel_gal?: number | null;
  fuel_capacity_gal?: number | null;
  reserve_minutes?: number | null;
  max_gross_weight_lb?: number | null;
  forward_cg_limit_in?: number | null;
  aft_cg_limit_in?: number | null;
  template_icao?: string | null;
  /** True once the pilot has confirmed these against their POH. */
  verified?: boolean;
  /** Create only: seed unset fields from this ICAO type's template. */
  from_type?: string | null;
}

/** A shipped type template (`GET /aircraft/types`). Book figures for a
 * new airframe on a standard day — a starting point, never authoritative
 * (§9.5.3), which is why nothing created from one is verified. */
export interface AircraftTemplate {
  icao_type: string;
  name: string;
  cruise_tas_kt: number | null;
  cruise_fuel_gph: number | null;
  climb_rate_fpm: number | null;
  climb_tas_kt: number | null;
  climb_fuel_gph: number | null;
  descent_rate_fpm: number | null;
  descent_tas_kt: number | null;
  descent_fuel_gph: number | null;
  taxi_fuel_gal: number | null;
  fuel_capacity_gal: number | null;
  reserve_minutes: number | null;
  max_gross_weight_lb: number | null;
  climb: PerformanceRow[];
  cruise: PerformanceRow[];
  descent: PerformanceRow[];
}

export type Phase = "climb" | "cruise" | "descent";

/** Thrown with the server's own message, so the UI can show why a save
 * was refused instead of "something went wrong". */
export class AircraftApiError extends Error {
  constructor(
    message: string,
    readonly status: number,
  ) {
    super(message);
    this.name = "AircraftApiError";
  }
}

async function request<T>(path: string, init: RequestInit = {}): Promise<T> {
  const token = getToken();
  const headers: Record<string, string> = { ...(init.headers as Record<string, string>) };
  if (token) headers.Authorization = `Bearer ${token}`;
  if (init.body) headers["Content-Type"] = "application/json";

  const res = await fetch(`${API_BASE_URL}${path}`, { ...init, headers });
  if (!res.ok) {
    // The API sends a plain-text reason for 400/403/409; fall back to the
    // status line when it doesn't (502 from a proxy, say).
    const detail = (await res.text().catch(() => "")).trim();
    throw new AircraftApiError(detail || `${res.status} ${res.statusText}`, res.status);
  }
  // 204 No Content on delete.
  if (res.status === 204) return undefined as T;
  return (await res.json()) as T;
}

/** Public — no sign-in needed, so the "add aircraft" form can offer types
 * before the user has an account. */
export async function fetchTemplates(): Promise<AircraftTemplate[]> {
  const res = await fetch(`${API_BASE_URL}/aircraft/types`);
  if (!res.ok) throw new AircraftApiError(`${res.status} ${res.statusText}`, res.status);
  return (await res.json()) as AircraftTemplate[];
}

export async function fetchFleet(): Promise<Aircraft[]> {
  return request<Aircraft[]>("/aircraft");
}

export async function fetchAircraft(id: number): Promise<AircraftDetail> {
  return request<AircraftDetail>(`/aircraft/${id}`);
}

export async function createAircraft(input: AircraftInput): Promise<AircraftDetail> {
  return request<AircraftDetail>("/aircraft", {
    method: "POST",
    body: JSON.stringify(input),
  });
}

export async function updateAircraft(id: number, input: AircraftInput): Promise<Aircraft> {
  return request<Aircraft>(`/aircraft/${id}`, {
    method: "PUT",
    body: JSON.stringify(input),
  });
}

export async function deleteAircraft(id: number): Promise<void> {
  return request<void>(`/aircraft/${id}`, { method: "DELETE" });
}

/** Replaces that phase's whole table — the grid is edited as one
 * document, so there are no per-row ids to keep track of. */
export async function replacePerformance(
  id: number,
  phase: Phase,
  rows: PerformanceRow[],
): Promise<AircraftDetail> {
  return request<AircraftDetail>(`/aircraft/${id}/performance/${phase}`, {
    method: "PUT",
    body: JSON.stringify(rows),
  });
}
