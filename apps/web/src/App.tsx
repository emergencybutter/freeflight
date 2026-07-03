import { useEffect, useMemo, useState } from "react";
import type { Database } from "sql.js";
import { loadDatabase, queryAll } from "./db";
import { MapView } from "./MapView";
import type { SyncSource } from "./sync";
import type { Airport, Frequency, Metar, Procedure, ProcedureLeg, ProcedureTransition, Runway, Taf } from "./types";
import { API_BASE_URL, fetchMetar, fetchTaf } from "./weather";
import "./App.css";

function syncStatusText(cycleId: string | null, source: SyncSource): string {
  switch (source) {
    case "synced":
      return `Cycle ${cycleId} · synced from ff-api`;
    case "cached":
      return `Cycle ${cycleId} · offline (cached copy — ff-api unreachable)`;
    case "bundled":
      return "Using bundled demo data (ff-api unreachable, nothing cached yet)";
  }
}

export default function App() {
  const [db, setDb] = useState<Database | null>(null);
  const [syncStatus, setSyncStatus] = useState<{ cycleId: string | null; source: SyncSource } | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [selectedIcao, setSelectedIcao] = useState<string | null>(null);
  const [selectedProcedureId, setSelectedProcedureId] = useState<string | null>(null);

  useEffect(() => {
    loadDatabase()
      .then(({ db: database, cycleId, source }) => {
        setDb(database);
        setSyncStatus({ cycleId, source });
        const airports = queryAll<Airport>(database, "SELECT icao FROM airport ORDER BY icao");
        if (airports.length > 0) {
          setSelectedIcao(airports[0].icao);
        }
      })
      .catch((err: unknown) => setLoadError(err instanceof Error ? err.message : String(err)));
  }, []);

  const selectAirport = (icao: string) => {
    setSelectedIcao(icao);
    setSelectedProcedureId(null);
  };

  if (loadError) {
    return (
      <div className="error">
        Failed to load cycle data: {loadError}
        <br />
        Nothing to fall back to — check that <code>apps/web/public/demo-cycle.sqlite</code> exists (see
        apps/web/README.md).
      </div>
    );
  }

  if (!db || !syncStatus) {
    return <div className="loading">Loading freeflight…</div>;
  }

  return (
    <div className="app-layout">
      <div className="sync-status">{syncStatusText(syncStatus.cycleId, syncStatus.source)}</div>
      <MapView
        db={db}
        selectedIcao={selectedIcao}
        onSelectAirport={selectAirport}
        selectedProcedureId={selectedProcedureId}
      />
      <div className="layout">
        <AirportList db={db} selectedIcao={selectedIcao} onSelect={selectAirport} />
        {selectedIcao && (
          <AirportDetail
            db={db}
            icao={selectedIcao}
            selectedProcedureId={selectedProcedureId}
            onSelectProcedure={setSelectedProcedureId}
          />
        )}
        {selectedProcedureId && <ProcedureDetail db={db} procedureId={selectedProcedureId} />}
      </div>
    </div>
  );
}

function AirportList({
  db,
  selectedIcao,
  onSelect,
}: {
  db: Database;
  selectedIcao: string | null;
  onSelect: (icao: string) => void;
}) {
  const airports = useMemo(
    () => queryAll<Airport>(db, "SELECT * FROM airport ORDER BY icao"),
    [db],
  );

  return (
    <div className="panel airport-list">
      <h2>Airports</h2>
      <p className="hint">{airports.length} loaded from the demo cycle bundle</p>
      <ul>
        {airports.map((a) => (
          <li key={a.icao}>
            <button className={a.icao === selectedIcao ? "selected" : ""} onClick={() => onSelect(a.icao)}>
              <strong>{a.icao}</strong> {a.iata ? `(${a.iata})` : ""}
              <br />
              <span className="airport-name">{a.name}</span>
            </button>
          </li>
        ))}
      </ul>
    </div>
  );
}

function AirportDetail({
  db,
  icao,
  selectedProcedureId,
  onSelectProcedure,
}: {
  db: Database;
  icao: string;
  selectedProcedureId: string | null;
  onSelectProcedure: (id: string) => void;
}) {
  const airport = useMemo(
    () => queryAll<Airport>(db, "SELECT * FROM airport WHERE icao = ?", [icao])[0],
    [db, icao],
  );
  const runways = useMemo(
    () => queryAll<Runway>(db, "SELECT * FROM runway WHERE airport_icao = ? ORDER BY ident", [icao]),
    [db, icao],
  );
  const frequencies = useMemo(
    () => queryAll<Frequency>(db, "SELECT * FROM frequency WHERE airport_icao = ?", [icao]),
    [db, icao],
  );
  const procedures = useMemo(
    () => queryAll<Procedure>(db, "SELECT * FROM procedure WHERE airport_icao = ? ORDER BY kind, ident", [icao]),
    [db, icao],
  );

  if (!airport) return null;

  const byKind = (kind: string) => procedures.filter((p) => p.kind === kind);
  const freqPriority = ["CTAF", "UNICOM", "TWR", "GND", "CLNC DEL", "ATIS", "AWOS", "APP", "DEP", "OTHER"];
  const sortedFrequencies = [...frequencies].sort(
    (a, b) => freqPriority.indexOf(a.kind) - freqPriority.indexOf(b.kind),
  );

  return (
    <div className="panel airport-detail">
      <h2>
        {airport.icao} — {airport.name}
      </h2>
      <p className="hint">
        {airport.lat.toFixed(4)}, {airport.lon.toFixed(4)} · elevation {airport.elevation_ft} ft
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
          {runways.map((r) => (
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
      {sortedFrequencies.length === 0 && <p className="hint">none in this bundle</p>}
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
          {byKind(kind).length === 0 && <p className="hint">none in this bundle</p>}
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
 * (services/ff-api) so the client doesn't hit aviationweather.gov
 * directly. ff-api is a separate process from `npm run dev` — see
 * apps/web/README.md. */
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
      {error && (
        <p className="hint">
          Couldn't reach ff-api at {API_BASE_URL} — is it running? (<code>cargo run -p ff-api</code>, see
          apps/web/README.md)
        </p>
      )}
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

function ProcedureDetail({ db, procedureId }: { db: Database; procedureId: string }) {
  const procedure = useMemo(
    () => queryAll<Procedure>(db, "SELECT * FROM procedure WHERE id = ?", [procedureId])[0],
    [db, procedureId],
  );
  const transitions = useMemo(
    () =>
      queryAll<ProcedureTransition>(db, "SELECT * FROM procedure_transition WHERE procedure_id = ? ORDER BY kind, ident", [
        procedureId,
      ]),
    [db, procedureId],
  );
  const legsByTransition = useMemo(() => {
    const map = new Map<string, ProcedureLeg[]>();
    for (const t of transitions) {
      map.set(
        t.id,
        queryAll<ProcedureLeg>(db, "SELECT * FROM procedure_leg WHERE transition_id = ? ORDER BY seq", [t.id]),
      );
    }
    return map;
  }, [db, transitions]);

  if (!procedure) return null;

  return (
    <div className="panel procedure-detail">
      <h2>
        {procedure.kind} {procedure.ident}
      </h2>
      {procedure.runway_ident && <p className="hint">runway {procedure.runway_ident}</p>}
      {transitions.map((t) => (
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
              {(legsByTransition.get(t.id) ?? []).map((leg) => (
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
