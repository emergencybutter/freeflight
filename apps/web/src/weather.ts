import type { GAirmet, Metar, Sigmet, Taf, WindsAloftBulletin } from "./types";

// ff-api isn't started by `npm run dev` — it's a separate Rust service
// (see apps/web/README.md). Default to the standard local dev port so
// things work out of the box when both are running; override with
// VITE_FF_API_BASE_URL if ff-api is elsewhere.
export const API_BASE_URL = import.meta.env.VITE_FF_API_BASE_URL ?? "http://localhost:8080";

async function fetchJson<T>(path: string): Promise<T> {
  const res = await fetch(`${API_BASE_URL}${path}`);
  if (!res.ok) {
    throw new Error(`${path} failed: ${res.status} ${await res.text()}`);
  }
  return res.json() as Promise<T>;
}

export async function fetchMetar(icao: string): Promise<Metar | null> {
  const metars = await fetchJson<Metar[]>(`/weather/metar?ids=${encodeURIComponent(icao)}`);
  return metars[0] ?? null;
}

export async function fetchTaf(icao: string): Promise<Taf | null> {
  const tafs = await fetchJson<Taf[]>(`/weather/taf?ids=${encodeURIComponent(icao)}`);
  return tafs[0] ?? null;
}

/** METAR for multiple stations in one request — used to color airport
 * markers by flight category, as opposed to fetchMetar's one-station
 * detail-panel use. */
export async function fetchMetars(icaos: string[]): Promise<Metar[]> {
  if (icaos.length === 0) return [];
  return fetchJson<Metar[]>(`/weather/metar?ids=${encodeURIComponent(icaos.join(","))}`);
}

export async function fetchGairmets(): Promise<GAirmet[]> {
  return fetchJson<GAirmet[]>("/weather/gairmet");
}

export async function fetchSigmets(): Promise<Sigmet[]> {
  return fetchJson<Sigmet[]>("/weather/sigmet");
}

export async function fetchWindsAloft(
  level: "low" | "high" = "low",
  fcstHour = "06",
  region = "all",
): Promise<WindsAloftBulletin> {
  return fetchJson<WindsAloftBulletin>(`/weather/windtemp?level=${level}&fcst=${fcstHour}&region=${region}`);
}
