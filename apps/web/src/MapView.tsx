import { forwardRef, useEffect, useImperativeHandle, useRef, useState } from "react";
import maplibregl, { type Map as MlMap } from "maplibre-gl";
import "maplibre-gl/dist/maplibre-gl.css";
import { Protocol as PmtilesProtocol } from "pmtiles";
import { API_BASE_URL } from "./api";
import { loadMapView, saveMapView } from "./persistence";
import {
  fetchAirportDetail,
  fetchAirportsInBbox,
  fetchAirspaceInBbox,
  fetchCharts,
  fetchNearestFix,
  fetchProcedureDetail,
} from "./data";
import type {
  Airport,
  AirspaceVolume,
  ChartCatalogEntry,
  Cwa,
  GAirmet,
  MapTapResult,
  NearestFix,
  Pirep,
  ProcedureDetail,
  RouteWaypoint,
  Runway,
  Sigmet,
  WindsAloftBulletin,
} from "./types";
import { fetchCwas, fetchFlightCategories, fetchGairmets, fetchPireps, fetchSigmets, fetchWindsAloft } from "./weather";

const AIRPORTS_SOURCE = "airports";
const RUNWAYS_SOURCE = "runways";
const PROCEDURE_SOURCE = "procedure-path";
const PROCEDURE_FIXES_SOURCE = "procedure-fixes";
const GAIRMET_SOURCE = "gairmets";
const SIGMET_SOURCE = "sigmets";
const CWA_SOURCE = "cwas";
const PIREP_SOURCE = "pireps";
const RADAR_SOURCE = "radar";
const WINDS_ALOFT_SOURCE = "winds-aloft";
const AIRSPACE_SOURCE = "airspace";
const PLANNED_ROUTE_SOURCE = "planned-route";
const PLANNED_ROUTE_FIXES_SOURCE = "planned-route-fixes";
const SELECTED_AIRPORT_SOURCE = "selected-airport";
const TAP_POINTS_SOURCE = "tap-points";

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
  TerminalAreaChart: "TAC",
  VfrFlyway: "Flyway",
  HelicopterRoute: "Heli",
  IfrEnrouteLow: "IFR Low",
  IfrEnrouteHigh: "IFR High",
};

// Dropdown order for the base-chart selector — VFR (broad → terminal),
// then IFR, then the specialty heli charts. Kinds not listed fall to the
// end. Independent of the catalog's own alphabetical ordering.
const CHART_KIND_ORDER: Record<string, number> = {
  Sectional: 0,
  TerminalAreaChart: 1,
  VfrFlyway: 2,
  IfrEnrouteLow: 3,
  IfrEnrouteHigh: 4,
  HelicopterRoute: 5,
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

// PIREP severity is derived (see pirepSeverity) from the worst icing/
// turbulence intensity text reported, not a field the API gives us
// directly.
const PIREP_SEVERITY_COLORS: Record<string, string> = {
  SEVERE: "#e5484d",
  MODERATE: "#e0973f",
  LIGHT: "#e0c341",
  NONE: "#7fa8d9",
};

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

// NWS base-reflectivity radar mosaic (mapservices.weather.noaa.gov) — the
// National Weather Service's own WMS, public-domain NOAA data, no API key
// and no usage/commercial restriction (unlike RainViewer's free tier).
// Covers CONUS plus Alaska, Hawaii, the Caribbean and Guam, refreshed
// ~every 10 min. Consumed as a WMS raster source via MapLibre's
// {bbox-epsg-3857} tile template — the service advertises EPSG:3857 in its
// GetCapabilities, and for WMS 1.3.0 that CRS uses easting/northing (x,y)
// axis order, matching the template's minx,miny,maxx,maxy. png32 gives a
// real alpha channel so no-echo areas are transparent rather than a solid
// backdrop over the chart/basemap beneath. `layers` is the single layer
// the service exposes (confirmed against its GetCapabilities).
const RADAR_WMS_URL =
  "https://mapservices.weather.noaa.gov/eventdriven/services/radar/radar_base_reflectivity_time/ImageServer/WMSServer" +
  "?service=WMS&request=GetMap&version=1.3.0&layers=radar_base_reflectivity_time&styles=" +
  "&format=image/png32&transparent=true&crs=EPSG:3857&width=256&height=256&bbox={bbox-epsg-3857}";
const RADAR_LAYER = "radar-reflectivity";

// iOS detection for the pixelRatio cap below. iPadOS 13+ deliberately
// reports a macOS user agent, so UA sniffing alone misses modern iPads —
// "MacIntel with a touchscreen" is the standard tell (real Macs report
// maxTouchPoints 0).
const IS_IOS =
  /iPad|iPhone|iPod/.test(navigator.userAgent) ||
  (navigator.platform === "MacIntel" && navigator.maxTouchPoints > 1);

// iOS Safari kills a tab outright at its per-tab memory ceiling (see the
// maxTileCacheSize note at the Map constructor) — and render-buffer/tile
// texture memory scales with pixelRatio². Capping an iPad's dpr-2 canvas
// at 1.5 cuts that footprint ~1.8× for a mild softening of map text;
// capping to 1 would halve it again but makes chart fine print
// noticeably fuzzy, the wrong trade for a chart app. Desktop/Android
// keep native sharpness.
const MAX_IOS_PIXEL_RATIO = 1.5;

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
      properties: {
        hazard: r.hazard,
        tag: r.tag,
        severity: r.severity,
        base: r.base,
        top: r.top,
        fzlbase: r.fzlbase,
        fzltop: r.fzltop,
        validTime: r.validTime,
      },
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
      properties: {
        hazard: r.hazard,
        seriesId: r.seriesId,
        altitudeLow1: r.altitudeLow1,
        altitudeHi1: r.altitudeHi1,
        rawAirSigmet: r.rawAirSigmet,
      },
    })),
  };
}

/** Center Weather Advisories — always a closed-ring Polygon on live data
 * (only ever seen `geom: "AREA"`, no multi-area case like IntlSigmet
 * has); coords use the same string-lat/lon shape as GAirmetCoord. */
function cwaGeoJson(records: Cwa[]): GeoJSON.FeatureCollection {
  return {
    type: "FeatureCollection",
    features: records.map((r) => ({
      type: "Feature",
      geometry: {
        type: "Polygon",
        coordinates: [r.coords.map((c) => [parseFloat(c.lon), parseFloat(c.lat)])],
      },
      properties: { hazard: r.hazard, cwsu: r.cwsu, rawText: r.rawText },
    })),
  };
}

/** Worst of the icing/turbulence intensity codes a PIREP reports,
 * across both possible layers — the API gives free-text intensity
 * codes (e.g. "LGT-MOD", "SEV", "NEG"), not a severity enum, so this
 * classifies by substring match against the codes actually seen live.
 * An Urgent PIREP is always at least "SEVERE" regardless of what the
 * intensity text says, since the type itself signals a hazard serious
 * enough to need immediate attention. */
function pirepSeverity(r: Pirep): "SEVERE" | "MODERATE" | "LIGHT" | "NONE" {
  if (r.pirepType === "Urgent PIREP") return "SEVERE";
  const texts = [r.icgInt1, r.icgInt2, r.tbInt1, r.tbInt2].filter(Boolean).join(" ").toUpperCase();
  if (texts.includes("SEV") || texts.includes("EXTM")) return "SEVERE";
  if (texts.includes("MOD")) return "MODERATE";
  if (texts.includes("LGT") || texts.includes("LIGHT")) return "LIGHT";
  return "NONE";
}

/** Short one-line summary for a PIREP's popup — the raw text (`rawOb`)
 * is the authoritative source, this is just a quicker-to-scan header
 * above it. */
function pirepSummary(r: Pirep): string {
  const parts = [r.acType, r.fltLvl !== null ? `FL${r.fltLvl}` : null];
  const icing = [r.icgInt1, r.icgType1].filter(Boolean).join(" ");
  const turb = [r.tbInt1, r.tbType1].filter(Boolean).join(" ");
  if (icing) parts.push(`ICE ${icing}`);
  if (turb) parts.push(`TURB ${turb}`);
  return parts.filter(Boolean).join(" · ");
}

function pirepGeoJson(records: Pirep[]): GeoJSON.FeatureCollection {
  return {
    type: "FeatureCollection",
    features: records.map((r) => ({
      type: "Feature",
      geometry: { type: "Point", coordinates: [r.lon, r.lat] },
      properties: {
        severity: pirepSeverity(r),
        urgent: r.pirepType === "Urgent PIREP",
        summary: pirepSummary(r),
        rawOb: r.rawOb,
      },
    })),
  };
}

function formatLimit(limit: string): string {
  if (limit === "SFC") return "SFC";
  if (limit === "UNLTD") return "UNLTD";
  if (limit.startsWith("MSL:")) {
    return limit.slice(4);
  }
  if (limit.startsWith("AGL:")) {
    return `${limit.slice(4)} AGL`;
  }
  return limit;
}

function makeAirspaceLabel(cls: string, floor: string, ceiling: string): string {
  let prefix = cls;
  if (cls === "RESTRICTED") prefix = "R";
  else if (cls === "PROHIBITED") prefix = "P";
  else if (cls === "WARNING") prefix = "W";
  else if (cls === "ALERT") prefix = "A";

  const f = formatLimit(floor);
  const c = formatLimit(ceiling);
  return `${prefix}: ${f} ${c}`;
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
      properties: {
        id: v.id,
        name: v.name,
        class: v.class,
        floor: v.floor,
        ceiling: v.ceiling,
        label: makeAirspaceLabel(v.class, v.floor, v.ceiling),
      },
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

/** A single highlight ring around the currently-selected airport, so the
 * airport a tap picks stands out from the field of same-colored markers. */
function selectedAirportGeoJson(airport: Airport | null): GeoJSON.FeatureCollection {
  if (!airport) return EMPTY_COLLECTION;
  return {
    type: "FeatureCollection",
    features: [{ type: "Feature", geometry: { type: "Point", coordinates: [airport.lon, airport.lat] }, properties: {} }],
  };
}

/** The two waypoints a tap proposes (see App's Waypoint tab), marked on
 * the map so the picks are visible where they sit: the nearest real fix/
 * navaid, and the tapped point itself as a lat/lon "user" waypoint. */
function tapPointsGeoJson(
  lngLat: { lng: number; lat: number },
  nearestFix: NearestFix | null,
): GeoJSON.FeatureCollection {
  const features: GeoJSON.Feature[] = [];
  if (nearestFix) {
    features.push({
      type: "Feature",
      geometry: { type: "Point", coordinates: [nearestFix.lon, nearestFix.lat] },
      properties: { role: "waypoint", label: nearestFix.ident },
    });
  }
  features.push({
    type: "Feature",
    geometry: { type: "Point", coordinates: [lngLat.lng, lngLat.lat] },
    properties: { role: "user", label: "USER" },
  });
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

/** Below this zoom the map doesn't show any airport markers at all — a
 * nationwide bundle has ~13k airports, and a CONUS-wide marker soup is
 * useless as well as slow. */
const AIRPORT_MIN_ZOOM = 7;

/** Below this (higher) zoom, only airports with a current METAR flight
 * category show — airports with no weather station are the vast majority
 * of the ~13k bundle and just add clutter until you're zoomed in enough
 * to be looking at a specific area. */
const AIRPORT_NO_WEATHER_MIN_ZOOM = 8;

/** Shared by "airports-circle" and "airports-label" so the label for a
 * marker never outlives (or outlasts) the marker itself. */
const AIRPORT_DISPLAY_FILTER: maplibregl.FilterSpecification = [
  "all",
  [">=", ["zoom"], AIRPORT_MIN_ZOOM],
  ["any", ["!=", ["get", "fltCat"], null], [">=", ["zoom"], AIRPORT_NO_WEATHER_MIN_ZOOM]],
];

/** Fetches the airports for the map's current view (bbox query, §4.1)
 * and refreshes the marker/winds-aloft sources, coloring markers by
 * METAR flight category from `flightCategoriesRef` — the full
 * `station -> category` map that `loadFlightCategories` keeps current
 * from ff-api's bulk cache. Every visible airport with a current METAR
 * is colored (no per-view cap), and this makes no upstream request on
 * pan/zoom — it just reads whatever categories are already loaded. Keeps
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
}

/** Loads ff-api's bulk METAR flight-category map into
 * `flightCategoriesRef` and re-colors the airports currently in view.
 * The heavy lifting (one download of aviationweather.gov's ~5,000-station
 * cache, parsed and held in memory) happens backend-side and is refreshed
 * there every few minutes; the client just pulls the resulting small map.
 * Called once on load and then on an interval — a failed fetch leaves the
 * previous categories in place rather than blanking the markers. */
async function loadFlightCategories(
  map: MlMap,
  visibleAirportsRef: { current: Airport[] },
  flightCategoriesRef: { current: Map<string, string> },
  unmountedRef: { current: boolean },
) {
  let categories: Record<string, string>;
  try {
    categories = await fetchFlightCategories();
  } catch (err) {
    console.warn("couldn't load METAR flight categories for the map", err);
    return;
  }
  if (unmountedRef.current) return;
  flightCategoriesRef.current = new Map(Object.entries(categories));
  (map.getSource(AIRPORTS_SOURCE) as maplibregl.GeoJSONSource | undefined)?.setData(
    airportsGeoJson(visibleAirportsRef.current, flightCategoriesRef.current),
  );
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

/** PIREPs are view-driven the same way (bbox query per moveend) —
 * unlike G-AIRMET/SIGMET/CWA, aviationweather.gov's `/pirep` requires a
 * bbox to begin with (see ff-weather's fetch_pireps), so there's no
 * "fetch once for all current records" option here even if we wanted
 * one. */
async function refreshVisiblePireps(map: MlMap, unmountedRef: { current: boolean }) {
  const bounds = map.getBounds();
  const bbox = `${bounds.getWest()},${bounds.getSouth()},${bounds.getEast()},${bounds.getNorth()}`;
  try {
    const pireps = await fetchPireps(bbox);
    if (unmountedRef.current) return;
    (map.getSource(PIREP_SOURCE) as maplibregl.GeoJSONSource | undefined)?.setData(pirepGeoJson(pireps));
  } catch (err) {
    console.warn("couldn't load PIREPs for the map view", err);
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
    const cwas = await fetchCwas();
    if (unmountedRef.current) return;
    (map.getSource(CWA_SOURCE) as maplibregl.GeoJSONSource | undefined)?.setData(cwaGeoJson(cwas));
  } catch (err) {
    console.warn("couldn't load Center Weather Advisories for the map", err);
  }

  try {
    windsBulletinRef.current = await fetchWindsAloft("low", "06", "all");
  } catch (err) {
    console.warn("couldn't load winds aloft for the map", err);
  }
}

/** Imperative handle for reading the map's current view state on demand
 * (App.tsx's Share button) — a ref rather than lifting this into
 * continuously-synced React state, since nothing needs to know the
 * camera position except at the moment of building a share link. */
export interface MapViewHandle {
  getViewState: () => { chartKind: string | null; center: { lat: number; lon: number }; zoom: number };
}

export const MapView = forwardRef<
  MapViewHandle,
  {
    selectedAirport: Airport | null;
    onSelectAirport: (airport: Airport) => void;
    /** Everything about a tap besides airport selection (which is always
     * unconditional — see the map's "click" handler) — feeds the tab bar
     * below the map (Waypoint/Airspace/PIREPs/AIRMET/SIGMET/CWA). */
    onMapTap: (result: MapTapResult) => void;
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
    /** A shared link's map state (App.tsx's share.ts), applied once at
     * mount: the initial camera position/zoom (passed straight to the
     * maplibregl.Map constructor) and the initial base chart selection
     * (used the one time the chart catalog first loads, replacing the
     * usual default-to-Sectional). `undefined` — no shared link — keeps
     * every existing default; `chartKind: null` means "no chart" was
     * itself the shared selection, distinct from "unspecified". */
    initialView?: { chartKind: string | null; center: { lat: number; lon: number }; zoom: number };
  }
>(function MapView(
  { selectedAirport, onSelectAirport, onMapTap, selectedProcedureId, visible, route, preferredAltitudeFt, initialView },
  handleRef,
) {
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
  // Raw catalog entries grouped by kind, from the fetchCharts() response —
  // *not* yet added as map sources/layers. Materialized lazily (see
  // materializeChartKind) so switching kinds is the only thing that ever
  // triggers loading a given series' PMTiles files.
  const chartsByKindRef = useRef<Map<string, ChartCatalogEntry[]>>(new Map());
  const materializedChartKindsRef = useRef<Set<string>>(new Set());
  const [chartKinds, setChartKinds] = useState<string[]>([]);
  // The last-persisted view (camera/base chart/overlay toggles — see
  // persistence.ts's PersistedMapView), read once per mount. A shared
  // link's initialView still wins over it: someone opening a link sent
  // to them should see the sender's view, not their own last session.
  const [persistedView] = useState(loadMapView);
  // Set when the mount camera came from somewhere meaningful (a shared
  // link or the persisted view) — makes the route-fit effect below skip
  // its first run so that camera isn't immediately overridden. See that
  // effect's comment.
  const skipNextRouteFitRef = useRef(initialView !== undefined || persistedView.center !== undefined);
  const [visibleChartKinds, setVisibleChartKinds] = useState<Set<string>>(() => {
    const kind = initialView !== undefined ? initialView.chartKind : (persistedView.chartKind ?? "Sectional");
    return kind === null ? new Set() : new Set([kind]);
  });
  // Independent on/off toggles (unlike the chart-kind group above, these
  // aren't mutually exclusive — airspace, weather hazards, airports,
  // PIREPs, and CWA are separate concerns a pilot might want any
  // combination of). Default on (the map's original behavior), restored
  // from the persisted view when one exists.
  const [visibleAirspace, setVisibleAirspace] = useState(persistedView.overlays?.airspace ?? true);
  const [visibleWeatherHazards, setVisibleWeatherHazards] = useState(persistedView.overlays?.weatherHazards ?? true);
  const [visibleAirports, setVisibleAirports] = useState(persistedView.overlays?.airports ?? true);
  const [visiblePireps, setVisiblePireps] = useState(persistedView.overlays?.pireps ?? true);
  const [visibleCwas, setVisibleCwas] = useState(persistedView.overlays?.cwas ?? true);
  // Radar (NWS base reflectivity) defaults *off*, unlike the overlays
  // above — it's a heavier always-refreshing raster the user opts into for
  // a weather check, not baseline situational awareness, and leaving it off
  // by default keeps a fresh load from hitting NOAA for tiles nobody asked
  // to see.
  const [visibleRadar, setVisibleRadar] = useState(persistedView.overlays?.radar ?? false);
  // Persist base chart + overlay toggles whenever they change; the
  // camera is saved separately on moveend (see the mount effect). Makes
  // any reload — including the iOS Safari memory-kill this was added
  // for — land back on the same view instead of the defaults.
  useEffect(() => {
    saveMapView({
      chartKind: [...visibleChartKinds][0] ?? null,
      overlays: {
        airspace: visibleAirspace,
        weatherHazards: visibleWeatherHazards,
        airports: visibleAirports,
        pireps: visiblePireps,
        cwas: visibleCwas,
        radar: visibleRadar,
      },
    });
  }, [visibleChartKinds, visibleAirspace, visibleWeatherHazards, visibleAirports, visiblePireps, visibleCwas, visibleRadar]);

  // Adds map sources/layers for one chart kind's catalog entries, if it
  // hasn't happened already — a no-op on repeat calls (e.g. re-selecting
  // a kind that was already shown once and then hidden). Always starts
  // hidden; the caller is responsible for the actual visibility flip
  // right after, so there's one place (not two) that decides what ends
  // up shown.
  function materializeChartKind(map: maplibregl.Map, kind: string) {
    if (materializedChartKindsRef.current.has(kind)) return;
    materializedChartKindsRef.current.add(kind);
    const layerIds: string[] = [];
    for (const chart of chartsByKindRef.current.get(kind) ?? []) {
      const sourceId = `chart-${chart.id}`;
      map.addSource(sourceId, {
        type: "raster",
        url: `pmtiles://${API_BASE_URL}${chart.tile_url}`,
        tileSize: 256,
      });
      // Anchored below the radar overlay, which itself sits just below the
      // airspace/hazard stack (see the radar layer in the load handler), so
      // chart imagery stays at the very bottom of every overlay: an opaque
      // sectional must never paint over precip or airspace. Falls back to
      // "airspace-fill" if the radar layer somehow isn't present yet.
      map.addLayer(
        { id: sourceId, type: "raster", source: sourceId, layout: { visibility: "none" } },
        map.getLayer(RADAR_LAYER) ? RADAR_LAYER : "airspace-fill",
      );
      layerIds.push(sourceId);
    }
    chartLayerIdsByKindRef.current.set(kind, layerIds);
  }

  // The inverse: fully removes a kind's layers *and sources*, releasing
  // their tile caches/textures. Switching kinds used to just flip
  // visibility to "none", but a hidden source keeps its cached tiles
  // alive — after browsing a few kinds that stacked several hundred MB
  // of dead raster textures, a real contributor to the iOS Safari
  // memory-kill described at the maxTileCacheSize option above. Cheap to
  // undo: re-selecting the kind re-adds sources and re-fetches a couple
  // of small PMTiles headers.
  function dematerializeChartKind(map: maplibregl.Map, kind: string) {
    if (!materializedChartKindsRef.current.has(kind)) return;
    materializedChartKindsRef.current.delete(kind);
    for (const layerId of chartLayerIdsByKindRef.current.get(kind) ?? []) {
      if (map.getLayer(layerId)) map.removeLayer(layerId);
      if (map.getSource(layerId)) map.removeSource(layerId);
    }
    chartLayerIdsByKindRef.current.delete(kind);
  }

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
      // Camera precedence: a shared link's view, else wherever this
      // browser last left the map (persisted on moveend below), else the
      // KLGA default.
      center: initialView
        ? [initialView.center.lon, initialView.center.lat]
        : persistedView.center
          ? [persistedView.center.lon, persistedView.center.lat]
          : [-73.874, 40.7769], // KLGA
      zoom: initialView?.zoom ?? persistedView.zoom ?? 6,
      // MapLibre's default tile cache is dynamically sized per source —
      // and one chart kind here is up to 57 raster sources at once (a
      // source per sectional), each caching decoded tile textures as the
      // view pans across it. On iOS Safari that blew past the per-tab
      // memory ceiling within a minute of panning (reported on iPad:
      // page crashes and force-reloads), because iOS kills the tab
      // instead of evicting GPU memory. A small fixed cap per source
      // trades some tile re-fetching on pan-back (cheap: HTTP range
      // requests, CDN-cached) for bounded memory.
      maxTileCacheSize: 16,
      // See MAX_IOS_PIXEL_RATIO — undefined everywhere else keeps
      // MapLibre's default (the device's own devicePixelRatio).
      pixelRatio: IS_IOS ? Math.min(window.devicePixelRatio || 1, MAX_IOS_PIXEL_RATIO) : undefined,
    });
    mapRef.current = map;
    map.addControl(new maplibregl.NavigationControl(), "top-right");

    // MapLibre only auto-tracks *window* resizes, not container ones —
    // so when the map pane is drag-resized or the narrow/wide layout
    // flips (App.tsx), nudge it to repaint at the new container size.
    const resizeObserver = new ResizeObserver(() => map.resize());
    resizeObserver.observe(containerRef.current);

    // METAR flight categories are refreshed from ff-api's bulk cache on
    // an interval (backend re-pulls upstream every few minutes; this just
    // re-reads the small map), not per pan — see loadFlightCategories.
    let flightCategoryTimer: ReturnType<typeof setInterval> | undefined;

    map.on("load", () => {
      map.addSource(AIRPORTS_SOURCE, { type: "geojson", data: EMPTY_COLLECTION });
      map.addLayer({
        id: "airports-circle",
        type: "circle",
        source: AIRPORTS_SOURCE,
        filter: AIRPORT_DISPLAY_FILTER,
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
        filter: AIRPORT_DISPLAY_FILTER,
        layout: {
          "text-field": ["get", "icao"],
          "text-size": 11,
          "text-offset": [0, 1.1],
          "text-anchor": "top",
        },
        paint: { "text-color": "#0b1220", "text-halo-color": "#c8d6e5", "text-halo-width": 1 },
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

      // Chart imagery renders as the base layer, under the radar overlay
      // and the airspace/weather/airport overlays added below -- inserted
      // before the radar layer (which is itself pinned just below
      // "airspace-fill"), not just "airports-circle": this
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
      // via HTTP range requests). Grouped by `kind` (Sectional/
      // IfrEnrouteLow/IfrEnrouteHigh/...) so the toggle control below can
      // show/hide a whole chart series at once.
      //
      // Sources/layers are materialized lazily, one kind at a time (see
      // materializeChartKind below), not for the whole catalog up front.
      // Adding a PMTiles source fetches that file's header immediately
      // regardless of layer visibility, so eagerly adding all ~108
      // charts (57 sectionals + IFR Low/High) fired ~108 requests on
      // every page load, most for chart kinds nobody had asked to see
      // yet — measured 6-10s of that competing with the actually-visible
      // charts for the browser's limited concurrent-connection pool.
      // Since chart kinds are mutually exclusive now, there's never a
      // reason to have more than one kind's sources loaded at a time.
      void fetchCharts()
        .then((charts) => {
          const byKind = new Map<string, ChartCatalogEntry[]>();
          for (const chart of charts) {
            const list = byKind.get(chart.kind) ?? [];
            list.push(chart);
            byKind.set(chart.kind, list);
          }
          chartsByKindRef.current = byKind;
          setChartKinds([...byKind.keys()].sort());
          // visibleChartKinds here is frozen at its initial-render value
          // (this effect has a [] dep array, mount-only) — either the
          // usual default Set(["Sectional"]) or whatever a shared link's
          // initialView asked for (see the useState above), which is
          // exactly what should materialize (and show) on first load
          // regardless of how long the fetch took.
          for (const kind of visibleChartKinds) {
            materializeChartKind(map, kind);
            for (const layerId of chartLayerIdsByKindRef.current.get(kind) ?? []) {
              map.setLayoutProperty(layerId, "visibility", "visible");
            }
          }
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
      map.addLayer(
        {
          id: "airspace-label-border",
          type: "symbol",
          source: AIRSPACE_SOURCE,
          layout: {
            "symbol-placement": "line",
            "symbol-spacing": 800,
            "text-field": ["get", "label"],
            "text-size": 12.5,
            "text-justify": "center",
            "text-anchor": "center",
            "text-line-height": 1.0,
            "text-keep-upright": true,
            "text-allow-overlap": true,
            "text-ignore-placement": true,
            "text-rotation-alignment": "map",
          },
          paint: {
            "text-color": airspaceColorExpr,
            "text-halo-color": airspaceColorExpr,
            "text-halo-width": 7.5,
          },
        },
        "airports-circle",
      );
      map.addLayer(
        {
          id: "airspace-label",
          type: "symbol",
          source: AIRSPACE_SOURCE,
          layout: {
            "symbol-placement": "line",
            "symbol-spacing": 800,
            "text-field": ["get", "label"],
            "text-size": 12.5,
            "text-justify": "center",
            "text-anchor": "center",
            "text-line-height": 1.0,
            "text-keep-upright": true,
            "text-allow-overlap": true,
            "text-ignore-placement": true,
            "text-rotation-alignment": "map",
          },
          paint: {
            "text-color": airspaceColorExpr,
            "text-halo-color": "#ffffff",
            "text-halo-width": 4.5,
          },
        },
        "airports-circle",
      );

      // NWS base-reflectivity radar (see RADAR_WMS_URL) as a translucent
      // raster overlay. Anchored before "airspace-fill" so it sits directly
      // beneath the airspace/hazard vector overlays (which keep them
      // readable on top) but above chart imagery — the chart loop anchors
      // its raster layers before *this* one (see materializeChartKind), so
      // an opaque sectional never hides the precip. Created hidden: the
      // overlay defaults off and a hidden WMS source issues no tile
      // requests, so a page load that doesn't want radar never touches
      // NOAA; the reconciliation effect flips it on when the persisted
      // state asks for it, and the toggle button drives it thereafter.
      map.addSource(RADAR_SOURCE, { type: "raster", tiles: [RADAR_WMS_URL], tileSize: 256 });
      map.addLayer(
        {
          id: RADAR_LAYER,
          type: "raster",
          source: RADAR_SOURCE,
          layout: { visibility: "none" },
          paint: { "raster-opacity": 0.6 },
        },
        "airspace-fill",
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

      // CWAs get their own flat color (distinct from SIGMET's red and
      // G-AIRMET's per-hazard palette) rather than a hazard-keyed match
      // expression — real data has only ever shown "TS" so far, and a
      // dedicated color keeps it visually distinct as its own category
      // regardless of what hazard codes show up later.
      map.addSource(CWA_SOURCE, { type: "geojson", data: EMPTY_COLLECTION });
      map.addLayer(
        {
          id: "cwa-fill",
          type: "fill",
          source: CWA_SOURCE,
          paint: { "fill-color": "#ff8c42", "fill-opacity": 0.15 },
        },
        "airports-circle",
      );
      map.addLayer(
        {
          id: "cwa-line",
          type: "line",
          source: CWA_SOURCE,
          paint: { "line-color": "#ff8c42", "line-width": 1.5, "line-dasharray": [1, 1] },
        },
        "airports-circle",
      );

      // PIREPs render as point markers, colored by the worst reported
      // icing/turbulence severity (see pirepSeverity) — inserted before
      // "airports-circle" same as everything else here, so airport
      // markers/labels still stay on top.
      map.addSource(PIREP_SOURCE, { type: "geojson", data: EMPTY_COLLECTION });
      map.addLayer(
        {
          id: "pirep-circle",
          type: "circle",
          source: PIREP_SOURCE,
          paint: {
            "circle-radius": ["case", ["get", "urgent"], 7, 4],
            "circle-color": [
              "match",
              ["get", "severity"],
              "SEVERE",
              PIREP_SEVERITY_COLORS.SEVERE,
              "MODERATE",
              PIREP_SEVERITY_COLORS.MODERATE,
              "LIGHT",
              PIREP_SEVERITY_COLORS.LIGHT,
              PIREP_SEVERITY_COLORS.NONE,
            ],
            "circle-stroke-color": "#0b1220",
            "circle-stroke-width": 1.2,
          },
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

      // Tap highlights, added last so they sit on top of every overlay:
      // a hollow ring around the selected airport (driven by the
      // selectedAirport effect below), plus point markers for the two
      // waypoints a tap proposes — the nearest fix/navaid and the tapped
      // "user" point (both set from the click handler below).
      map.addSource(SELECTED_AIRPORT_SOURCE, { type: "geojson", data: EMPTY_COLLECTION });
      map.addLayer({
        id: "selected-airport-ring",
        type: "circle",
        source: SELECTED_AIRPORT_SOURCE,
        paint: {
          "circle-radius": 11,
          "circle-color": "rgba(0,0,0,0)",
          "circle-stroke-color": "#ffffff",
          "circle-stroke-width": 3,
        },
      });
      map.addSource(TAP_POINTS_SOURCE, { type: "geojson", data: EMPTY_COLLECTION });
      map.addLayer({
        id: "tap-points-circle",
        type: "circle",
        source: TAP_POINTS_SOURCE,
        paint: {
          "circle-radius": 6,
          "circle-color": ["match", ["get", "role"], "waypoint", "#ff9f43", "user", "#7ee787", "#ffffff"],
          "circle-stroke-color": "#0b1220",
          "circle-stroke-width": 1.5,
        },
      });
      map.addLayer({
        id: "tap-points-label",
        type: "symbol",
        source: TAP_POINTS_SOURCE,
        layout: {
          "text-field": ["get", "label"],
          "text-size": 11,
          "text-offset": [0, 1.1],
          "text-anchor": "top",
        },
        paint: { "text-color": "#e6edf3", "text-halo-color": "#0b1220", "text-halo-width": 1.2 },
      });

      // Single unified tap handler, replacing the old per-layer Popups
      // (airspace/G-AIRMET/SIGMET/CWA/PIREP each used to open their own
      // MapLibre Popup on click). Every tap now does two things at
      // once: selects whichever airport in the current view is closest
      // to the tap (regardless of what's actually under the point —
      // there's no dedicated "airports-circle" click binding anymore),
      // and gathers everything else at the tapped point for the tab bar
      // below the map to display (Airspace/PIREPs/AIRMET/SIGMET/CWA
      // tabs) plus the nearest waypoint/navaid (Waypoint tab, needs its
      // own network round trip since there's no client-side navaid/
      // waypoint layer to query locally the way the others are).
      map.on("click", (e) => {
        const airports = visibleAirportsRef.current;
        if (airports.length > 0) {
          let nearest = airports[0];
          let bestDistSq = (nearest.lat - e.lngLat.lat) ** 2 + (nearest.lon - e.lngLat.lng) ** 2;
          for (const a of airports) {
            const distSq = (a.lat - e.lngLat.lat) ** 2 + (a.lon - e.lngLat.lng) ** 2;
            if (distSq < bestDistSq) {
              bestDistSq = distSq;
              nearest = a;
            }
          }
          onSelectAirport(nearest);
        }

        const dedupeById = (features: maplibregl.MapGeoJSONFeature[]): Record<string, unknown>[] => {
          const seenIds = new Set<string>();
          const out: Record<string, unknown>[] = [];
          for (const f of features) {
            const id = f.properties?.id as string | undefined;
            if (id) {
              if (seenIds.has(id)) continue;
              seenIds.add(id);
            }
            out.push(f.properties ?? {});
          }
          return out;
        };
        // A small padded box around the tap point, not the exact pixel,
        // for the small/thin targets (PIREP's point circles, G-AIRMET's
        // freezing-level lines) — a real tap easily lands a few pixels
        // off a 4px-radius circle or a 1px line, and the fill layers
        // below (airspace/sigmet/cwa) are already large enough not to
        // need this forgiveness.
        const nearPoint: [maplibregl.PointLike, maplibregl.PointLike] = [
          [e.point.x - 6, e.point.y - 6],
          [e.point.x + 6, e.point.y + 6],
        ];
        const base = {
          lngLat: { lng: e.lngLat.lng, lat: e.lngLat.lat },
          airspace: dedupeById(map.queryRenderedFeatures(e.point, { layers: ["airspace-fill"] })),
          gairmets: dedupeById(map.queryRenderedFeatures(nearPoint, { layers: ["gairmet-fill", "gairmet-line"] })),
          sigmets: dedupeById(map.queryRenderedFeatures(e.point, { layers: ["sigmet-fill"] })),
          cwas: dedupeById(map.queryRenderedFeatures(e.point, { layers: ["cwa-fill"] })),
          pireps: dedupeById(map.queryRenderedFeatures(nearPoint, { layers: ["pirep-circle"] })),
        };
        const setTapPoints = (nearestFix: NearestFix | null) =>
          (map.getSource(TAP_POINTS_SOURCE) as maplibregl.GeoJSONSource | undefined)?.setData(
            tapPointsGeoJson(e.lngLat, nearestFix),
          );
        fetchNearestFix(e.lngLat.lat, e.lngLat.lng)
          .then((nearestFix) => {
            if (unmountedRef.current) return;
            onMapTap({ ...base, nearestFix });
            setTapPoints(nearestFix);
          })
          .catch(() => {
            if (unmountedRef.current) return;
            onMapTap({ ...base, nearestFix: null });
            setTapPoints(null);
          });
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
      // Load the flight-category map now and refresh it periodically. The
      // backend re-pulls upstream on its own schedule; 5 min here keeps
      // the map's colors reasonably fresh without polling ff-api hard.
      void loadFlightCategories(map, visibleAirportsRef, flightCategoriesRef, unmountedRef);
      flightCategoryTimer = setInterval(() => {
        void loadFlightCategories(map, visibleAirportsRef, flightCategoriesRef, unmountedRef);
      }, 5 * 60 * 1000);
      void refreshVisibleAirspace(map, unmountedRef);
      void refreshVisiblePireps(map, unmountedRef);
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
        void refreshVisiblePireps(map, unmountedRef);
        // Persist the camera so any reload (crash, refresh, tab
        // eviction) restores this view — one tiny localStorage write per
        // completed pan/zoom, merged with the chart/toggle slice saved
        // by its own effect (see saveMapView).
        const center = map.getCenter();
        saveMapView({ center: { lat: center.lat, lon: center.lng }, zoom: map.getZoom() });
      });

      setLoaded(true);
    });

    return () => {
      unmountedRef.current = true;
      resizeObserver.disconnect();
      if (flightCategoryTimer) clearInterval(flightCategoryTimer);
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

  // Applies the persisted overlay-toggle state to the actual layers once
  // they exist — every overlay layer is created visible in the "load"
  // handler (the pre-persistence default), and the toggle handlers below
  // only flip visibility on click, so a restored "off" state needs this
  // one-time reconciliation. Runs on `loaded` only; later changes go
  // through the toggle handlers as before.
  useEffect(() => {
    if (!loaded) return;
    if (!visibleAirspace) setLayersVisibility(["airspace-fill", "airspace-line", "airspace-label", "airspace-label-border"], false);
    if (!visibleWeatherHazards)
      setLayersVisibility(["gairmet-fill", "gairmet-line", "sigmet-fill", "sigmet-line"], false);
    if (!visibleAirports) setLayersVisibility(["airports-circle", "airports-label"], false);
    if (!visiblePireps) setLayersVisibility(["pirep-circle"], false);
    if (!visibleCwas) setLayersVisibility(["cwa-fill", "cwa-line"], false);
    // Radar is the inverse of the overlays above: its layer is created
    // hidden (default off), so this shows it only when the persisted state
    // had it on, rather than hiding an otherwise-visible layer.
    if (visibleRadar) setLayersVisibility([RADAR_LAYER], true);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [loaded]);

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
    // A restored camera (persisted view or shared link) must win over
    // the route fit on the first eligible run: a persisted flight plan
    // reloads alongside the persisted camera, and without this the fit
    // immediately flew the map to the route bounds, stomping the exact
    // view the persistence had just restored — reported on iPad after a
    // Safari memory-kill reload ("returned me to my flight plan, but
    // not the region of the map I was viewing"). Later route *changes*
    // fit as before; the skip is consumed exactly once.
    if (skipNextRouteFitRef.current) {
      skipNextRouteFitRef.current = false;
      return;
    }
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
    (map.getSource(SELECTED_AIRPORT_SOURCE) as maplibregl.GeoJSONSource | undefined)?.setData(
      selectedAirportGeoJson(selectedAirport),
    );

    let cancelled = false;
    if (selectedAirport) {
      fetchAirportDetail(selectedAirport.icao)
        .then((detail) => {
          if (!cancelled) setRunways(detail.runways);
        })
        .catch((err: unknown) => console.warn("couldn't load runways for the map", err));
      map.flyTo({ center: [selectedAirport.lon, selectedAirport.lat], zoom: 9, duration: 800 });
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

  // At most one chart series at a time — Sectional/TAC/Flyway/IFR/Heli are
  // meant to replace each other, not stack (they're the same charts at
  // different scales/scopes, not independent overlays like airspace/
  // weather/airports below), so they're a single dropdown rather than the
  // independent on/off buttons further down. `kind` is the chosen series,
  // or "" for no base chart.
  const selectChartKind = (kind: string) => {
    const map = mapRef.current;
    if (!map) return;
    setVisibleChartKinds(() => {
      const next: Set<string> = kind ? new Set([kind]) : new Set();
      // Deselected kinds are removed outright (not just hidden) so their
      // tile caches/GPU textures are actually released — see
      // dematerializeChartKind's comment for the iOS memory story.
      for (const k of [...materializedChartKindsRef.current]) {
        if (!next.has(k)) dematerializeChartKind(map, k);
      }
      // First time a kind is selected, its sources/layers haven't been
      // added yet (see materializeChartKind) — do that now so the
      // visibility loop below has layer ids to flip.
      if (kind) materializeChartKind(map, kind);
      for (const [k, layerIds] of chartLayerIdsByKindRef.current) {
        const nowVisible = next.has(k);
        for (const layerId of layerIds) {
          map.setLayoutProperty(layerId, "visibility", nowVisible ? "visible" : "none");
        }
      }
      return next;
    });
  };

  // Exposes the map's current view (App.tsx's Share button reads this on
  // click, not continuously — see MapViewHandle) — refreshed whenever
  // visibleChartKinds changes so the handle always reports the live
  // selection rather than whatever it was at mount.
  useImperativeHandle(
    handleRef,
    () => ({
      getViewState: () => {
        const map = mapRef.current;
        const center = map?.getCenter();
        return {
          chartKind: [...visibleChartKinds][0] ?? null,
          center: center ? { lat: center.lat, lon: center.lng } : { lat: 40.7769, lon: -73.874 },
          zoom: map?.getZoom() ?? 6,
        };
      },
    }),
    [visibleChartKinds],
  );

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

  const setLayersVisibility = (layerIds: string[], visible: boolean) => {
    const map = mapRef.current;
    if (!map) return;
    for (const layerId of layerIds) {
      map.setLayoutProperty(layerId, "visibility", visible ? "visible" : "none");
    }
  };
  const toggleAirspaceVisibility = () =>
    setVisibleAirspace((prev) => {
      setLayersVisibility(["airspace-fill", "airspace-line", "airspace-label", "airspace-label-border"], !prev);
      return !prev;
    });
  const toggleWeatherHazardsVisibility = () =>
    setVisibleWeatherHazards((prev) => {
      setLayersVisibility(["gairmet-fill", "gairmet-line", "sigmet-fill", "sigmet-line"], !prev);
      return !prev;
    });
  const toggleAirportsVisibility = () =>
    setVisibleAirports((prev) => {
      setLayersVisibility(["airports-circle", "airports-label"], !prev);
      return !prev;
    });
  const togglePirepsVisibility = () =>
    setVisiblePireps((prev) => {
      setLayersVisibility(["pirep-circle"], !prev);
      return !prev;
    });
  const toggleCwaVisibility = () =>
    setVisibleCwas((prev) => {
      setLayersVisibility(["cwa-fill", "cwa-line"], !prev);
      return !prev;
    });
  const toggleRadarVisibility = () =>
    setVisibleRadar((prev) => {
      setLayersVisibility([RADAR_LAYER], !prev);
      return !prev;
    });

  return (
    <div className="map-view">
      <div ref={containerRef} className="map-view-canvas" />
      <div className="chart-kind-toggle">
        {/* One base chart series at a time — a dropdown rather than a row
            of mutually-exclusive buttons, since there are now several
            (Sectional/TAC/Flyway/IFR Low/IFR High/Heli). Ordered by
            CHART_KIND_ORDER so the list reads sensibly regardless of the
            catalog's own ordering. */}
        <select
          className="chart-kind-select"
          value={[...visibleChartKinds][0] ?? ""}
          onChange={(e) => selectChartKind(e.target.value)}
          aria-label="Base chart"
        >
          <option value="">No chart</option>
          {[...chartKinds]
            .sort((a, b) => (CHART_KIND_ORDER[a] ?? 99) - (CHART_KIND_ORDER[b] ?? 99))
            .map((kind) => (
              <option key={kind} value={kind}>
                {CHART_KIND_LABELS[kind] ?? kind}
              </option>
            ))}
        </select>
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
        {/* Unlike the chart-kind group above, these are independent
            on/off switches, not mutually exclusive — any combination of
            airspace/weather hazards/airports/PIREPs/CWA can be showing
            at once. */}
        <div className="chart-kind-toggle-divider" />
        <button className={visibleAirspace ? "selected" : ""} onClick={toggleAirspaceVisibility}>
          Airspace
        </button>
        <button className={visibleWeatherHazards ? "selected" : ""} onClick={toggleWeatherHazardsVisibility}>
          AIRMET/SIGMET
        </button>
        <button className={visibleAirports ? "selected" : ""} onClick={toggleAirportsVisibility}>
          Airports
        </button>
        <button className={visiblePireps ? "selected" : ""} onClick={togglePirepsVisibility}>
          PIREPs
        </button>
        <button className={visibleCwas ? "selected" : ""} onClick={toggleCwaVisibility}>
          CWA
        </button>
        <button className={visibleRadar ? "selected" : ""} onClick={toggleRadarVisibility}>
          Radar
        </button>
      </div>
    </div>
  );
});
