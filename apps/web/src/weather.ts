import { fetchJson } from "./api";
import type { GAirmet, Metar, Sigmet, Taf, WindsAloftBulletin } from "./types";

export { API_BASE_URL } from "./api";

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
