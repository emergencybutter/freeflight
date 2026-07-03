import { useEffect, useState } from "react";
import { API_BASE_URL } from "./api";
import { fetchAirportDetail, fetchAirportProcedures, fetchAirports, fetchCycleManifest, fetchProcedureDetail } from "./data";
import { MapView } from "./MapView";
import type { Airport, AirportDetail as AirportDetailData, Metar, Procedure, ProcedureDetail as ProcedureDetailData, Taf } from "./types";
import { fetchMetar, fetchTaf } from "./weather";
import "./App.css";

export default function App() {
  const [airports, setAirports] = useState<Airport[] | null>(null);
  const [cycleId, setCycleId] = useState<string | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [selectedIcao, setSelectedIcao] = useState<string | null>(null);
  const [selectedProcedureId, setSelectedProcedureId] = useState<string | null>(null);

  useEffect(() => {
    // Web assumes connectivity to ff-api (DESIGN.md §8): if this first
    // fetch fails there's nothing to fall back to — fail visibly.
    Promise.all([fetchAirports(), fetchCycleManifest()])
      .then(([airportList, manifest]) => {
        setAirports(airportList);
        setCycleId(manifest.cycle_id);
        if (airportList.length > 0) {
          setSelectedIcao(airportList[0].icao);
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
        Can't reach ff-api at {API_BASE_URL} — this client has no offline mode (see DESIGN.md §8).
        <br />
        Start it with <code>cargo run -p ff-api</code> (and publish a cycle first with <code>cargo run -p ff-etl</code>).
        <br />
        <span className="hint">{loadError}</span>
      </div>
    );
  }

  if (!airports) {
    return <div className="loading">Loading freeflight…</div>;
  }

  return (
    <div className="app-layout">
      <div className="sync-status">
        Cycle {cycleId} · live from ff-api at {API_BASE_URL}
      </div>
      <MapView
        airports={airports}
        selectedIcao={selectedIcao}
        onSelectAirport={selectAirport}
        selectedProcedureId={selectedProcedureId}
      />
      <div className="layout">
        <AirportList airports={airports} selectedIcao={selectedIcao} onSelect={selectAirport} />
        {selectedIcao && (
          <AirportPanel
            icao={selectedIcao}
            selectedProcedureId={selectedProcedureId}
            onSelectProcedure={setSelectedProcedureId}
          />
        )}
        {selectedProcedureId && <ProcedurePanel procedureId={selectedProcedureId} />}
      </div>
    </div>
  );
}

function AirportList({
  airports,
  selectedIcao,
  onSelect,
}: {
  airports: Airport[];
  selectedIcao: string | null;
  onSelect: (icao: string) => void;
}) {
  return (
    <div className="panel airport-list">
      <h2>Airports</h2>
      <p className="hint">{airports.length} in the current cycle</p>
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

function ProcedurePanel({ procedureId }: { procedureId: string }) {
  const [detail, setDetail] = useState<ProcedureDetailData | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setDetail(null);
    setError(null);
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

  return (
    <div className="panel procedure-detail">
      <h2>
        {detail.kind} {detail.ident}
      </h2>
      {detail.runway_ident && <p className="hint">runway {detail.runway_ident}</p>}
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
