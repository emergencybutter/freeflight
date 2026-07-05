// Flags airspace the planned route's legs actually pass through — plain
// 2D geometry over data the map already fetches (see
// data.ts::fetchAirspaceInBbox), lon/lat treated as flat x/y. Same
// approximation this app already makes elsewhere for legs at this
// scale (e.g. MapView's turn-fillet rendering is a display
// simplification too, not a flight-performance model) — not a
// certified airspace tool, just a lateral heads-up (see
// FlightPlanning.tsx for why altitude isn't factored in).
import type { AirspaceVolume, RouteWaypoint } from "../types";

type Point = [number, number]; // [lon, lat]

function pointInRing(point: Point, ring: Point[]): boolean {
  // Standard ray-casting: count crossings of a horizontal ray from the
  // point to +infinity against each ring edge.
  let inside = false;
  const [x, y] = point;
  for (let i = 0, j = ring.length - 1; i < ring.length; j = i++) {
    const [xi, yi] = ring[i];
    const [xj, yj] = ring[j];
    const crosses = yi > y !== yj > y && x < ((xj - xi) * (y - yi)) / (yj - yi) + xi;
    if (crosses) inside = !inside;
  }
  return inside;
}

function pointInPolygon(point: Point, rings: Point[][]): boolean {
  const [exterior, ...holes] = rings;
  if (!exterior || !pointInRing(point, exterior)) return false;
  return !holes.some((hole) => pointInRing(point, hole));
}

function orientation(a: Point, b: Point, c: Point): number {
  return (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
}

function onSegment(a: Point, b: Point, p: Point): boolean {
  return (
    Math.min(a[0], b[0]) <= p[0] &&
    p[0] <= Math.max(a[0], b[0]) &&
    Math.min(a[1], b[1]) <= p[1] &&
    p[1] <= Math.max(a[1], b[1])
  );
}

function segmentsIntersect(a1: Point, a2: Point, b1: Point, b2: Point): boolean {
  const d1 = orientation(b1, b2, a1);
  const d2 = orientation(b1, b2, a2);
  const d3 = orientation(a1, a2, b1);
  const d4 = orientation(a1, a2, b2);
  if (((d1 > 0 && d2 < 0) || (d1 < 0 && d2 > 0)) && ((d3 > 0 && d4 < 0) || (d3 < 0 && d4 > 0))) {
    return true;
  }
  // Collinear-touching cases.
  if (d1 === 0 && onSegment(b1, b2, a1)) return true;
  if (d2 === 0 && onSegment(b1, b2, a2)) return true;
  if (d3 === 0 && onSegment(a1, a2, b1)) return true;
  if (d4 === 0 && onSegment(a1, a2, b2)) return true;
  return false;
}

function legCrossesVolume(from: Point, to: Point, volume: AirspaceVolume): boolean {
  const geometry = JSON.parse(volume.boundary_geojson) as { coordinates: Point[][] };
  const rings = geometry.coordinates;
  const [exterior] = rings;
  if (!exterior) return false;

  if (pointInPolygon(from, rings) || pointInPolygon(to, rings)) return true;

  // Crossing without either endpoint inside (a leg that clips through a
  // corner) — checked against the exterior ring only: entering via a
  // hole's boundary isn't a real crossing of the volume itself.
  for (let i = 0; i < exterior.length - 1; i++) {
    if (segmentsIntersect(from, to, exterior[i], exterior[i + 1])) return true;
  }
  return false;
}

/** Every distinct airspace volume any leg of `points` passes through,
 * in route order (first-crossed first). `volumes` is expected to
 * already be bbox-filtered to the route's extent (see
 * data.ts::fetchAirspaceInBbox) — this doesn't re-filter by bounding
 * box itself. */
export function findCrossedAirspace(points: RouteWaypoint[], volumes: AirspaceVolume[]): AirspaceVolume[] {
  const crossed: AirspaceVolume[] = [];
  const seenIds = new Set<string>();

  for (let i = 0; i < points.length - 1; i++) {
    const from: Point = [points[i].lon, points[i].lat];
    const to: Point = [points[i + 1].lon, points[i + 1].lat];
    for (const volume of volumes) {
      if (seenIds.has(volume.id)) continue;
      if (legCrossesVolume(from, to, volume)) {
        seenIds.add(volume.id);
        crossed.push(volume);
      }
    }
  }
  return crossed;
}
