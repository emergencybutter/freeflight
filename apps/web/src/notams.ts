import { fetchJson } from "./api";
import type { Notam, NotamResponse } from "./types";

/** NOTAMs for one location, unwrapped from ff-api's `/notams` GeoJSON
 * envelope into a flat list. Returns `[]` when the location has none;
 * throws (like other fetches) if the proxy isn't configured (501) or the
 * upstream fails, which the caller surfaces as a NOTAM-section error. */
export async function fetchNotams(icao: string): Promise<Notam[]> {
  const resp = await fetchJson<NotamResponse>(`/notams?location=${encodeURIComponent(icao)}`);
  const features = resp.data?.geojson ?? [];
  return features
    .map((f) => f.properties?.coreNOTAMData?.notam)
    .filter((n): n is Notam => n != null);
}
