import type { Metar, Taf } from "./types";

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
