import { useEffect, useRef, useState } from "react";
import maplibregl, { type Map as MlMap, type StyleSpecification } from "maplibre-gl";
import "maplibre-gl/dist/maplibre-gl.css";
import { Protocol as PmtilesProtocol } from "pmtiles";
import { API_BASE_URL } from "./api";
import { fetchAirportDetail, fetchAirportsInBbox, fetchAirspaceInBbox, fetchCharts, fetchProcedureDetail } from "./data";
import type { Airport, AirspaceVolume, GAirmet, ProcedureDetail, Runway, Sigmet, WindsAloftBulletin } from "./types";
import { fetchGairmets, fetchMetars, fetchSigmets, fetchWindsAloft } from "./weather";

const AIRPORTS_SOURCE = "airports";
const RUNWAYS_SOURCE = "runways";
const PROCEDURE_SOURCE = "procedure-path";
const GAIRMET_SOURCE = "gairmets";
const SIGMET_SOURCE = "sigmets";
const WINDS_ALOFT_SOURCE = "winds-aloft";
const AIRSPACE_SOURCE = "airspace";

// Bay Area demo scope only has stations at this one altitude reliably —
// see MapView's winds-aloft fetch for why this isn't user-selectable yet.
const WINDS_ALOFT_ALTITUDE_FT = 9000;

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

// No third-party basemap tiles: DESIGN.md's map is built on our own
// charts.pmtiles (raster sectionals/TACs) as the base layer instead —
// see the chart_catalog handling below, which renders one when the
// loaded cycle bundle has one. A plain background keeps the map usable
// when it doesn't and matches the app's dark theme.
const BLANK_STYLE: StyleSpecification = {
  version: 8,
  sources: {},
  layers: [{ id: "background", type: "background", paint: { "background-color": "#0b1220" } }],
};

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

/** Winds-aloft station idents are 3-letter FAA identifiers (e.g. "SFO"),
 * not ICAO codes — strip the CONUS "K" prefix to match. Only stations
 * the bulletin actually covers get a feature (confirmed live: of this
 * demo's 5 airports, only KSFO/SFO is a winds-aloft reporting point —
 * small GA fields like KPAO generally aren't, and that's correct
 * behavior to show, not a bug to work around). */
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
function procedureGeoJson(detail: ProcedureDetail): GeoJSON.FeatureCollection {
  const features: GeoJSON.Feature[] = [];
  for (const t of detail.transitions) {
    const coords = t.legs
      .map((leg) => (leg.fix_ident ? detail.fixes[leg.fix_ident] : undefined))
      .filter((fix): fix is { lat: number; lon: number } => fix !== undefined)
      .map((fix) => [fix.lon, fix.lat]);
    if (coords.length >= 2) {
      features.push({
        type: "Feature",
        geometry: { type: "LineString", coordinates: coords },
        properties: { transitionId: t.id },
      });
    }
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
 * flight category for up to MAX_METAR_AIRPORTS of them. Keeps the
 * fetched list in `visibleAirportsRef` so the click handler can hand a
 * full Airport object to the app. */
async function refreshVisibleAirports(
  map: MlMap,
  visibleAirportsRef: { current: Airport[] },
  windsBulletinRef: { current: WindsAloftBulletin | null },
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
  visibleAirportsRef.current = airports;
  (map.getSource(AIRPORTS_SOURCE) as maplibregl.GeoJSONSource | undefined)?.setData(
    airportsGeoJson(airports, new Map()),
  );
  if (windsBulletinRef.current) {
    (map.getSource(WINDS_ALOFT_SOURCE) as maplibregl.GeoJSONSource | undefined)?.setData(
      windsAloftGeoJson(windsBulletinRef.current, airports, WINDS_ALOFT_ALTITUDE_FT),
    );
  }

  try {
    const metars = await fetchMetars(airports.slice(0, MAX_METAR_AIRPORTS).map((a) => a.icao));
    const flightCategories = new Map<string, string>();
    for (const m of metars) {
      if (m.fltCat) flightCategories.set(m.icaoId, m.fltCat);
    }
    // The view may have moved on while the METARs were in flight — only
    // apply if these airports are still the current set.
    if (visibleAirportsRef.current === airports) {
      (map.getSource(AIRPORTS_SOURCE) as maplibregl.GeoJSONSource | undefined)?.setData(
        airportsGeoJson(airports, flightCategories),
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
async function refreshVisibleAirspace(map: MlMap) {
  const bounds = map.getBounds();
  const bbox = `${bounds.getWest()},${bounds.getSouth()},${bounds.getEast()},${bounds.getNorth()}`;
  try {
    const volumes = await fetchAirspaceInBbox(bbox);
    (map.getSource(AIRSPACE_SOURCE) as maplibregl.GeoJSONSource | undefined)?.setData(airspaceGeoJson(volumes));
  } catch (err) {
    console.warn("couldn't load airspace boundaries for the map view", err);
  }
}

/** Fetches the CONUS-wide hazard overlays and the winds-aloft bulletin
 * (cached in `windsBulletinRef` for reuse as the view moves). Each is
 * independent, so one failing doesn't block the others. */
async function loadWeatherOverlays(map: MlMap, windsBulletinRef: { current: WindsAloftBulletin | null }) {
  try {
    const gairmets = await fetchGairmets();
    (map.getSource(GAIRMET_SOURCE) as maplibregl.GeoJSONSource | undefined)?.setData(gairmetGeoJson(gairmets));
  } catch (err) {
    console.warn("couldn't load G-AIRMETs for the map", err);
  }

  try {
    const sigmets = await fetchSigmets();
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
}: {
  selectedAirport: Airport | null;
  onSelectAirport: (airport: Airport) => void;
  selectedProcedureId: string | null;
}) {
  const containerRef = useRef<HTMLDivElement>(null);
  const mapRef = useRef<MlMap | null>(null);
  const visibleAirportsRef = useRef<Airport[]>([]);
  const windsBulletinRef = useRef<WindsAloftBulletin | null>(null);
  const [loaded, setLoaded] = useState(false);

  useEffect(() => {
    if (!containerRef.current) return;
    const map = new maplibregl.Map({
      container: containerRef.current,
      style: BLANK_STYLE,
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

      map.addSource(PROCEDURE_SOURCE, { type: "geojson", data: EMPTY_COLLECTION });
      map.addLayer({
        id: "procedure-line",
        type: "line",
        source: PROCEDURE_SOURCE,
        paint: { "line-color": "#e254e0", "line-width": 4, "line-dasharray": [2, 1.5] },
      });

      // Chart imagery renders as the base layer, under the airport/runway/
      // procedure overlays above -- inserted before "airports-circle"
      // (already added) rather than appended, so it doesn't cover them.
      // The catalog comes from ff-api (tile_url is an ff-api path like
      // /bundles/<cycle>/chart.pmtiles, fetched by the pmtiles protocol
      // via HTTP range requests); fetched async, layers added on arrival.
      void fetchCharts()
        .then((charts) => {
          for (const chart of charts) {
            const sourceId = `chart-${chart.id}`;
            map.addSource(sourceId, {
              type: "raster",
              url: `pmtiles://${API_BASE_URL}${chart.tile_url}`,
              tileSize: 256,
            });
            map.addLayer({ id: sourceId, type: "raster", source: sourceId }, "airports-circle");
          }
        })
        .catch((err: unknown) => console.warn("couldn't load the chart catalog for the map", err));

      // Airspace boundaries render above chart imagery but below weather
      // hazards/airports, same insertion point (before "airports-circle")
      // as the chart loop above and the G-AIRMET/SIGMET sources below.
      // Filled lightly so overlapping shelves (a busy Class B/C stacks
      // several) are still readable rather than opaque.
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
          "text-offset": [1.6, -1.2],
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
          "text-offset": [1.6, -2.3],
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
      void loadWeatherOverlays(map, windsBulletinRef).then(() => refreshVisibleAirports(map, visibleAirportsRef, windsBulletinRef));
      void refreshVisibleAirspace(map);
      map.on("moveend", () => {
        void refreshVisibleAirports(map, visibleAirportsRef, windsBulletinRef);
        void refreshVisibleAirspace(map);
      });

      setLoaded(true);
    });

    return () => {
      map.remove();
      mapRef.current = null;
      setLoaded(false);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

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

    let cancelled = false;
    if (selectedProcedureId) {
      fetchProcedureDetail(selectedProcedureId)
        .then((detail) => {
          if (!cancelled) setPath(procedureGeoJson(detail));
        })
        .catch((err: unknown) => console.warn("couldn't load the procedure path for the map", err));
    } else {
      setPath(EMPTY_COLLECTION);
    }
    return () => {
      cancelled = true;
    };
  }, [selectedProcedureId, loaded]);

  return <div ref={containerRef} className="map-view" />;
}
