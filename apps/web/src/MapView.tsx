import { useEffect, useMemo, useRef, useState } from "react";
import maplibregl, { type Map as MlMap, type StyleSpecification } from "maplibre-gl";
import "maplibre-gl/dist/maplibre-gl.css";
import type { Database } from "sql.js";
import { queryAll } from "./db";
import type { Airport, Fix, ProcedureLeg, ProcedureTransition, Runway } from "./types";

const AIRPORTS_SOURCE = "airports";
const RUNWAYS_SOURCE = "runways";
const PROCEDURE_SOURCE = "procedure-path";

// No basemap tiles: DESIGN.md's map is built on our own charts.pmtiles
// (raster sectionals/TACs), not a third-party basemap — that pipeline
// exists (ff-charts) but has no real chart bundled yet (see TODO.md). A
// plain background keeps the map usable in the meantime and matches the
// app's dark theme.
const BLANK_STYLE: StyleSpecification = {
  version: 8,
  sources: {},
  layers: [{ id: "background", type: "background", paint: { "background-color": "#0b1220" } }],
};

const EMPTY_COLLECTION: GeoJSON.FeatureCollection = { type: "FeatureCollection", features: [] };

function airportsGeoJson(airports: Airport[]): GeoJSON.FeatureCollection {
  return {
    type: "FeatureCollection",
    features: airports.map((a) => ({
      type: "Feature",
      geometry: { type: "Point", coordinates: [a.lon, a.lat] },
      properties: { icao: a.icao },
    })),
  };
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
      map.addSource(AIRPORTS_SOURCE, { type: "geojson", data: airportsGeoJson(airports) });
      map.addLayer({
        id: "airports-circle",
        type: "circle",
        source: AIRPORTS_SOURCE,
        paint: {
          "circle-radius": 5,
          "circle-color": "#3d7fc4",
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
