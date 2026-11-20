// ff-api isn't started by `npm run dev` — it's a separate Rust service
// (see apps/web/README.md), and under DESIGN.md §8 this client assumes
// it's reachable: there is no offline fallback on web by design.
// Default to ff-api's standard port on whatever host the page itself
// was loaded from — not a hardcoded localhost, so opening the dev
// server from another machine (LAN/Tailscale) reaches the ff-api next
// to it instead of the browser's own machine. Override with
// VITE_FF_API_BASE_URL when ff-api lives somewhere else entirely.
export const API_BASE_URL =
  import.meta.env.VITE_FF_API_BASE_URL ?? `${window.location.protocol}//${window.location.hostname}:8080`;

// Where cycle data (`/data/*`, `/cycles/latest`, chart tiles) comes from.
// The same server as everything else on the web; the Android app sets it to
// its own origin, where those requests are answered on-device from the
// downloaded cycle so they work offline (see ff-uniffi's `web` module).
// Weather, NOTAMs, sign-in and aircraft are live and always use API_BASE_URL.
export const DATA_BASE_URL = import.meta.env.VITE_FF_DATA_BASE_URL ?? API_BASE_URL;

/** True inside the Android app, where cycle data is answered on-device. */
export const DATA_ON_DEVICE = DATA_BASE_URL !== API_BASE_URL;

/** Absolute URL for a chart's `tile_url`: on-device data may hand back
 * absolute server URLs for charts that aren't downloaded yet. */
export function chartTileUrl(tileUrl: string): string {
  return /^https?:\/\//.test(tileUrl) ? tileUrl : `${DATA_BASE_URL}${tileUrl}`;
}

export async function fetchJson<T>(path: string): Promise<T> {
  return fetchJsonFrom<T>(API_BASE_URL, path);
}

/** Like `fetchJson`, for cycle data (see DATA_BASE_URL). */
export async function fetchDataJson<T>(path: string): Promise<T> {
  return fetchJsonFrom<T>(DATA_BASE_URL, path);
}

async function fetchJsonFrom<T>(base: string, path: string): Promise<T> {
  const res = await fetch(`${base}${path}`);
  if (!res.ok) {
    // An HTML body is a proxy's error page (e.g. Cloudflare's), not a
    // message from ff-api; don't dump the markup into the UI.
    const body = await res.text();
    const detail = body.trimStart().startsWith("<") ? res.statusText : body;
    throw new Error(`${path} failed: ${res.status} ${detail}`);
  }
  return res.json() as Promise<T>;
}
