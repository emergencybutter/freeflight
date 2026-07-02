import { useEffect, useMemo, useState } from "react";
import type { Database } from "sql.js";
import { loadDemoDatabase, queryAll } from "./db";
import type { Airport, Procedure, ProcedureLeg, ProcedureTransition, Runway } from "./types";
import "./App.css";

export default function App() {
  const [db, setDb] = useState<Database | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [selectedIcao, setSelectedIcao] = useState<string | null>(null);
  const [selectedProcedureId, setSelectedProcedureId] = useState<string | null>(null);

  useEffect(() => {
    loadDemoDatabase()
      .then((database) => {
        setDb(database);
        const airports = queryAll<Airport>(database, "SELECT icao FROM airport ORDER BY icao");
        if (airports.length > 0) {
          setSelectedIcao(airports[0].icao);
        }
      })
      .catch((err: unknown) => setLoadError(err instanceof Error ? err.message : String(err)));
  }, []);

  if (loadError) {
    return (
      <div className="error">
        Failed to load demo data: {loadError}
        <br />
        Run <code>cargo run -p ff-etl --example build_demo_bundle -- &lt;cifp-file&gt; apps/web/public/demo-cycle.sqlite
        KSFO KOAK KSJC KPAO KHWD</code> first.
      </div>
    );
  }

  if (!db) {
    return <div className="loading">Loading freeflight demo…</div>;
  }

  return (
    <div className="layout">
      <AirportList db={db} selectedIcao={selectedIcao} onSelect={(icao) => {
        setSelectedIcao(icao);
        setSelectedProcedureId(null);
      }} />
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
  const procedures = useMemo(
    () => queryAll<Procedure>(db, "SELECT * FROM procedure WHERE airport_icao = ? ORDER BY kind, ident", [icao]),
    [db, icao],
  );

  if (!airport) return null;

  const byKind = (kind: string) => procedures.filter((p) => p.kind === kind);

  return (
    <div className="panel airport-detail">
      <h2>
        {airport.icao} — {airport.name}
      </h2>
      <p className="hint">
        {airport.lat.toFixed(4)}, {airport.lon.toFixed(4)} · elevation {airport.elevation_ft} ft
      </p>

      <h3>Runways</h3>
      <table>
        <thead>
          <tr>
            <th>Ident</th>
            <th>Length</th>
            <th>Width</th>
            <th>Ends</th>
          </tr>
        </thead>
        <tbody>
          {runways.map((r) => (
            <tr key={r.ident}>
              <td>{r.ident}</td>
              <td>{r.length_ft.toLocaleString()} ft</td>
              <td>{r.width_ft} ft</td>
              <td>
                {r.le_ident} ({r.le_heading_deg.toFixed(0)}°) / {r.he_ident} ({r.he_heading_deg.toFixed(0)}°)
              </td>
            </tr>
          ))}
        </tbody>
      </table>

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
