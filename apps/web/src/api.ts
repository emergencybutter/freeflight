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

export async function fetchJson<T>(path: string): Promise<T> {
  const res = await fetch(`${API_BASE_URL}${path}`);
  if (!res.ok) {
    throw new Error(`${path} failed: ${res.status} ${await res.text()}`);
  }
  return res.json() as Promise<T>;
}
