import { useEffect, useRef, useState } from "react";
import maplibregl, { type Map as MlMap } from "maplibre-gl";
import "maplibre-gl/dist/maplibre-gl.css";
import { Protocol as PmtilesProtocol } from "pmtiles";
import { API_BASE_URL } from "./api";
import { fetchAirportDetail, fetchAirportsInBbox, fetchAirspaceInBbox, fetchCharts, fetchProcedureDetail } from "./data";
import type {
  Airport,
  AirspaceVolume,
  GAirmet,
  ProcedureDetail,
  RouteWaypoint,
  Runway,
  Sigmet,
  WindsAloftBulletin,
} from "./types";
import { fetchGairmets, fetchMetars, fetchSigmets, fetchWindsAloft } from "./weather";

const AIRPORTS_SOURCE = "airports";
const RUNWAYS_SOURCE = "runways";
const PROCEDURE_SOURCE = "procedure-path";
const PROCEDURE_FIXES_SOURCE = "procedure-fixes";
const GAIRMET_SOURCE = "gairmets";
const SIGMET_SOURCE = "sigmets";
const WINDS_ALOFT_SOURCE = "winds-aloft";
const AIRSPACE_SOURCE = "airspace";
const PLANNED_ROUTE_SOURCE = "planned-route";
const PLANNED_ROUTE_FIXES_SOURCE = "planned-route-fixes";

// The standard levels NOAA's "low" FD product reports per station (see
// a real response: every CONUS station carries all nine) — used to
// populate the altitude selector and to snap a preferred altitude (the
// flight plan's cruise altitude, if set) to the nearest one that
// actually has data.
const WINDS_ALOFT_LEVELS_FT = [3000, 6000, 9000, 12000, 18000, 24000, 30000, 34000, 39000];
const DEFAULT_WINDS_ALOFT_ALTITUDE_FT = 9000;

function nearestWindsAloftLevel(altitudeFt: number): number {
  return WINDS_ALOFT_LEVELS_FT.reduce((closest, level) =>
    Math.abs(level - altitudeFt) < Math.abs(closest - altitudeFt) ? level : closest,
  );
}

/** Friendly labels for `chart_catalog.kind` values (see `chart_kind_str`
 * in services/ff-etl/src/bundle.rs) — falls back to the raw kind string
 * for anything not listed here. */
const CHART_KIND_LABELS: Record<string, string> = {
  Sectional: "Sectional",
  IfrEnrouteLow: "IFR Low",
  IfrEnrouteHigh: "IFR High",
};

const FLIGHT_CATEGORY_COLORS: Record<string, string> = {
  VFR: "#3fa64a",
  MVFR: "#1f6fd1",
  IFR: "#d13f3f",
  LIFR: "#c23fd1",
};
const DEFAULT_AIRPORT_COLOR = "#3d7fc4";

const GAIRMET_HAZARD_COLORS: Record<string, string> = {
  TURB: "#ffb020",
  ICE: "#4fc3f7",
  MT_OBSC: "#8a8a8a",
  IFR: "#9b6bd6",
  FZLVL: "#7fd0ff",
  SFC_WND: "#e0c341",
};
const DEFAULT_GAIRMET_COLOR = "#ffb020";

// Roughly follows real sectional-chart convention (blue = Class B/D,
// magenta = Class C, red/orange = the military-flavored special-use
// kinds) rather than inventing an arbitrary categorical palette — a
// pilot reading this map already has that color association.
const AIRSPACE_CLASS_COLORS: Record<string, string> = {
  B: "#3d7fc4",
  C: "#c23fd1",
  D: "#3d7fc4",
  MOA: "#d1663f",
  RESTRICTED: "#d13f3f",
  PROHIBITED: "#d13f3f",
  WARNING: "#e0973f",
  ALERT: "#e0c341",
};
const DEFAULT_AIRSPACE_COLOR = "#8a8a8a";

// Registered once per page load (module scope, not per-component-mount):
// MapLibre's addProtocol is global, and re-registering on every mount
// (e.g. React StrictMode's double-invoke) is harmless but pointless.
maplibregl.addProtocol("pmtiles", new PmtilesProtocol().tile);

// Real chart imagery (raster sectionals/IFR enroute) is still the
// primary base layer — see the chart_catalog handling below — but it's
// opt-in per DESIGN.md §9.1 and only exists once a cycle bundle has
// published one, so relying on it alone left the map solid black
// whenever charts were toggled off or hadn't loaded yet. OpenFreeMap
// (openfreemap.org) fills that gap: free vector tiles, no API key, no
// rate limits (sponsor-funded, actually built for this kind of use
// rather than a free tier of a paid product). Our own layers get added
// on top of this style's in the "load" handler below — chart raster
// tiles are opaque, so they cover it naturally without any extra
// show/hide logic once loaded and visible.
const BASEMAP_STYLE_URL = "https://tiles.openfreemap.org/styles/liberty";

const EMPTY_COLLECTION: GeoJSON.FeatureCollection = { type: "FeatureCollection", features: [] };

function airportsGeoJson(airports: Airport[], flightCategories: Map<string, string>): GeoJSON.FeatureCollection {
  return {
    type: "FeatureCollection",
    features: airports.map((a) => ({
      type: "Feature",
      geometry: { type: "Point", coordinates: [a.lon, a.lat] },
      properties: { icao: a.icao, fltCat: flightCategories.get(a.icao) ?? null },
    })),
  };
}

/** G-AIRMET records as map features: `"AREA"` -> a closed-ring Polygon
 * (the API's coords already close the ring), `"LINE"` -> a LineString
 * (e.g. freezing-level lines) — both confirmed on live data, see
 * ff-weather's hazards.rs docs. Coordinates come across as strings. */
function gairmetGeoJson(records: GAirmet[]): GeoJSON.FeatureCollection {
  const features: GeoJSON.Feature[] = records.map((r) => {
    const coords = r.coords.map((c) => [parseFloat(c.lon), parseFloat(c.lat)]);
    const geometry: GeoJSON.Geometry =
      r.geometryType === "LINE" ? { type: "LineString", coordinates: coords } : { type: "Polygon", coordinates: [coords] };
    return {
      type: "Feature",
      geometry,
      properties: { hazard: r.hazard, tag: r.tag },
    };
  });
  return { type: "FeatureCollection", features };
}

/** US domestic/convective SIGMETs — always a closed-ring Polygon on live
 * data (confirmed: no `geom`/multi-polygon case like `IntlSigmet` has). */
function sigmetGeoJson(records: Sigmet[]): GeoJSON.FeatureCollection {
  return {
    type: "FeatureCollection",
    features: records.map((r) => ({
      type: "Feature",
      geometry: {
        type: "Polygon",
        coordinates: [r.coords.map((c) => [c.lon, c.lat])],
      },
      properties: { hazard: r.hazard, seriesId: r.seriesId },
    })),
  };
}

/** `boundary_geojson` is a GeoJSON `Polygon` geometry object (not a
 * whole `Feature`), stored/served as an unparsed string — see
 * `AirspaceVolume`'s doc comment in types.ts. */
function airspaceGeoJson(volumes: AirspaceVolume[]): GeoJSON.FeatureCollection {
  return {
    type: "FeatureCollection",
    features: volumes.map((v) => ({
      type: "Feature",
      geometry: JSON.parse(v.boundary_geojson) as GeoJSON.Geometry,
      properties: { id: v.id, name: v.name, class: v.class, floor: v.floor, ceiling: v.ceiling },
    })),
  };
}

/** The Flight Plan view's route (App.tsx lifts it so both that view and
 * this one can see it) as a single straight-leg line — a planned route
 * is the great-circle legs `ff-planning` computes the nav log from, not
 * a flown path, so unlike the procedure line this isn't curved. */
function plannedRouteGeoJson(route: RouteWaypoint[]): GeoJSON.FeatureCollection {
  if (route.length < 2) return EMPTY_COLLECTION;
  return {
    type: "FeatureCollection",
    features: [
      {
        type: "Feature",
        geometry: { type: "LineString", coordinates: route.map((a) => [a.lon, a.lat]) },
        properties: {},
      },
    ],
  };
}

/** One marker + ident label per planned-route point, so an inserted
 * airway's constituent fixes are visible as more than just line bends. */
function plannedRouteFixesGeoJson(route: RouteWaypoint[]): GeoJSON.FeatureCollection {
  return {
    type: "FeatureCollection",
    features: route.map((p) => ({
      type: "Feature",
      geometry: { type: "Point", coordinates: [p.lon, p.lat] },
      properties: { label: p.ident },
    })),
  };
}

/** Winds-aloft station idents are 3-letter FAA identifiers (e.g. "SFO"),
 * not ICAO codes — strip the CONUS "K" prefix to match. Only stations
 * the bulletin actually covers get a feature (confirmed live: most
 * towered/major airports report, small GA fields generally don't, and
 * that's correct behavior to show, not a bug to work around). */
function windsAloftGeoJson(bulletin: WindsAloftBulletin, airports: Airport[], altitudeFt: number): GeoJSON.FeatureCollection {
  const features: GeoJSON.Feature[] = [];
  for (const airport of airports) {
    const stationId = airport.icao.length === 4 && airport.icao.startsWith("K") ? airport.icao.slice(1) : airport.icao;
    const station = bulletin.stations.find((s) => s.station_id === stationId);
    const level = station?.levels.find((l) => l.altitude_ft === altitudeFt);
    if (!level) continue;
    const wind = level.wind;
    const lightAndVariable = wind === "LightAndVariable";
    const directionDeg = lightAndVariable ? null : wind.Directional.direction_deg;
    const speedKt = lightAndVariable ? 0 : wind.Directional.speed_kt;
    features.push({
      type: "Feature",
      geometry: { type: "Point", coordinates: [airport.lon, airport.lat] },
      properties: {
        icao: airport.icao,
        // Arrow points in the direction the wind is blowing TOWARD (the
        // API reports the direction it's blowing FROM, aviation
        // convention) — rotate by +180 so the glyph reads as "this way".
        arrowRotation: directionDeg === null ? 0 : (directionDeg + 180) % 360,
        lightAndVariable,
        label: lightAndVariable
          ? `LGT VRB${level.temp_c !== null ? ` ${level.temp_c}°C` : ""}`
          : `${speedKt}kt${level.temp_c !== null ? ` ${level.temp_c}°C` : ""}`,
      },
    });
  }
  return { type: "FeatureCollection", features };
}

function runwaysGeoJson(runways: Runway[]): GeoJSON.FeatureCollection {
  return {
    type: "FeatureCollection",
    features: runways.map((r) => ({
      type: "Feature",
      geometry: {
        type: "LineString",
        coordinates: [
          [r.le_lon, r.le_lat],
          [r.he_lon, r.he_lat],
        ],
      },
      properties: { ident: r.ident },
    })),
  };
}

/** One line per transition — legs from different transitions (enroute vs.
 * approach vs. missed) aren't a continuous path, so they're never joined.
 * Fix coordinates come pre-resolved in the /data/procedures/:id response. */
// Rounds the sharp vertex at each procedure-path turn into a circular
// arc, at a fixed turn radius of 1 NM. This is a display simplification
// (a real turn radius depends on groundspeed/bank angle), not a flight-
// performance model — it just replaces angular vertices with something
// that reads as a flown path rather than a surveyor's traverse.
const EARTH_RADIUS_M = 6371000;
const NM_TO_M = 1852;
const TURN_RADIUS_M = 1 * NM_TO_M;
// Turns tighter than this are left as sharp corners — not worth curving
// a heading change you'd barely see on a chart.
const MIN_FILLET_TURN_RAD = (1 * Math.PI) / 180;
// Caps how much of either adjacent leg a fillet can consume, so two
// consecutive tight turns on a short leg can't eat past each other.
const FILLET_TANGENT_SAFETY_FRACTION = 0.45;

type LocalPoint = [number, number];

/** Local flat-earth (east, north) meters around `refLon,refLat` — only
 * ever used within a couple of turn radii of the reference point, where
 * the flat approximation's error is negligible for chart display. */
function toLocalMeters(lon: number, lat: number, refLon: number, refLat: number): LocalPoint {
  const refLatRad = (refLat * Math.PI) / 180;
  const x = (((lon - refLon) * Math.PI) / 180) * Math.cos(refLatRad) * EARTH_RADIUS_M;
  const y = (((lat - refLat) * Math.PI) / 180) * EARTH_RADIUS_M;
  return [x, y];
}

function fromLocalMeters(x: number, y: number, refLon: number, refLat: number): [number, number] {
  const refLatRad = (refLat * Math.PI) / 180;
  const lon = refLon + (x / (EARTH_RADIUS_M * Math.cos(refLatRad))) * (180 / Math.PI);
  const lat = refLat + (y / EARTH_RADIUS_M) * (180 / Math.PI);
  return [lon, lat];
}

function vecSub(a: LocalPoint, b: LocalPoint): LocalPoint {
  return [a[0] - b[0], a[1] - b[1]];
}
function vecLen(a: LocalPoint): number {
  return Math.hypot(a[0], a[1]);
}
function vecNormalize(a: LocalPoint): LocalPoint {
  const l = vecLen(a);
  return l === 0 ? [0, 0] : [a[0] / l, a[1] / l];
}

/** The arc replacing the sharp corner at `curr`, tangent to both
 * `prev->curr` and `curr->next`, at `TURN_RADIUS_M` (shrunk if either
 * adjacent leg is too short to fit it). Empty if the turn is negligible
 * or a leg is degenerate (zero-length). */
function filletCorner(prev: [number, number], curr: [number, number], next: [number, number]): [number, number][] {
  const [refLon, refLat] = curr;
  const p = toLocalMeters(prev[0], prev[1], refLon, refLat);
  const c: LocalPoint = [0, 0];
  const n = toLocalMeters(next[0], next[1], refLon, refLat);

  const lenBefore = vecLen(vecSub(c, p));
  const lenAfter = vecLen(vecSub(n, c));
  if (lenBefore < 1e-6 || lenAfter < 1e-6) return [];

  const vIn = vecNormalize(vecSub(c, p));
  const vOut = vecNormalize(vecSub(n, c));
  const dot = Math.max(-1, Math.min(1, vIn[0] * vOut[0] + vIn[1] * vOut[1]));
  const turnAngle = Math.acos(dot);
  if (turnAngle < MIN_FILLET_TURN_RAD) return [];

  const idealTangent = TURN_RADIUS_M * Math.tan(turnAngle / 2);
  const maxTangent = Math.min(lenBefore, lenAfter) * FILLET_TANGENT_SAFETY_FRACTION;
  const tangentDist = Math.min(idealTangent, maxTangent);
  if (tangentDist < 1) return [];
  const effectiveRadius = tangentDist / Math.tan(turnAngle / 2);

  const t1: LocalPoint = [c[0] - vIn[0] * tangentDist, c[1] - vIn[1] * tangentDist];
  const t2: LocalPoint = [c[0] + vOut[0] * tangentDist, c[1] + vOut[1] * tangentDist];

  // Which side the turn bends toward, so the arc bulges the right way
  // and sweeps in the right direction.
  const cross = vIn[0] * vOut[1] - vIn[1] * vOut[0];
  const turnSign = cross > 0 ? 1 : -1;
  const perpIn: LocalPoint = turnSign > 0 ? [-vIn[1], vIn[0]] : [vIn[1], -vIn[0]];
  const center: LocalPoint = [t1[0] + perpIn[0] * effectiveRadius, t1[1] + perpIn[1] * effectiveRadius];

  const startAngle = Math.atan2(t1[1] - center[1], t1[0] - center[0]);
  const endAngle = Math.atan2(t2[1] - center[1], t2[0] - center[0]);
  let sweep = endAngle - startAngle;
  if (turnSign > 0) {
    while (sweep < 0) sweep += 2 * Math.PI;
  } else {
    while (sweep > 0) sweep -= 2 * Math.PI;
  }

  const steps = Math.max(4, Math.ceil(Math.abs(sweep) / (Math.PI / 16)));
  const arc: [number, number][] = [];
  for (let i = 0; i <= steps; i++) {
    const a = startAngle + (sweep * i) / steps;
    const x = center[0] + effectiveRadius * Math.cos(a);
    const y = center[1] + effectiveRadius * Math.sin(a);
    arc.push(fromLocalMeters(x, y, refLon, refLat));
  }
  return arc;
}

/** Replaces every interior vertex of a lon/lat path with a fillet arc
 * (see `filletCorner`) — endpoints are left as-is, only turns in
 * between are rounded. `vertexStart[i]` is the index into `coords`
 * where input point `i`'s arc (or the point itself, if unfilleted)
 * begins — callers that need to split the path at a particular input
 * vertex (e.g. procedureGeoJson splitting solid/dashed at the runway)
 * must split the *curved* output there, not at `i` itself: the runway
 * vertex is exactly the kind of sharp turn this exists to round, and a
 * naive split at the original vertex would cut the path exactly where
 * fillet has already replaced it with several arc points, leaving the
 * turn unrounded on whichever side lost them. */
function filletPolyline(points: [number, number][]): { coords: [number, number][]; vertexStart: number[] } {
  if (points.length < 3) {
    return { coords: points, vertexStart: points.map((_, i) => i) };
  }
  const coords: [number, number][] = [points[0]];
  const vertexStart: number[] = [0];
  for (let i = 1; i < points.length - 1; i++) {
    vertexStart.push(coords.length);
    const arc = filletCorner(points[i - 1], points[i], points[i + 1]);
    if (arc.length === 0) {
      coords.push(points[i]);
    } else {
      coords.push(...arc);
    }
  }
  vertexStart.push(coords.length);
  coords.push(points[points.length - 1]);
  return { coords, vertexStart };
}

/** Splits each transition's path at its runway-threshold leg (the
 * resolved "RW<ident>" pseudo-fix — see ff-api's procedure_detail) into
 * an "approach" segment (solid) and, if anything follows the runway fix,
 * a "missed" segment (dashed). CIFP doesn't reliably put missed-approach
 * legs in their own transition: the 'Z' route type ff-cifp's parser
 * recognizes for that is real per the ARINC 424 spec, but confirmed
 * against a real nationwide CIFP cycle that it never actually appears —
 * missed-approach legs land in the same "common" transition as the
 * final approach course, with no field the backend currently parses to
 * mark the boundary. The runway leg is the one boundary that's always
 * there and always resolvable, so it's what this splits on instead. */
function procedureGeoJson(detail: ProcedureDetail): GeoJSON.FeatureCollection {
  const features: GeoJSON.Feature[] = [];
  for (const t of detail.transitions) {
    const points: { lat: number; lon: number }[] = [];
    let runwayIndex: number | null = null;
    for (const leg of t.legs) {
      if (!leg.fix_ident) continue;
      const fix = detail.fixes[leg.fix_ident];
      if (!fix) continue;
      points.push(fix);
      if (leg.fix_ident.startsWith("RW")) {
        runwayIndex = points.length - 1;
      }
    }
    if (points.length < 2) continue;
    // Fillet the whole transition as one path — including across the
    // runway/missed-approach boundary — so the turn *at* the runway
    // (typically the sharpest one on the whole procedure) gets rounded
    // too, then split the resulting curve rather than the straight
    // input. Splitting the input first (as an earlier version of this
    // did) made the runway vertex an endpoint of both halves, and
    // endpoints never get filleted, so the turn onto the missed
    // approach never rounded no matter what.
    const { coords, vertexStart } = filletPolyline(points.map((p): [number, number] => [p.lon, p.lat]));
    if (runwayIndex === null) {
      features.push({
        type: "Feature",
        geometry: { type: "LineString", coordinates: coords },
        properties: { transitionId: t.id, segment: "approach" },
      });
    } else {
      const splitAt = vertexStart[runwayIndex];
      const approachCoords = coords.slice(0, splitAt + 1);
      const missedCoords = coords.slice(splitAt);
      if (approachCoords.length >= 2) {
        features.push({
          type: "Feature",
          geometry: { type: "LineString", coordinates: approachCoords },
          properties: { transitionId: t.id, segment: "approach" },
        });
      }
      if (missedCoords.length >= 2) {
        features.push({
          type: "Feature",
          geometry: { type: "LineString", coordinates: missedCoords },
          properties: { transitionId: t.id, segment: "missed" },
        });
      }
    }
  }
  return { type: "FeatureCollection", features };
}

/** One marker per distinct resolvable fix the procedure's legs actually
 * reference (waypoint, navaid, or a runway threshold resolved server-
 * side — see ff-api's procedure_detail), labeled with its ident and,
 * where CIFP encodes one, its altitude restriction. The same fix can
 * appear in more than one transition (e.g. an IAF that's also the start
 * of the common/missed segment) — first altitude constraint seen wins
 * rather than stacking duplicate markers at the same coordinate. */
function procedureFixesGeoJson(detail: ProcedureDetail): GeoJSON.FeatureCollection {
  const seen = new Map<string, string | null>();
  for (const t of detail.transitions) {
    for (const leg of t.legs) {
      if (!leg.fix_ident || !detail.fixes[leg.fix_ident]) continue;
      if (!seen.has(leg.fix_ident)) {
        seen.set(leg.fix_ident, leg.altitude_constraint);
      } else if (!seen.get(leg.fix_ident) && leg.altitude_constraint) {
        seen.set(leg.fix_ident, leg.altitude_constraint);
      }
    }
  }
  const features: GeoJSON.Feature[] = [];
  for (const [ident, altitude] of seen) {
    const fix = detail.fixes[ident];
    features.push({
      type: "Feature",
      geometry: { type: "Point", coordinates: [fix.lon, fix.lat] },
      properties: { label: altitude ? `${ident}\n${altitude}` : ident },
    });
  }
  return { type: "FeatureCollection", features };
}

/** Below this zoom the map doesn't show airport markers at all — a
 * nationwide bundle has ~13k airports, and a CONUS-wide marker soup is
 * useless as well as slow. */
const AIRPORT_MIN_ZOOM = 6;

/** Cap on how many visible airports get a METAR flight-category lookup
 * per refresh — one batched request, but aviationweather.gov shouldn't
 * be asked for hundreds of stations every pan. */
const MAX_METAR_AIRPORTS = 60;

/** Fetches the airports for the map's current view (bbox query, §4.1)
 * and refreshes the marker/winds-aloft sources; colors markers by METAR
 * flight category for up to MAX_METAR_AIRPORTS of them, read from/merged
 * into `flightCategoriesRef` rather than a fresh map each call — this
 * runs on every moveend, and a blank map would flash every marker back
 * to the default color on every pan/zoom while that view's METARs
 * re-fetch, even for stations whose category is already known. Keeps
 * the fetched list in `visibleAirportsRef` so the click handler can hand
 * a full Airport object to the app. */
async function refreshVisibleAirports(
  map: MlMap,
  visibleAirportsRef: { current: Airport[] },
  windsBulletinRef: { current: WindsAloftBulletin | null },
  flightCategoriesRef: { current: Map<string, string> },
  selectedAltitudeFtRef: { current: number },
  unmountedRef: { current: boolean },
) {
  const clear = () => {
    visibleAirportsRef.current = [];
    (map.getSource(AIRPORTS_SOURCE) as maplibregl.GeoJSONSource | undefined)?.setData(EMPTY_COLLECTION);
    (map.getSource(WINDS_ALOFT_SOURCE) as maplibregl.GeoJSONSource | undefined)?.setData(EMPTY_COLLECTION);
  };
  if (map.getZoom() < AIRPORT_MIN_ZOOM) {
    clear();
    return;
  }
  const bounds = map.getBounds();
  const bbox = `${bounds.getWest()},${bounds.getSouth()},${bounds.getEast()},${bounds.getNorth()}`;
  let airports: Airport[];
  try {
    airports = await fetchAirportsInBbox(bbox);
  } catch (err) {
    console.warn("couldn't load airports for the map view", err);
    return;
  }
  // The view (MapView switched away from — see App.tsx's map/plan
  // toggle) may have unmounted and torn down `map` while that fetch was
  // in flight; touching a removed map throws.
  if (unmountedRef.current) return;
  visibleAirportsRef.current = airports;
  // Uses whatever's already cached (from earlier calls, possibly for a
  // different view) rather than blanking every marker to the default
  // color while this view's own METARs are still in flight.
  (map.getSource(AIRPORTS_SOURCE) as maplibregl.GeoJSONSource | undefined)?.setData(
    airportsGeoJson(airports, flightCategoriesRef.current),
  );
  if (windsBulletinRef.current) {
    (map.getSource(WINDS_ALOFT_SOURCE) as maplibregl.GeoJSONSource | undefined)?.setData(
      windsAloftGeoJson(windsBulletinRef.current, airports, selectedAltitudeFtRef.current),
    );
  }

  try {
    const metars = await fetchMetars(airports.slice(0, MAX_METAR_AIRPORTS).map((a) => a.icao));
    if (unmountedRef.current) return;
    for (const m of metars) {
      if (m.fltCat) flightCategoriesRef.current.set(m.icaoId, m.fltCat);
    }
    // The view may have moved on while the METARs were in flight — only
    // apply if these airports are still the current set.
    if (visibleAirportsRef.current === airports) {
      (map.getSource(AIRPORTS_SOURCE) as maplibregl.GeoJSONSource | undefined)?.setData(
        airportsGeoJson(airports, flightCategoriesRef.current),
      );
    }
  } catch (err) {
    console.warn("couldn't load METAR flight categories for the map", err);
  }
}

/** Airspace boundaries are view-driven (bbox query per moveend, same as
 * airports) but not zoom-gated the way airports are — a Class B/C/D or
 * SUA boundary is relevant situational awareness at any zoom, and a
 * bbox at a low zoom still only returns what's actually in view rather
 * than nationwide, so there's no marker-soup-style volume problem here. */
async function refreshVisibleAirspace(map: MlMap, unmountedRef: { current: boolean }) {
  const bounds = map.getBounds();
  const bbox = `${bounds.getWest()},${bounds.getSouth()},${bounds.getEast()},${bounds.getNorth()}`;
  try {
    const volumes = await fetchAirspaceInBbox(bbox);
    if (unmountedRef.current) return;
    (map.getSource(AIRSPACE_SOURCE) as maplibregl.GeoJSONSource | undefined)?.setData(airspaceGeoJson(volumes));
  } catch (err) {
    console.warn("couldn't load airspace boundaries for the map view", err);
  }
}

/** Fetches the CONUS-wide hazard overlays and the winds-aloft bulletin
 * (cached in `windsBulletinRef` for reuse as the view moves). Each is
 * independent, so one failing doesn't block the others. */
async function loadWeatherOverlays(
  map: MlMap,
  windsBulletinRef: { current: WindsAloftBulletin | null },
  unmountedRef: { current: boolean },
) {
  try {
    const gairmets = await fetchGairmets();
    if (unmountedRef.current) return;
    (map.getSource(GAIRMET_SOURCE) as maplibregl.GeoJSONSource | undefined)?.setData(gairmetGeoJson(gairmets));
  } catch (err) {
    console.warn("couldn't load G-AIRMETs for the map", err);
  }

  try {
    const sigmets = await fetchSigmets();
    if (unmountedRef.current) return;
    (map.getSource(SIGMET_SOURCE) as maplibregl.GeoJSONSource | undefined)?.setData(sigmetGeoJson(sigmets));
  } catch (err) {
    console.warn("couldn't load SIGMETs for the map", err);
  }

  try {
    windsBulletinRef.current = await fetchWindsAloft("low", "06", "all");
  } catch (err) {
    console.warn("couldn't load winds aloft for the map", err);
  }
}

export function MapView({
  selectedAirport,
  onSelectAirport,
  selectedProcedureId,
  visible,
  route,
  preferredAltitudeFt,
}: {
  selectedAirport: Airport | null;
  onSelectAirport: (airport: Airport) => void;
  selectedProcedureId: string | null;
  /** Whether this is the currently-shown view. App.tsx keeps MapView
   * mounted (rather than conditionally rendering it) even while the
   * Flight Plan view is showing, toggling visibility via CSS instead —
   * unmounting it on every switch used to destroy the Flight Plan
   * view's own state the same way (the actual bug this prop exists to
   * let App.tsx avoid). MapLibre doesn't repaint correctly after its
   * container was `display: none`, so this drives an explicit
   * `resize()` when the map becomes visible again. */
  visible: boolean;
  /** The Flight Plan view's expanded route points (tokens are expanded
   * in App.tsx — see planning/expandRoute.ts) — drawn in cyan. */
  route: RouteWaypoint[];
  /** The flight plan's aircraft profile cruise altitude, if one's been
   * set — snaps the winds-aloft altitude selector to the nearest level
   * that actually has data the first time it becomes available (see the
   * effect below), rather than fighting a later manual pick on the map
   * every time this changes. */
  preferredAltitudeFt: number | null;
}) {
  const containerRef = useRef<HTMLDivElement>(null);
  const mapRef = useRef<MlMap | null>(null);
  const visibleAirportsRef = useRef<Airport[]>([]);
  const windsBulletinRef = useRef<WindsAloftBulletin | null>(null);
  const [selectedAltitudeFt, setSelectedAltitudeFt] = useState(DEFAULT_WINDS_ALOFT_ALTITUDE_FT);
  // refreshVisibleAirports is called from event handlers (moveend) set
  // up once on mount, so it needs a ref to read whatever the current
  // selection is rather than closing over a stale value from mount time.
  const selectedAltitudeFtRef = useRef(DEFAULT_WINDS_ALOFT_ALTITUDE_FT);
  // Snaps the altitude selector to the flight plan's cruise altitude
  // exactly once, the first time it becomes available — after that the
  // user has full manual control via the dropdown, so setting a cruise
  // altitude later doesn't yank the map's selection out from under them.
  const hasSyncedAltitudeRef = useRef(false);
  useEffect(() => {
    if (preferredAltitudeFt === null || hasSyncedAltitudeRef.current) return;
    hasSyncedAltitudeRef.current = true;
    const nearest = nearestWindsAloftLevel(preferredAltitudeFt);
    setSelectedAltitudeFt(nearest);
    selectedAltitudeFtRef.current = nearest;
    const map = mapRef.current;
    if (map && windsBulletinRef.current) {
      (map.getSource(WINDS_ALOFT_SOURCE) as maplibregl.GeoJSONSource | undefined)?.setData(
        windsAloftGeoJson(windsBulletinRef.current, visibleAirportsRef.current, nearest),
      );
    }
  }, [preferredAltitudeFt]);
  // Accumulates flight categories across every refreshVisibleAirports
  // call (never reset) so a station keeps its last-known color the
  // instant the view changes again, rather than flashing back to the
  // default blue on every moveend while its METAR re-fetches — see
  // refreshVisibleAirports for where this gets read/merged.
  const flightCategoriesRef = useRef<Map<string, string>>(new Map());
  // Set in this effect's cleanup so in-flight fetches from the initial
  // load (which don't run through a "cancelled" closure the way the
  // other effects do) don't touch `map` after App.tsx's map/plan toggle
  // has unmounted this component and removed it.
  const unmountedRef = useRef(false);
  const [loaded, setLoaded] = useState(false);
  // Which chart-catalog `kind`s exist (for rendering toggle buttons) and
  // which layer ids belong to each (so a toggle click can flip every
  // layer of that kind) — populated once the chart-loading effect below
  // gets a response, read imperatively from the click handler rather
  // than through React state since it never needs to trigger a re-render
  // itself.
  const chartLayerIdsByKindRef = useRef<Map<string, string[]>>(new Map());
  const [chartKinds, setChartKinds] = useState<string[]>([]);
  const [visibleChartKinds, setVisibleChartKinds] = useState<Set<string>>(new Set(["Sectional"]));

  useEffect(() => {
    if (!containerRef.current) return;
    // React StrictMode double-invokes this effect in dev (mount, cleanup,
    // mount again) on the same component instance — the cleanup below
    // sets this to true, and since useRef's initial value only applies
    // once ever (not per-mount), it has to be reset here or every async
    // callback on the real second mount permanently thinks it's stale.
    unmountedRef.current = false;
    const map = new maplibregl.Map({
      container: containerRef.current,
      style: BASEMAP_STYLE_URL,
      center: [-98, 39],
      zoom: 3,
    });
    mapRef.current = map;
    map.addControl(new maplibregl.NavigationControl(), "top-right");

    map.on("load", () => {
      map.addSource(AIRPORTS_SOURCE, { type: "geojson", data: EMPTY_COLLECTION });
      map.addLayer({
        id: "airports-circle",
        type: "circle",
        source: AIRPORTS_SOURCE,
        paint: {
          "circle-radius": 5,
          "circle-color": [
            "match",
            ["get", "fltCat"],
            "VFR",
            FLIGHT_CATEGORY_COLORS.VFR,
            "MVFR",
            FLIGHT_CATEGORY_COLORS.MVFR,
            "IFR",
            FLIGHT_CATEGORY_COLORS.IFR,
            "LIFR",
            FLIGHT_CATEGORY_COLORS.LIFR,
            DEFAULT_AIRPORT_COLOR,
          ],
          "circle-stroke-color": "#0b1220",
          "circle-stroke-width": 1.5,
        },
      });
      map.addLayer({
        id: "airports-label",
        type: "symbol",
        source: AIRPORTS_SOURCE,
        layout: {
          "text-field": ["get", "icao"],
          "text-size": 11,
          "text-offset": [0, 1.1],
          "text-anchor": "top",
        },
        paint: { "text-color": "#c8d6e5", "text-halo-color": "#0b1220", "text-halo-width": 1 },
      });

      map.addSource(RUNWAYS_SOURCE, { type: "geojson", data: EMPTY_COLLECTION });
      map.addLayer({
        id: "runways-line",
        type: "line",
        source: RUNWAYS_SOURCE,
        paint: { "line-color": "#ffb020", "line-width": 3 },
      });

      // Two layers, not one data-driven one: MapLibre's line-dasharray
      // isn't a supported data-driven (per-feature) property, so solid
      // "approach" segments and dashed "missed" segments (see
      // procedureGeoJson) need their own layers, filtered by the
      // `segment` property, sharing one source.
      map.addSource(PROCEDURE_SOURCE, { type: "geojson", data: EMPTY_COLLECTION });
      map.addLayer({
        id: "procedure-line",
        type: "line",
        source: PROCEDURE_SOURCE,
        filter: ["!=", ["get", "segment"], "missed"],
        paint: { "line-color": "#e254e0", "line-width": 4 },
      });
      map.addLayer({
        id: "procedure-line-missed",
        type: "line",
        source: PROCEDURE_SOURCE,
        filter: ["==", ["get", "segment"], "missed"],
        paint: { "line-color": "#e254e0", "line-width": 4, "line-dasharray": [2, 1.5] },
      });

      // The Flight Plan view's route, kept in sync from App.tsx's lifted
      // `route` state regardless of which view is currently shown.
      map.addSource(PLANNED_ROUTE_SOURCE, { type: "geojson", data: EMPTY_COLLECTION });
      map.addLayer({
        id: "planned-route-line",
        type: "line",
        source: PLANNED_ROUTE_SOURCE,
        paint: { "line-color": "#22d3ee", "line-width": 3 },
      });
      map.addSource(PLANNED_ROUTE_FIXES_SOURCE, { type: "geojson", data: EMPTY_COLLECTION });
      map.addLayer({
        id: "planned-route-fix-circle",
        type: "circle",
        source: PLANNED_ROUTE_FIXES_SOURCE,
        paint: {
          "circle-radius": 4,
          "circle-color": "#22d3ee",
          "circle-stroke-color": "#0b1220",
          "circle-stroke-width": 1.5,
        },
      });
      map.addLayer({
        id: "planned-route-fix-label",
        type: "symbol",
        source: PLANNED_ROUTE_FIXES_SOURCE,
        layout: {
          "text-field": ["get", "label"],
          "text-size": 11,
          "text-offset": [0, 1.1],
          "text-anchor": "top",
          "text-justify": "center",
        },
        paint: { "text-color": "#22d3ee", "text-halo-color": "#0b1220", "text-halo-width": 1.2 },
      });

      // One marker + label per fix the selected procedure's legs actually
      // reference (see procedureFixesGeoJson) — the label is ident alone,
      // or ident + a second line with the CIFP altitude restriction when
      // one exists for that leg.
      map.addSource(PROCEDURE_FIXES_SOURCE, { type: "geojson", data: EMPTY_COLLECTION });
      map.addLayer({
        id: "procedure-fix-circle",
        type: "circle",
        source: PROCEDURE_FIXES_SOURCE,
        paint: {
          "circle-radius": 4,
          "circle-color": "#f2e6c9",
          "circle-stroke-color": "#0b1220",
          "circle-stroke-width": 1.5,
        },
      });
      map.addLayer({
        id: "procedure-fix-label",
        type: "symbol",
        source: PROCEDURE_FIXES_SOURCE,
        layout: {
          "text-field": ["get", "label"],
          "text-size": 11,
          "text-offset": [0, 1.1],
          "text-anchor": "top",
          "text-justify": "center",
        },
        paint: { "text-color": "#f2e6c9", "text-halo-color": "#0b1220", "text-halo-width": 1.2 },
      });

      // Chart imagery renders as the base layer, under the airspace/
      // weather/airport overlays added below -- inserted before
      // "airspace-fill" specifically, not just "airports-circle": this
      // whole fetch runs async (fetchCharts() is a network round trip),
      // so its .then() always fires as a microtask *after* the
      // synchronous airspace/G-AIRMET/SIGMET layer-adding code further
      // down in this same "load" handler has already run. addLayer(...,
      // beforeId) inserts immediately before that anchor, so anchoring
      // to "airports-circle" (added once, at the very top of the stack)
      // would put charts *above* those already-inserted overlays —
      // exactly the bug this caused (airspace polygons visually
      // disappearing once sectionals painted in). Anchoring to
      // "airspace-fill" instead keeps charts pinned below the whole
      // overlay stack regardless of how the async timing falls out.
      // The catalog comes from ff-api (tile_url is an ff-api path like
      // /bundles/<cycle>/chart.pmtiles, fetched by the pmtiles protocol
      // via HTTP range requests); fetched async, layers added on arrival.
      // Grouped by `kind` (Sectional/IfrEnrouteLow/IfrEnrouteHigh/...) so
      // the toggle control below can show/hide a whole chart series at
      // once — only Sectional starts visible, matching pre-IFR-chart
      // behavior for existing users; the others are opt-in since
      // stacking every chart series at full opacity by default would
      // just be visual noise.
      void fetchCharts()
        .then((charts) => {
          const layerIdsByKind = new Map<string, string[]>();
          for (const chart of charts) {
            const sourceId = `chart-${chart.id}`;
            map.addSource(sourceId, {
              type: "raster",
              url: `pmtiles://${API_BASE_URL}${chart.tile_url}`,
              tileSize: 256,
            });
            map.addLayer(
              {
                id: sourceId,
                type: "raster",
                source: sourceId,
                layout: { visibility: chart.kind === "Sectional" ? "visible" : "none" },
              },
              "airspace-fill",
            );
            const layerIds = layerIdsByKind.get(chart.kind) ?? [];
            layerIds.push(sourceId);
            layerIdsByKind.set(chart.kind, layerIds);
          }
          chartLayerIdsByKindRef.current = layerIdsByKind;
          setChartKinds([...layerIdsByKind.keys()].sort());
        })
        .catch((err: unknown) => console.warn("couldn't load the chart catalog for the map", err));

      // Airspace boundaries render above chart imagery (which anchors
      // itself below this layer specifically — see the chart loop above)
      // but below weather hazards/airports, same insertion point (before
      // "airports-circle") as the G-AIRMET/SIGMET sources below. Filled
      // lightly so overlapping shelves (a busy Class B/C stacks several)
      // are still readable rather than opaque.
      const airspaceColorExpr: maplibregl.ExpressionSpecification = [
        "match",
        ["get", "class"],
        "B",
        AIRSPACE_CLASS_COLORS.B,
        "C",
        AIRSPACE_CLASS_COLORS.C,
        "D",
        AIRSPACE_CLASS_COLORS.D,
        "MOA",
        AIRSPACE_CLASS_COLORS.MOA,
        "RESTRICTED",
        AIRSPACE_CLASS_COLORS.RESTRICTED,
        "PROHIBITED",
        AIRSPACE_CLASS_COLORS.PROHIBITED,
        "WARNING",
        AIRSPACE_CLASS_COLORS.WARNING,
        "ALERT",
        AIRSPACE_CLASS_COLORS.ALERT,
        DEFAULT_AIRSPACE_COLOR,
      ];
      map.addSource(AIRSPACE_SOURCE, { type: "geojson", data: EMPTY_COLLECTION });
      map.addLayer(
        {
          id: "airspace-fill",
          type: "fill",
          source: AIRSPACE_SOURCE,
          paint: { "fill-color": airspaceColorExpr, "fill-opacity": 0.05 },
        },
        "airports-circle",
      );
      map.addLayer(
        {
          id: "airspace-line",
          type: "line",
          source: AIRSPACE_SOURCE,
          paint: { "line-color": airspaceColorExpr, "line-width": 1.5 },
        },
        "airports-circle",
      );

      // G-AIRMET/SIGMET overlays render above chart imagery but below the
      // airport markers, inserted before "airports-circle" same as charts
      // (added after the chart loop above, so they end up above charts —
      // insertion order relative to a shared beforeId determines stacking).
      const gairmetColorExpr: maplibregl.ExpressionSpecification = [
        "match",
        ["get", "hazard"],
        "TURB",
        GAIRMET_HAZARD_COLORS.TURB,
        "ICE",
        GAIRMET_HAZARD_COLORS.ICE,
        "MT_OBSC",
        GAIRMET_HAZARD_COLORS.MT_OBSC,
        "IFR",
        GAIRMET_HAZARD_COLORS.IFR,
        "FZLVL",
        GAIRMET_HAZARD_COLORS.FZLVL,
        "SFC_WND",
        GAIRMET_HAZARD_COLORS.SFC_WND,
        DEFAULT_GAIRMET_COLOR,
      ];
      map.addSource(GAIRMET_SOURCE, { type: "geojson", data: EMPTY_COLLECTION });
      map.addLayer(
        {
          id: "gairmet-fill",
          type: "fill",
          source: GAIRMET_SOURCE,
          filter: ["==", ["geometry-type"], "Polygon"],
          paint: { "fill-color": gairmetColorExpr, "fill-opacity": 0.15 },
        },
        "airports-circle",
      );
      map.addLayer(
        {
          id: "gairmet-line",
          type: "line",
          source: GAIRMET_SOURCE,
          paint: { "line-color": gairmetColorExpr, "line-width": 1.5, "line-dasharray": [3, 2] },
        },
        "airports-circle",
      );

      map.addSource(SIGMET_SOURCE, { type: "geojson", data: EMPTY_COLLECTION });
      map.addLayer(
        {
          id: "sigmet-fill",
          type: "fill",
          source: SIGMET_SOURCE,
          paint: { "fill-color": "#e5484d", "fill-opacity": 0.2 },
        },
        "airports-circle",
      );
      map.addLayer(
        {
          id: "sigmet-line",
          type: "line",
          source: SIGMET_SOURCE,
          paint: { "line-color": "#e5484d", "line-width": 2 },
        },
        "airports-circle",
      );

      // Winds-aloft arrows render on top of everything (appended with no
      // beforeId) so they stay visible over airport markers/labels.
      map.addSource(WINDS_ALOFT_SOURCE, { type: "geojson", data: EMPTY_COLLECTION });
      map.addLayer({
        id: "winds-aloft-arrow",
        type: "symbol",
        source: WINDS_ALOFT_SOURCE,
        filter: ["==", ["get", "lightAndVariable"], false],
        layout: {
          "text-field": "➤",
          "text-size": 18,
          "text-rotate": ["get", "arrowRotation"],
          "text-rotation-alignment": "map",
          "text-allow-overlap": true,
          // No offset: `text-offset` is applied in the glyph's own
          // rotated frame when combined with `text-rotate` (confirmed
          // live — a non-zero offset here rotated along with the wind
          // direction, while the label's offset below doesn't rotate,
          // so the two drifted apart depending on which way the wind
          // happened to be blowing at each station). Anchoring at [0,0]
          // means rotating in place around the airport's own point —
          // no drift regardless of direction.
        },
        paint: { "text-color": "#ffe066", "text-halo-color": "#0b1220", "text-halo-width": 1.2 },
      });
      map.addLayer({
        id: "winds-aloft-label",
        type: "symbol",
        source: WINDS_ALOFT_SOURCE,
        layout: {
          "text-field": ["get", "label"],
          "text-size": 10,
          // Not rotated, so unlike the arrow above this offset is stable
          // in screen space — placed just above-right of the (now
          // point-centered) arrow, clear of the airport's own ICAO
          // label which sits below the point instead.
          "text-offset": [1.1, -1.5],
          "text-allow-overlap": true,
        },
        paint: { "text-color": "#ffe066", "text-halo-color": "#0b1220", "text-halo-width": 1 },
      });

      map.on("click", "airports-circle", (e) => {
        const icao = e.features?.[0]?.properties?.icao as string | undefined;
        const airport = icao ? visibleAirportsRef.current.find((a) => a.icao === icao) : undefined;
        if (airport) onSelectAirport(airport);
      });
      map.on("mouseenter", "airports-circle", () => {
        map.getCanvas().style.cursor = "pointer";
      });
      map.on("mouseleave", "airports-circle", () => {
        map.getCanvas().style.cursor = "";
      });

      // Airport markers are view-driven (bbox query per moveend, hidden
      // below AIRPORT_MIN_ZOOM) — a nationwide bundle is too big to draw
      // whole. Weather overlays load once; the winds bulletin is cached
      // and re-applied to whatever airports are in view. Each fetch is
      // independent so one failing doesn't block the others.
      void loadWeatherOverlays(map, windsBulletinRef, unmountedRef).then(
        () =>
          void refreshVisibleAirports(
            map,
            visibleAirportsRef,
            windsBulletinRef,
            flightCategoriesRef,
            selectedAltitudeFtRef,
            unmountedRef,
          ),
      );
      void refreshVisibleAirspace(map, unmountedRef);
      map.on("moveend", () => {
        void refreshVisibleAirports(
          map,
          visibleAirportsRef,
          windsBulletinRef,
          flightCategoriesRef,
          selectedAltitudeFtRef,
          unmountedRef,
        );
        void refreshVisibleAirspace(map, unmountedRef);
      });

      setLoaded(true);
    });

    return () => {
      unmountedRef.current = true;
      map.remove();
      mapRef.current = null;
      setLoaded(false);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    const map = mapRef.current;
    if (!map || !loaded || !visible) return;
    map.resize();
  }, [visible, loaded]);

  useEffect(() => {
    const map = mapRef.current;
    if (!map || !loaded) return;
    (map.getSource(PLANNED_ROUTE_SOURCE) as maplibregl.GeoJSONSource | undefined)?.setData(
      plannedRouteGeoJson(route),
    );
    (map.getSource(PLANNED_ROUTE_FIXES_SOURCE) as maplibregl.GeoJSONSource | undefined)?.setData(
      plannedRouteFixesGeoJson(route),
    );
  }, [route, loaded]);

  useEffect(() => {
    const map = mapRef.current;
    // Gated on `visible` (like the resize() above, and for the same
    // reason): a route built in the Flight Plan view never otherwise
    // moves the camera, so switching to Map leaves it wherever it was —
    // typically the initial nationwide view, whose zoom is too far out
    // for chart raster tiles to render at all. That reads as "the tiles
    // don't load," when the real issue is the map never looked at the
    // route. Computing the fit while hidden would use a zero-size
    // container and produce a bogus camera position, so this waits
    // until the view is actually shown.
    if (!map || !loaded || !visible || route.length === 0) return;
    if (route.length === 1) {
      map.flyTo({ center: [route[0].lon, route[0].lat], zoom: 10 });
      return;
    }
    const lons = route.map((p) => p.lon);
    const lats = route.map((p) => p.lat);
    map.fitBounds(
      [
        [Math.min(...lons), Math.min(...lats)],
        [Math.max(...lons), Math.max(...lats)],
      ],
      { padding: 60, duration: 800 },
    );
  }, [route, visible, loaded]);

  useEffect(() => {
    const map = mapRef.current;
    if (!map || !loaded) return;

    const setRunways = (runways: Runway[]) =>
      (map.getSource(RUNWAYS_SOURCE) as maplibregl.GeoJSONSource | undefined)?.setData(runwaysGeoJson(runways));

    let cancelled = false;
    if (selectedAirport) {
      fetchAirportDetail(selectedAirport.icao)
        .then((detail) => {
          if (!cancelled) setRunways(detail.runways);
        })
        .catch((err: unknown) => console.warn("couldn't load runways for the map", err));
      map.flyTo({ center: [selectedAirport.lon, selectedAirport.lat], zoom: 12, duration: 800 });
    } else {
      setRunways([]);
    }
    return () => {
      cancelled = true;
    };
  }, [selectedAirport, loaded]);

  useEffect(() => {
    const map = mapRef.current;
    if (!map || !loaded) return;

    const setPath = (data: GeoJSON.FeatureCollection) =>
      (map.getSource(PROCEDURE_SOURCE) as maplibregl.GeoJSONSource | undefined)?.setData(data);
    const setFixes = (data: GeoJSON.FeatureCollection) =>
      (map.getSource(PROCEDURE_FIXES_SOURCE) as maplibregl.GeoJSONSource | undefined)?.setData(data);

    let cancelled = false;
    if (selectedProcedureId) {
      fetchProcedureDetail(selectedProcedureId)
        .then((detail) => {
          if (cancelled) return;
          setPath(procedureGeoJson(detail));
          setFixes(procedureFixesGeoJson(detail));
        })
        .catch((err: unknown) => console.warn("couldn't load the procedure path for the map", err));
    } else {
      setPath(EMPTY_COLLECTION);
      setFixes(EMPTY_COLLECTION);
    }
    return () => {
      cancelled = true;
    };
  }, [selectedProcedureId, loaded]);

  const toggleChartKind = (kind: string) => {
    const map = mapRef.current;
    if (!map) return;
    setVisibleChartKinds((current) => {
      const next = new Set(current);
      const nowVisible = !next.has(kind);
      if (nowVisible) next.add(kind);
      else next.delete(kind);
      for (const layerId of chartLayerIdsByKindRef.current.get(kind) ?? []) {
        map.setLayoutProperty(layerId, "visibility", nowVisible ? "visible" : "none");
      }
      return next;
    });
  };

  const changeWindsAloftAltitude = (altitudeFt: number) => {
    setSelectedAltitudeFt(altitudeFt);
    selectedAltitudeFtRef.current = altitudeFt;
    const map = mapRef.current;
    if (map && windsBulletinRef.current) {
      (map.getSource(WINDS_ALOFT_SOURCE) as maplibregl.GeoJSONSource | undefined)?.setData(
        windsAloftGeoJson(windsBulletinRef.current, visibleAirportsRef.current, altitudeFt),
      );
    }
  };

  return (
    <div className="map-view">
      <div ref={containerRef} className="map-view-canvas" />
      <div className="chart-kind-toggle">
        {chartKinds.map((kind) => (
          <button
            key={kind}
            className={visibleChartKinds.has(kind) ? "selected" : ""}
            onClick={() => toggleChartKind(kind)}
          >
            {CHART_KIND_LABELS[kind] ?? kind}
          </button>
        ))}
        {/* Independent of chart-kind loading — winds-aloft is fetched
            eagerly on mount regardless of the chart catalog, so this
            shouldn't wait on chartKinds the way the toggle buttons do. */}
        <select
          className="winds-aloft-altitude-select"
          value={selectedAltitudeFt}
          onChange={(e) => changeWindsAloftAltitude(Number(e.target.value))}
          aria-label="Winds-aloft altitude"
        >
          {WINDS_ALOFT_LEVELS_FT.map((altitudeFt) => (
            <option key={altitudeFt} value={altitudeFt}>
              {altitudeFt.toLocaleString()} ft winds
            </option>
          ))}
        </select>
      </div>
    </div>
  );
}
