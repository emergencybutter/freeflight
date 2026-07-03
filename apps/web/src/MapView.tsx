import { useEffect, useMemo, useRef, useState } from "react";
import maplibregl, { type Map as MlMap, type StyleSpecification } from "maplibre-gl";
import "maplibre-gl/dist/maplibre-gl.css";
import { Protocol as PmtilesProtocol } from "pmtiles";
import type { Database } from "sql.js";
import { queryAll } from "./db";
import type {
  Airport,
  ChartCatalogEntry,
  Fix,
  GAirmet,
  ProcedureLeg,
  ProcedureTransition,
  Runway,
  Sigmet,
  WindsAloftBulletin,
} from "./types";
import { fetchGairmets, fetchMetars, fetchSigmets, fetchWindsAloft } from "./weather";

const AIRPORTS_SOURCE = "airports";
const RUNWAYS_SOURCE = "runways";
const PROCEDURE_SOURCE = "procedure-path";
const GAIRMET_SOURCE = "gairmets";
const SIGMET_SOURCE = "sigmets";
const WINDS_ALOFT_SOURCE = "winds-aloft";

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

// Registered once per page load (module scope, not per-component-mount):
// MapLibre's addProtocol is global, and re-registering on every mount
// (e.g. React StrictMode's double-invoke) is harmless but pointless.
maplibregl.addProtocol("pmtiles", new PmtilesProtocol().tile);

// No third-party basemap tiles: DESIGN.md's map is built on our own
// charts.pmtiles (raster sectionals/TACs) as the base layer instead —
// see the chart_catalog handling below, which renders one when the
// loaded cycle bundle has one. A plain background keeps the map usable
// when it doesn't (see TODO.md) and matches the app's dark theme.
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
 * approach vs. missed) aren't a continuous path, so they're never joined. */
function procedureGeoJson(
  transitions: ProcedureTransition[],
  legsByTransition: Map<string, ProcedureLeg[]>,
  fixes: Map<string, Fix>,
): GeoJSON.FeatureCollection {
  const features: GeoJSON.Feature[] = [];
  for (const t of transitions) {
    const coords = (legsByTransition.get(t.id) ?? [])
      .map((leg) => (leg.fix_ident ? fixes.get(leg.fix_ident) : undefined))
      .filter((fix): fix is Fix => fix !== undefined)
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

/** Fetches METAR/G-AIRMET/SIGMET/winds-aloft from ff-api and pushes each
 * into its map source independently, so one failing (most likely:
 * ff-api isn't running) doesn't block the others or the base map, which
 * is already usable from the static SQLite bundle regardless. */
async function loadWeatherOverlays(map: MlMap, airports: Airport[]) {
  try {
    const metars = await fetchMetars(airports.map((a) => a.icao));
    const flightCategories = new Map<string, string>();
    for (const m of metars) {
      if (m.fltCat) flightCategories.set(m.icaoId, m.fltCat);
    }
    (map.getSource(AIRPORTS_SOURCE) as maplibregl.GeoJSONSource | undefined)?.setData(
      airportsGeoJson(airports, flightCategories),
    );
  } catch (err) {
    console.warn("couldn't load METAR flight categories for the map", err);
  }

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
    const bulletin = await fetchWindsAloft("low", "06", "all");
    (map.getSource(WINDS_ALOFT_SOURCE) as maplibregl.GeoJSONSource | undefined)?.setData(
      windsAloftGeoJson(bulletin, airports, WINDS_ALOFT_ALTITUDE_FT),
    );
  } catch (err) {
    console.warn("couldn't load winds aloft for the map", err);
  }
}

export function MapView({
  db,
  selectedIcao,
  onSelectAirport,
  selectedProcedureId,
}: {
  db: Database;
  selectedIcao: string | null;
  onSelectAirport: (icao: string) => void;
  selectedProcedureId: string | null;
}) {
  const containerRef = useRef<HTMLDivElement>(null);
  const mapRef = useRef<MlMap | null>(null);
  const [loaded, setLoaded] = useState(false);

  const airports = useMemo(() => queryAll<Airport>(db, "SELECT * FROM airport"), [db]);
  const charts = useMemo(() => queryAll<ChartCatalogEntry>(db, "SELECT * FROM chart_catalog"), [db]);
  // Every waypoint/navaid ident -> coordinates, so a procedure leg's
  // fix_ident can be resolved without a per-leg query. Both tables share
  // the ident/lat/lon shape (see types.ts's `Fix`); idents aren't
  // globally unique across ICAO regions, so this takes whichever match
  // comes first, same as the CIFP data itself doesn't disambiguate here.
  const fixes = useMemo(() => {
    const rows = [
      ...queryAll<Fix>(db, "SELECT ident, lat, lon FROM waypoint"),
      ...queryAll<Fix>(db, "SELECT ident, lat, lon FROM navaid"),
    ];
    const map = new Map<string, Fix>();
    for (const row of rows) {
      if (!map.has(row.ident)) map.set(row.ident, row);
    }
    return map;
  }, [db]);

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
      map.addSource(AIRPORTS_SOURCE, { type: "geojson", data: airportsGeoJson(airports, new Map()) });
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
        paint: { "line-color": "#7fd0ff", "line-width": 2, "line-dasharray": [2, 1.5] },
      });

      // Chart imagery renders as the base layer, under the airport/runway/
      // procedure overlays above -- inserted before "airports-circle"
      // (already added) rather than appended, so it doesn't cover them.
      for (const chart of charts) {
        const sourceId = `chart-${chart.id}`;
        map.addSource(sourceId, {
          type: "raster",
          url: `pmtiles://${chart.tile_url}`,
          tileSize: 256,
        });
        map.addLayer({ id: sourceId, type: "raster", source: sourceId }, "airports-circle");
      }

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
        if (icao) onSelectAirport(icao);
      });
      map.on("mouseenter", "airports-circle", () => {
        map.getCanvas().style.cursor = "pointer";
      });
      map.on("mouseleave", "airports-circle", () => {
        map.getCanvas().style.cursor = "";
      });

      if (airports.length > 0) {
        const bounds = airports.reduce(
          (b, a) => b.extend([a.lon, a.lat]),
          new maplibregl.LngLatBounds([airports[0].lon, airports[0].lat], [airports[0].lon, airports[0].lat]),
        );
        map.fitBounds(bounds, { padding: 60, maxZoom: 10, duration: 0 });
      }

      // Weather overlays are all fetched from ff-api, which isn't started
      // by `npm run dev` (see apps/web/README.md) — each is independent so
      // one being unreachable doesn't block the others or the base map.
      void loadWeatherOverlays(map, airports);

      setLoaded(true);
    });

    return () => {
      map.remove();
      mapRef.current = null;
      setLoaded(false);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [db]);

  useEffect(() => {
    const map = mapRef.current;
    if (!map || !loaded) return;

    const runways = selectedIcao
      ? queryAll<Runway>(db, "SELECT * FROM runway WHERE airport_icao = ?", [selectedIcao])
      : [];
    (map.getSource(RUNWAYS_SOURCE) as maplibregl.GeoJSONSource | undefined)?.setData(runwaysGeoJson(runways));

    if (selectedIcao) {
      const airport = airports.find((a) => a.icao === selectedIcao);
      if (airport) {
        map.flyTo({ center: [airport.lon, airport.lat], zoom: 12, duration: 800 });
      }
    }
  }, [db, airports, selectedIcao, loaded]);

  useEffect(() => {
    const map = mapRef.current;
    if (!map || !loaded) return;

    let data = EMPTY_COLLECTION;
    if (selectedProcedureId) {
      const transitions = queryAll<ProcedureTransition>(
        db,
        "SELECT * FROM procedure_transition WHERE procedure_id = ?",
        [selectedProcedureId],
      );
      const legsByTransition = new Map<string, ProcedureLeg[]>();
      for (const t of transitions) {
        legsByTransition.set(
          t.id,
          queryAll<ProcedureLeg>(db, "SELECT * FROM procedure_leg WHERE transition_id = ? ORDER BY seq", [t.id]),
        );
      }
      data = procedureGeoJson(transitions, legsByTransition, fixes);
    }
    (map.getSource(PROCEDURE_SOURCE) as maplibregl.GeoJSONSource | undefined)?.setData(data);
  }, [db, fixes, selectedProcedureId, loaded]);

  return <div ref={containerRef} className="map-view" />;
}
