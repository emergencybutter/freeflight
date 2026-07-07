import { useEffect, useMemo, useState } from "react";
import { API_BASE_URL } from "./api";
import { fetchAirportDetail, fetchAirportProcedures, fetchAirspaceInBbox, fetchCycleManifest, fetchProcedureDetail, searchAirports } from "./data";
import { MapView } from "./MapView";
import { findCrossedAirspace } from "./planning/airspaceCrossing";
import { expandRoute } from "./planning/expandRoute";
import { DEFAULT_PROFILE, FlightPlanning } from "./planning/FlightPlanning";
import { buildResolvedProcedure, transitionOptions } from "./planning/procedureLookup";
import type { AircraftProfile } from "./planning/wasm";
import { airspaceInfoHtml, cwaInfoHtml, gairmetInfoHtml, pirepInfoHtml, sigmetInfoHtml } from "./tapInfo";
import type {
  Airport,
  AirportDetail as AirportDetailData,
  AirspaceVolume,
  MapTapResult,
  Metar,
  NearestFix,
  Procedure,
  ProcedureDetail as ProcedureDetailData,
  RouteState,
  RouteWaypoint,
  Taf,
} from "./types";
import { fetchMetar, fetchTaf } from "./weather";
import "./App.css";

const EMPTY_ROUTE: RouteState = { departure: null, arrival: null, sid: null, star: null, middleTokens: [] };

type TapTabId = "airport" | "waypoint" | "airspace" | "pireps" | "airmet" | "sigmet" | "cwa";
const TAP_TABS: { id: TapTabId; label: string }[] = [
  { id: "airport", label: "Airport" },
  { id: "waypoint", label: "Waypoint" },
  { id: "airspace", label: "Airspace" },
  { id: "pireps", label: "PIREPs" },
  { id: "airmet", label: "AIRMET" },
  { id: "sigmet", label: "SIGMET" },
  { id: "cwa", label: "CWA" },
];

export default function App() {
  const [cycleId, setCycleId] = useState<string | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [selectedAirport, setSelectedAirport] = useState<Airport | null>(null);
  const [selectedProcedureId, setSelectedProcedureId] = useState<string | null>(null);
  const [view, setView] = useState<"map" | "plan">("map");
  // Which of the map tap tabs is showing, and the data from the most
  // recent tap (airspace/PIREP/AIRMET/SIGMET/CWA features at the point,
  // plus the nearest waypoint/navaid) — airport selection itself is
  // handled separately (selectedAirport below), since every tap selects
  // the closest airport unconditionally regardless of the active tab.
  const [activeTapTab, setActiveTapTab] = useState<TapTabId>("airport");
  const [mapTap, setMapTap] = useState<MapTapResult | null>(null);
  // Lifted out of FlightPlanning (rather than its own local state) so
  // MapView can draw the planned route too. Departure/arrival/SID/STAR
  // are dedicated slots (picked explicitly via FlightPlanning's route
  // builder) rather than positions in a flat token list — see
  // RouteState's doc comment in types.ts.
  const [route, setRoute] = useState<RouteState>(EMPTY_ROUTE);
  // Lifted out of FlightPlanning the same way route was — MapView reads
  // profile.cruise_altitude_ft to default its own winds-aloft altitude
  // selector to whatever the flight plan is actually using.
  const [profile, setProfile] = useState<AircraftProfile>(DEFAULT_PROFILE);
  const expandedMiddle = useMemo(() => {
    const before = (route.sid && route.sid.points[route.sid.points.length - 1]) ?? route.departure;
    const after = route.star?.points[0] ?? route.arrival;
    return expandRoute(route.middleTokens, before, after);
  }, [route.middleTokens, route.sid, route.departure, route.star, route.arrival]);
  const routePoints = useMemo<RouteWaypoint[]>(
    () => [
      ...(route.departure ? [route.departure] : []),
      ...(route.sid?.points ?? []),
      ...expandedMiddle.points,
      ...(route.star?.points ?? []),
      ...(route.arrival ? [route.arrival] : []),
    ],
    [route.departure, route.sid, expandedMiddle, route.star, route.arrival],
  );

  // Which real Class B/C/D + Special Use Airspace volumes the route's
  // legs actually pass through — computed here (rather than inside
  // FlightPlanning) since it needs the same bbox-filtered fetch MapView
  // already relies on for the airspace overlay. Always computed
  // regardless of VFR/IFR (cheap, and FlightPlanning decides whether to
  // show it) so flipping that toggle doesn't need a refetch.
  const [airspaceCrossings, setAirspaceCrossings] = useState<AirspaceVolume[]>([]);
  useEffect(() => {
    if (routePoints.length < 2) {
      setAirspaceCrossings([]);
      return;
    }
    let cancelled = false;
    const lats = routePoints.map((p) => p.lat);
    const lons = routePoints.map((p) => p.lon);
    const bbox = [Math.min(...lons), Math.min(...lats), Math.max(...lons), Math.max(...lats)].join(",");
    fetchAirspaceInBbox(bbox)
      .then((volumes) => {
        if (!cancelled) setAirspaceCrossings(findCrossedAirspace(routePoints, volumes));
      })
      .catch((err: unknown) => console.warn("couldn't check the route against airspace boundaries", err));
    return () => {
      cancelled = true;
    };
  }, [routePoints]);

  useEffect(() => {
    // Web assumes connectivity to ff-api (DESIGN.md §8): if this first
    // fetch fails there's nothing to fall back to — fail visibly.
    fetchCycleManifest()
      .then((manifest) => setCycleId(manifest.cycle_id))
      .catch((err: unknown) => setLoadError(err instanceof Error ? err.message : String(err)));
  }, []);

  const selectAirport = (airport: Airport | null) => {
    setSelectedAirport(airport);
    setSelectedProcedureId(null);
  };

  // Same Airport -> RouteWaypoint shape AirportSlot/addResult already
  // use in FlightPlanning.tsx (ident/name/lat/lon) — kept in sync with
  // that rather than introducing a second conversion.
  const airportToWaypoint = (airport: Airport): RouteWaypoint => ({
    ident: airport.icao,
    name: airport.name,
    lat: airport.lat,
    lon: airport.lon,
  });
  const setSelectedAirportAsDeparture = () => {
    if (!selectedAirport) return;
    setRoute({ ...route, departure: airportToWaypoint(selectedAirport), sid: null });
  };
  const setSelectedAirportAsArrival = () => {
    if (!selectedAirport) return;
    setRoute({ ...route, arrival: airportToWaypoint(selectedAirport), star: null });
  };
  const addSelectedAirportToFlightPlan = () => {
    if (!selectedAirport) return;
    setRoute({
      ...route,
      middleTokens: [...route.middleTokens, { kind: "point", point: airportToWaypoint(selectedAirport) }],
    });
  };

  if (loadError) {
    return (
      <div className="error">
        Can't reach ff-api at {API_BASE_URL} — this client has no offline mode (see DESIGN.md §8).
        <br />
        Start it with <code>cargo run -p ff-api</code> (and publish a cycle first with <code>cargo run -p ff-etl</code>).
        <br />
        <span className="hint">{loadError}</span>
      </div>
    );
  }

  if (!cycleId) {
    return <div className="loading">Loading freeflight…</div>;
  }

  return (
    <div className="app-layout">
      <div className="sync-status">
        <span className="view-toggle">
          <button className={view === "map" ? "selected" : ""} onClick={() => setView("map")}>
            Map
          </button>
          <button className={view === "plan" ? "selected" : ""} onClick={() => setView("plan")}>
            Flight Plan
          </button>
        </span>
      </div>
      {/* Both views stay mounted once shown — conditionally rendering
          them out of the tree on every toggle used to destroy the
          Flight Plan view's own React state (the route, the profile,
          W&B loads) the instant you switched to the map and back.
          Visibility is CSS-only; MapView gets a `visible` prop so it
          can call MapLibre's resize() when it reappears, since a map
          left `display: none` doesn't repaint correctly on its own. */}
      <div style={{ display: view === "map" ? "contents" : "none" }}>
        <MapView
          selectedAirport={selectedAirport}
          onSelectAirport={selectAirport}
          onMapTap={setMapTap}
          selectedProcedureId={selectedProcedureId}
          visible={view === "map"}
          route={routePoints}
          preferredAltitudeFt={profile.cruise_altitude_ft}
        />
        <div className="tap-tab-bar">
          {TAP_TABS.map((tab) => (
            <button
              key={tab.id}
              className={activeTapTab === tab.id ? "selected" : ""}
              onClick={() => setActiveTapTab(tab.id)}
            >
              {tab.label}
            </button>
          ))}
        </div>
        {activeTapTab === "airport" && (
          <>
            <AirportActionBar
              selectedAirport={selectedAirport}
              isDeparture={selectedAirport !== null && selectedAirport.icao === route.departure?.ident}
              isArrival={selectedAirport !== null && selectedAirport.icao === route.arrival?.ident}
              onSelectAirport={selectAirport}
              onSetDeparture={setSelectedAirportAsDeparture}
              onSetArrival={setSelectedAirportAsArrival}
              onAddToFlightPlan={addSelectedAirportToFlightPlan}
            />
            <div className="layout">
              {selectedAirport && (
                <AirportPanel
                  icao={selectedAirport.icao}
                  selectedProcedureId={selectedProcedureId}
                  onSelectProcedure={setSelectedProcedureId}
                />
              )}
              {selectedProcedureId && (
                <ProcedurePanel procedureId={selectedProcedureId} route={route} onRouteChange={setRoute} />
              )}
            </div>
          </>
        )}
        {activeTapTab === "waypoint" && (
          <WaypointTab nearestFix={mapTap?.nearestFix ?? null} route={route} onRouteChange={setRoute} />
        )}
        {activeTapTab === "airspace" && (
          <TapInfoTab items={mapTap?.airspace ?? []} formatter={airspaceInfoHtml} emptyText="No airspace at the last tap." />
        )}
        {activeTapTab === "pireps" && (
          <TapInfoTab items={mapTap?.pireps ?? []} formatter={pirepInfoHtml} emptyText="No PIREP at the last tap." />
        )}
        {activeTapTab === "airmet" && (
          <TapInfoTab items={mapTap?.gairmets ?? []} formatter={gairmetInfoHtml} emptyText="No G-AIRMET at the last tap." />
        )}
        {activeTapTab === "sigmet" && (
          <TapInfoTab items={mapTap?.sigmets ?? []} formatter={sigmetInfoHtml} emptyText="No SIGMET at the last tap." />
        )}
        {activeTapTab === "cwa" && (
          <TapInfoTab items={mapTap?.cwas ?? []} formatter={cwaInfoHtml} emptyText="No CWA at the last tap." />
        )}
      </div>
      <div style={{ display: view === "plan" ? "contents" : "none" }}>
        <FlightPlanning
          route={route}
          onRouteChange={setRoute}
          points={routePoints}
          warnings={expandedMiddle.warnings}
          airspaceCrossings={airspaceCrossings}
          profile={profile}
          onProfileChange={setProfile}
        />
      </div>
      <div className="status-footer">
        Cycle {cycleId} · live from ff-api at {API_BASE_URL}
      </div>
    </div>
  );
}

/** The Waypoint tap tab: the nearest waypoint/navaid to the last map
 * tap (fetched server-side — see MapView's tap handler), with a button
 * to drop it into the route builder's middle fixes list, same shape
 * AirportActionBar's "Add to Flight Plan" and the route builder's own
 * ident search already build (`{ kind: "point", point: {...} }`). */
function WaypointTab({
  nearestFix,
  route,
  onRouteChange,
}: {
  nearestFix: NearestFix | null;
  route: RouteState;
  onRouteChange: (route: RouteState) => void;
}) {
  const addToFlightPlan = () => {
    if (!nearestFix) return;
    onRouteChange({
      ...route,
      middleTokens: [
        ...route.middleTokens,
        { kind: "point", point: { ident: nearestFix.ident, name: null, lat: nearestFix.lat, lon: nearestFix.lon } },
      ],
    });
  };

  return (
    <div className="tap-info-tab">
      {nearestFix ? (
        <>
          <h2>
            {nearestFix.kind === "WAYPOINT" ? "Waypoint" : nearestFix.kind}: {nearestFix.ident}
            <button className="add-procedure-button" onClick={addToFlightPlan} aria-label="Add to Flight Plan">
              +
            </button>
          </h2>
          <p className="hint">
            {nearestFix.lat.toFixed(4)}, {nearestFix.lon.toFixed(4)}
          </p>
        </>
      ) : (
        <p className="hint">Tap the map to find the nearest waypoint or navaid.</p>
      )}
    </div>
  );
}

/** The Airspace/PIREPs/AIRMET/SIGMET/CWA tap tabs: one card per feature
 * found at the last map tap, rendered from the same pre-escaped HTML
 * fragments (tapInfo.ts) the removed MapLibre Popups used to show —
 * reused as-is rather than re-deriving the formatting logic as JSX. */
function TapInfoTab({
  items,
  formatter,
  emptyText,
}: {
  items: Record<string, unknown>[];
  formatter: (props: Record<string, unknown>) => string;
  emptyText: string;
}) {
  if (items.length === 0) {
    return (
      <div className="tap-info-tab">
        <p className="hint">{emptyText}</p>
      </div>
    );
  }
  return (
    <div className="tap-info-tab">
      {items.map((props, i) => (
        // eslint-disable-next-line react/no-array-index-key -- these
        // rows have no stable id of their own across taps; the tap
        // itself (not this list) is the thing that changes over time.
        <div key={i} className="tap-info-item" dangerouslySetInnerHTML={{ __html: formatter(props) }} />
      ))}
    </div>
  );
}

/** The map tab's bottom bar: shows the selected airport and lets it be
 * changed by tapping the label, replacing what used to be a separate
 * always-visible search panel — tapping swaps the label for a search
 * input (same debounced /data/search this replaces), and picking a
 * result swaps back to label mode. The three route-builder actions
 * stay visible either way, just disabled with nothing selected. */
function AirportActionBar({
  selectedAirport,
  isDeparture,
  isArrival,
  onSelectAirport,
  onSetDeparture,
  onSetArrival,
  onAddToFlightPlan,
}: {
  selectedAirport: Airport | null;
  isDeparture: boolean;
  isArrival: boolean;
  onSelectAirport: (airport: Airport) => void;
  onSetDeparture: () => void;
  onSetArrival: () => void;
  onAddToFlightPlan: () => void;
}) {
  const [editing, setEditing] = useState(false);
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<Airport[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!editing) return;
    const q = query.trim();
    if (q.length < 2) {
      setResults([]);
      setError(null);
      return;
    }
    let cancelled = false;
    const timer = setTimeout(() => {
      searchAirports(q)
        .then((airports) => {
          if (cancelled) return;
          setResults(airports);
          setError(null);
        })
        .catch((err: unknown) => {
          if (cancelled) return;
          setError(err instanceof Error ? err.message : String(err));
        });
    }, 200);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [query, editing]);

  const pick = (airport: Airport) => {
    onSelectAirport(airport);
    setEditing(false);
    setQuery("");
    setResults([]);
  };

  return (
    <div className="airport-action-bar">
      {editing ? (
        <span className="airport-action-bar-search">
          <input
            className="airport-search"
            type="search"
            placeholder="Search ident or name (e.g. KSFO, O'Hare)…"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            // A blur fired by clicking a result button would otherwise
            // close the dropdown before that click's onClick ever runs
            // — deferring the revert-to-label lets the click land first.
            onBlur={() => setTimeout(() => setEditing(false), 150)}
            autoFocus
          />
          {error && <p className="hint">search failed: {error}</p>}
          {results.length > 0 && (
            <ul className="airport-action-bar-results">
              {results.map((a) => (
                <li key={a.icao}>
                  <button onClick={() => pick(a)}>
                    <strong>{a.icao}</strong> {a.iata ? `(${a.iata})` : ""} — <span className="airport-name">{a.name}</span>
                  </button>
                </li>
              ))}
            </ul>
          )}
        </span>
      ) : (
        <button className="airport-action-bar-label" onClick={() => setEditing(true)}>
          {selectedAirport ? `${selectedAirport.icao} — ${selectedAirport.name}` : "No airport selected — tap to search"}
        </button>
      )}
      <span className="view-toggle">
        <button className={isDeparture ? "selected" : ""} disabled={!selectedAirport} onClick={onSetDeparture}>
          Set as Departure
        </button>
        <button className={isArrival ? "selected" : ""} disabled={!selectedAirport} onClick={onSetArrival}>
          Set as Arrival
        </button>
        <button disabled={!selectedAirport} onClick={onAddToFlightPlan} aria-label="Add to Flight Plan">
          +
        </button>
      </span>
    </div>
  );
}

function AirportPanel({
  icao,
  selectedProcedureId,
  onSelectProcedure,
}: {
  icao: string;
  selectedProcedureId: string | null;
  onSelectProcedure: (id: string) => void;
}) {
  const [detail, setDetail] = useState<AirportDetailData | null>(null);
  const [procedures, setProcedures] = useState<Procedure[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setDetail(null);
    setError(null);
    Promise.all([fetchAirportDetail(icao), fetchAirportProcedures(icao)])
      .then(([airportDetail, procedureList]) => {
        if (cancelled) return;
        setDetail(airportDetail);
        setProcedures(procedureList);
      })
      .catch((err: unknown) => {
        if (cancelled) return;
        setError(err instanceof Error ? err.message : String(err));
      });
    return () => {
      cancelled = true;
    };
  }, [icao]);

  if (error) {
    return (
      <div className="panel airport-detail">
        <p className="hint">Failed to load {icao}: {error}</p>
      </div>
    );
  }
  if (!detail) {
    return (
      <div className="panel airport-detail">
        <p className="hint">loading…</p>
      </div>
    );
  }

  const byKind = (kind: string) => procedures.filter((p) => p.kind === kind);
  const freqPriority = ["CTAF", "UNICOM", "TWR", "GND", "CLNC DEL", "ATIS", "AWOS", "APP", "DEP", "OTHER"];
  const sortedFrequencies = [...detail.frequencies].sort(
    (a, b) => freqPriority.indexOf(a.kind) - freqPriority.indexOf(b.kind),
  );

  return (
    <div className="panel airport-detail">
      <h2>
        {detail.icao} — {detail.name}
      </h2>
      <p className="hint">
        {detail.lat.toFixed(4)}, {detail.lon.toFixed(4)} · elevation {detail.elevation_ft} ft
      </p>

      <WeatherSection icao={icao} />

      <h3>Runways</h3>
      <table>
        <thead>
          <tr>
            <th>Ident</th>
            <th>Length</th>
            <th>Width</th>
            <th>Surface</th>
            <th>Ends</th>
          </tr>
        </thead>
        <tbody>
          {detail.runways.map((r) => (
            <tr key={r.ident}>
              <td>{r.ident}</td>
              <td>{r.length_ft.toLocaleString()} ft</td>
              <td>{r.width_ft} ft</td>
              <td>{r.surface}</td>
              <td>
                {r.le_ident} ({r.le_heading_deg.toFixed(0)}°) / {r.he_ident} ({r.he_heading_deg.toFixed(0)}°)
              </td>
            </tr>
          ))}
        </tbody>
      </table>

      <h3>Frequencies</h3>
      {sortedFrequencies.length === 0 && <p className="hint">none in this cycle</p>}
      {sortedFrequencies.length > 0 && (
        <table>
          <thead>
            <tr>
              <th>Use</th>
              <th>Freq</th>
              <th>Detail</th>
            </tr>
          </thead>
          <tbody>
            {sortedFrequencies.map((f, i) => (
              <tr key={i}>
                <td>{f.kind}</td>
                <td>{f.freq_mhz.toFixed(3).replace(/0+$/, "").replace(/\.$/, "")}</td>
                <td>{f.remarks ?? "—"}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}

      {(["SID", "STAR", "APPROACH"] as const).map((kind) => (
        <div key={kind}>
          <h3>{kind === "APPROACH" ? "Approaches" : kind + "s"}</h3>
          {byKind(kind).length === 0 && <p className="hint">none in this cycle</p>}
          <ul className="procedure-list">
            {byKind(kind).map((p) => (
              <li key={p.id}>
                <button
                  className={p.id === selectedProcedureId ? "selected" : ""}
                  onClick={() => onSelectProcedure(p.id)}
                >
                  {p.ident}
                </button>
              </li>
            ))}
          </ul>
        </div>
      ))}
    </div>
  );
}

function formatWind(wdir: number | string | null, wspd: number | null, wgst: number | null): string | null {
  if (wdir === null || wspd === null) return null;
  const dir = wdir === "VRB" ? "VRB" : `${wdir}°`;
  const gust = wgst !== null ? `G${wgst}` : "";
  return `${dir} at ${wspd}${gust}kt`;
}

/** Live METAR/TAF for the selected airport, proxied through ff-api
 * (services/ff-api). */
function WeatherSection({ icao }: { icao: string }) {
  const [metar, setMetar] = useState<Metar | null>(null);
  const [taf, setTaf] = useState<Taf | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    Promise.all([fetchMetar(icao), fetchTaf(icao)])
      .then(([m, t]) => {
        if (cancelled) return;
        setMetar(m);
        setTaf(t);
      })
      .catch((err: unknown) => {
        if (cancelled) return;
        setError(err instanceof Error ? err.message : String(err));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [icao]);

  return (
    <>
      <h3>Weather</h3>
      {loading && <p className="hint">loading…</p>}
      {error && <p className="hint">Couldn't fetch weather: {error}</p>}
      {!loading && !error && (
        <>
          {metar ? (
            <p className="weather-report">
              <span className="raw-report">{metar.rawOb}</span>
              <br />
              <span className="hint">
                {[
                  metar.temp !== null ? `${metar.temp}°C` : null,
                  metar.dewp !== null ? `dewpoint ${metar.dewp}°C` : null,
                  formatWind(metar.wdir, metar.wspd, metar.wgst),
                  metar.altim !== null ? `altimeter ${metar.altim}` : null,
                ]
                  .filter(Boolean)
                  .join(" · ")}
              </span>
            </p>
          ) : (
            <p className="hint">no current METAR</p>
          )}
          {taf ? <p className="weather-report raw-report">{taf.rawTAF}</p> : <p className="hint">no current TAF</p>}
        </>
      )}
    </>
  );
}

function ProcedurePanel({
  procedureId,
  route,
  onRouteChange,
}: {
  procedureId: string;
  route: RouteState;
  onRouteChange: (route: RouteState) => void;
}) {
  const [detail, setDetail] = useState<ProcedureDetailData | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [pickingTransition, setPickingTransition] = useState(false);

  useEffect(() => {
    let cancelled = false;
    setDetail(null);
    setError(null);
    setPickingTransition(false);
    fetchProcedureDetail(procedureId)
      .then((d) => {
        if (!cancelled) setDetail(d);
      })
      .catch((err: unknown) => {
        if (!cancelled) setError(err instanceof Error ? err.message : String(err));
      });
    return () => {
      cancelled = true;
    };
  }, [procedureId]);

  if (error) {
    return (
      <div className="panel procedure-detail">
        <p className="hint">Failed to load procedure: {error}</p>
      </div>
    );
  }
  if (!detail) {
    return (
      <div className="panel procedure-detail">
        <p className="hint">loading…</p>
      </div>
    );
  }

  // "SID"/"STAR" only in practice (Procedure.kind is a plain string
  // since APPROACH procedures share the same shape, but this panel's
  // caller — AirportPanel's procedure list — only ever links to SID/
  // STAR entries). Same "skip the picker if there's only one real
  // choice" behavior as FlightPlanning's own ProcedurePickerButton:
  // resolves immediately when there's a single enroute transition (or
  // none at all — falls back to COMMON for the rare procedure that's
  // just runway/common legs with no named enroute segment), and only
  // shows the transition list when there's an actual choice to make.
  const resolveWith = (transitionId: string) => {
    const kind = detail.kind as "SID" | "STAR";
    const resolved = buildResolvedProcedure(detail.airport_icao, kind, detail, transitionId);
    if (!resolved) return;
    onRouteChange(kind === "SID" ? { ...route, sid: resolved } : { ...route, star: resolved });
    setPickingTransition(false);
  };
  const addToFlightPlan = () => {
    const enroute = transitionOptions(detail);
    if (enroute.length > 1) {
      setPickingTransition(true);
      return;
    }
    const transitionId = enroute[0]?.id ?? detail.transitions.find((t) => t.kind === "COMMON")?.id;
    if (transitionId) resolveWith(transitionId);
  };

  return (
    <div className="panel procedure-detail">
      <h2>
        {detail.kind} {detail.ident}
        <button className="add-procedure-button" onClick={addToFlightPlan} aria-label="Add to Flight Plan">
          +
        </button>
      </h2>
      {pickingTransition && (
        <ul className="procedure-list">
          {transitionOptions(detail).map((t) => (
            <li key={t.id}>
              <button onClick={() => resolveWith(t.id)}>{t.ident}</button>
            </li>
          ))}
        </ul>
      )}
      {detail.runway_ident && <p className="hint">runway {detail.runway_ident}</p>}
      {detail.chart_url ? (
        <iframe
          src={detail.chart_url}
          title={detail.chart_name ?? `${detail.kind} ${detail.ident} plate`}
          className="dtpp-chart-frame"
        />
      ) : (
        <p className="hint">No FAA plate chart matched for this procedure.</p>
      )}
      {detail.transitions.map((t) => (
        <div key={t.id} className="transition">
          <h3>
            {t.kind}
            {t.ident ? ` — ${t.ident}` : ""}
          </h3>
          <table>
            <thead>
              <tr>
                <th>Seq</th>
                <th>Leg</th>
                <th>Fix</th>
                <th>Course</th>
                <th>Altitude</th>
              </tr>
            </thead>
            <tbody>
              {t.legs.map((leg) => (
                <tr key={leg.seq}>
                  <td>{leg.seq}</td>
                  <td>{leg.path_and_term}</td>
                  <td>{leg.fix_ident ?? "—"}</td>
                  <td>{leg.course_deg !== null ? `${leg.course_deg.toFixed(0)}°` : "—"}</td>
                  <td>{leg.altitude_constraint ?? "—"}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      ))}
    </div>
  );
}
