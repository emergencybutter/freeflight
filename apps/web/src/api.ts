// ff-api isn't started by `npm run dev` — it's a separate Rust service
// (see apps/web/README.md), and under DESIGN.md §8 this client assumes
// it's reachable: there is no offline fallback on web by design.
// Default to the standard local dev port so things work out of the box
// when both are running; override with VITE_FF_API_BASE_URL.
export const API_BASE_URL = import.meta.env.VITE_FF_API_BASE_URL ?? "http://localhost:8080";

export async function fetchJson<T>(path: string): Promise<T> {
  const res = await fetch(`${API_BASE_URL}${path}`);
  if (!res.ok) {
    throw new Error(`${path} failed: ${res.status} ${await res.text()}`);
  }
  return res.json() as Promise<T>;
}
