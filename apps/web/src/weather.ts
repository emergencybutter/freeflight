import { fetchJson } from "./api";
import type { Cwa, Datis, GAirmet, Metar, Pirep, Sigmet, Taf, WindsAloftBulletin } from "./types";

export { API_BASE_URL } from "./api";

export async function fetchMetar(icao: string): Promise<Metar | null> {
  const metars = await fetchJson<Metar[]>(`/weather/metar?ids=${encodeURIComponent(icao)}`);
  return metars[0] ?? null;
}

export async function fetchTaf(icao: string): Promise<Taf | null> {
  const tafs = await fetchJson<Taf[]>(`/weather/taf?ids=${encodeURIComponent(icao)}`);
  return tafs[0] ?? null;
}

/** D-ATIS for one airport — usually one "combined" entry, or a "dep"/
 * "arr" pair, or empty when the airport has no Digital ATIS. */
export async function fetchDatis(icao: string): Promise<Datis[]> {
  return fetchJson<Datis[]>(`/weather/atis?ids=${encodeURIComponent(icao)}`);
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

export async function fetchCwas(): Promise<Cwa[]> {
  return fetchJson<Cwa[]>("/weather/cwa");
}

/** `bbox` is `minLon,minLat,maxLon,maxLat` (same convention as
 * fetchAirportsInBbox/fetchAirspaceInBbox) — ff-api reorders it to
 * aviationweather.gov's own bbox convention before proxying. */
export async function fetchPireps(bbox: string): Promise<Pirep[]> {
  return fetchJson<Pirep[]>(`/weather/pirep?bbox=${encodeURIComponent(bbox)}`);
}

export async function fetchWindsAloft(
  level: "low" | "high" = "low",
  fcstHour = "06",
  region = "all",
): Promise<WindsAloftBulletin> {
  return fetchJson<WindsAloftBulletin>(`/weather/windtemp?level=${level}&fcst=${fcstHour}&region=${region}`);
}
