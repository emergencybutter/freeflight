import { useEffect, useState } from "react";
import { searchAirports } from "../data";
import type { Airport } from "../types";
import {
  checkWeightBalance,
  planRoute,
  type AircraftProfile,
  type RoutePlanSummary,
  type WeightAtStation,
  type WeightBalanceResult,
} from "./wasm";

/** A default profile shaped like a real Cessna 172 (same numbers used in
 * ff-planning's own tests) so the nav log/W&B sections have something
 * sensible to show before the user has typed anything in. */
const DEFAULT_PROFILE: AircraftProfile = {
  name: "Cessna 172",
  cruise_tas_kt: 110,
  fuel_burn_gph: 8.5,
  max_gross_weight_lb: 2450,
  forward_cg_limit_in: 35,
  aft_cg_limit_in: 47.3,
};

interface WeightRow {
  label: string;
  weight_lb: string;
  arm_in: string;
}

const DEFAULT_WEIGHT_ROWS: WeightRow[] = [
  { label: "Empty weight", weight_lb: "", arm_in: "" },
  { label: "Occupants", weight_lb: "", arm_in: "" },
  { label: "Fuel", weight_lb: "", arm_in: "" },
  { label: "Baggage", weight_lb: "", arm_in: "" },
];

function formatHours(hours: number): string {
  if (!Number.isFinite(hours)) return "—";
  const h = Math.floor(hours);
  const m = Math.round((hours - h) * 60);
  return `${h}:${m.toString().padStart(2, "0")}`;
}

/** Session-only flight planning: aircraft profile, route builder, an
 * auto-computed nav log, and a basic single-envelope weight & balance
 * check (DESIGN.md §9.3). Nothing here persists past a page reload —
 * the web client has no local database by design (§8), and cross-
 * device sync of plans/profiles is an explicit Phase 4 concern, not
 * this pass. All the math runs client-side via ff-wasm (`./wasm.ts`),
 * not through ff-api — DESIGN.md §5 scopes planning logic to ff-wasm
 * for exactly this reason. */
export function FlightPlanning() {
  const [profile, setProfile] = useState<AircraftProfile>(DEFAULT_PROFILE);
  const [route, setRoute] = useState<Airport[]>([]);
  const [navLog, setNavLog] = useState<RoutePlanSummary | null>(null);
  const [navLogError, setNavLogError] = useState<string | null>(null);

  useEffect(() => {
    if (route.length < 2) {
      setNavLog(null);
      setNavLogError(null);
      return;
    }
    let cancelled = false;
    planRoute(
      route.map((a) => ({ lat: a.lat, lon: a.lon })),
      profile,
    )
      .then((summary) => {
        if (!cancelled) {
          setNavLog(summary);
          setNavLogError(null);
        }
      })
      .catch((err: unknown) => {
        if (!cancelled) setNavLogError(err instanceof Error ? err.message : String(err));
      });
    return () => {
      cancelled = true;
    };
  }, [route, profile]);

  const hasWbEnvelope =
    profile.max_gross_weight_lb !== null &&
    profile.forward_cg_limit_in !== null &&
    profile.aft_cg_limit_in !== null;

  return (
    <div className="planning-layout">
      <AircraftProfileForm profile={profile} onChange={setProfile} />
      <RouteBuilder route={route} onChange={setRoute} />
      <div className="panel nav-log">
        <h2>Nav Log</h2>
        {route.length < 2 && <p className="hint">Add at least two airports to the route to see a nav log.</p>}
        {navLogError && <p className="hint">Couldn't compute nav log: {navLogError}</p>}
        {navLog && (
          <>
            <table>
              <thead>
                <tr>
                  <th>Leg</th>
                  <th>Dist (nm)</th>
                  <th>Course</th>
                  <th>Heading</th>
                  <th>GS (kt)</th>
                  <th>ETE</th>
                  <th>Fuel (gal)</th>
                </tr>
              </thead>
              <tbody>
                {navLog.legs.map((leg, i) => (
                  <tr key={i}>
                    <td>
                      {route[i].icao} → {route[i + 1].icao}
                    </td>
                    <td>{leg.distance_nm.toFixed(1)}</td>
                    <td>{leg.true_course_deg.toFixed(0)}°</td>
                    <td>{leg.true_heading_deg.toFixed(0)}°</td>
                    <td>{leg.ground_speed_kt.toFixed(0)}</td>
                    <td>{formatHours(leg.ete_hours)}</td>
                    <td>{leg.fuel_gal.toFixed(1)}</td>
                  </tr>
                ))}
              </tbody>
              <tfoot>
                <tr>
                  <td>
                    <strong>Total</strong>
                  </td>
                  <td>
                    <strong>{navLog.total_distance_nm.toFixed(1)}</strong>
                  </td>
                  <td />
                  <td />
                  <td />
                  <td>
                    <strong>{formatHours(navLog.total_ete_hours)}</strong>
                  </td>
                  <td>
                    <strong>{navLog.total_fuel_gal.toFixed(1)}</strong>
                  </td>
                </tr>
              </tfoot>
            </table>
            <p className="hint">No wind correction — headings equal course, groundspeed equals TAS (DESIGN.md §9.3).</p>
          </>
        )}
      </div>
      {hasWbEnvelope && (
        <WeightBalancePanel
          envelope={{
            max_gross_weight_lb: profile.max_gross_weight_lb as number,
            forward_cg_limit_in: profile.forward_cg_limit_in as number,
            aft_cg_limit_in: profile.aft_cg_limit_in as number,
          }}
        />
      )}
    </div>
  );
}

function AircraftProfileForm({
  profile,
  onChange,
}: {
  profile: AircraftProfile;
  onChange: (profile: AircraftProfile) => void;
}) {
  const numberField = (key: keyof AircraftProfile) => ({
    value: profile[key] === null ? "" : String(profile[key]),
    onChange: (e: React.ChangeEvent<HTMLInputElement>) => {
      const raw = e.target.value;
      onChange({ ...profile, [key]: raw === "" ? (key === "name" ? "" : null) : Number(raw) });
    },
  });

  return (
    <div className="panel aircraft-profile">
      <h2>Aircraft</h2>
      <label>
        Name
        <input
          type="text"
          value={profile.name}
          onChange={(e) => onChange({ ...profile, name: e.target.value })}
        />
      </label>
      <label>
        Cruise TAS (kt)
        <input type="number" {...numberField("cruise_tas_kt")} />
      </label>
      <label>
        Fuel burn (gal/hr)
        <input type="number" {...numberField("fuel_burn_gph")} />
      </label>
      <h3>Weight &amp; Balance envelope (optional)</h3>
      <p className="hint">Fill these in to enable the W&amp;B check below.</p>
      <label>
        Max gross weight (lb)
        <input type="number" {...numberField("max_gross_weight_lb")} />
      </label>
      <label>
        Forward CG limit (in)
        <input type="number" {...numberField("forward_cg_limit_in")} />
      </label>
      <label>
        Aft CG limit (in)
        <input type="number" {...numberField("aft_cg_limit_in")} />
      </label>
    </div>
  );
}

function RouteBuilder({ route, onChange }: { route: Airport[]; onChange: (route: Airport[]) => void }) {
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<Airport[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
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
          if (!cancelled) {
            setResults(airports);
            setError(null);
          }
        })
        .catch((err: unknown) => {
          if (!cancelled) setError(err instanceof Error ? err.message : String(err));
        });
    }, 200);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [query]);

  const addAirport = (airport: Airport) => {
    onChange([...route, airport]);
    setQuery("");
    setResults([]);
  };
  const removeAt = (i: number) => onChange(route.filter((_, idx) => idx !== i));
  const moveUp = (i: number) => {
    if (i === 0) return;
    const next = [...route];
    [next[i - 1], next[i]] = [next[i], next[i - 1]];
    onChange(next);
  };
  const moveDown = (i: number) => {
    if (i === route.length - 1) return;
    const next = [...route];
    [next[i], next[i + 1]] = [next[i + 1], next[i]];
    onChange(next);
  };

  return (
    <div className="panel route-builder">
      <h2>Route</h2>
      <input
        className="airport-search"
        type="search"
        placeholder="Add an airport (ident or name)…"
        value={query}
        onChange={(e) => setQuery(e.target.value)}
      />
      {error && <p className="hint">search failed: {error}</p>}
      {results.length > 0 && (
        <ul>
          {results.map((a) => (
            <li key={a.icao}>
              <button onClick={() => addAirport(a)}>
                <strong>{a.icao}</strong> — <span className="airport-name">{a.name}</span>
              </button>
            </li>
          ))}
        </ul>
      )}
      {route.length === 0 && <p className="hint">No waypoints yet — search above to add the first one.</p>}
      <ol className="route-list">
        {route.map((a, i) => (
          <li key={`${a.icao}-${i}`}>
            <span>
              <strong>{a.icao}</strong> <span className="airport-name">{a.name}</span>
            </span>
            <span className="route-list-actions">
              <button onClick={() => moveUp(i)} disabled={i === 0} aria-label={`Move ${a.icao} up`}>
                ↑
              </button>
              <button onClick={() => moveDown(i)} disabled={i === route.length - 1} aria-label={`Move ${a.icao} down`}>
                ↓
              </button>
              <button onClick={() => removeAt(i)} aria-label={`Remove ${a.icao}`}>
                ×
              </button>
            </span>
          </li>
        ))}
      </ol>
    </div>
  );
}

function WeightBalancePanel({
  envelope,
}: {
  envelope: { max_gross_weight_lb: number; forward_cg_limit_in: number; aft_cg_limit_in: number };
}) {
  const [rows, setRows] = useState<WeightRow[]>(DEFAULT_WEIGHT_ROWS);
  const [result, setResult] = useState<WeightBalanceResult | null>(null);
  const [error, setError] = useState<string | null>(null);

  const updateRow = (i: number, field: "weight_lb" | "arm_in", value: string) => {
    const next = [...rows];
    next[i] = { ...next[i], [field]: value };
    setRows(next);
  };

  useEffect(() => {
    const items: WeightAtStation[] = rows
      .map((r) => ({ weight_lb: Number(r.weight_lb), arm_in: Number(r.arm_in) }))
      .filter((r) => r.weight_lb > 0 && Number.isFinite(r.arm_in));
    if (items.length === 0) {
      setResult(null);
      setError(null);
      return;
    }
    let cancelled = false;
    checkWeightBalance(items, envelope)
      .then((r) => {
        if (!cancelled) {
          setResult(r);
          setError(null);
        }
      })
      .catch((err: unknown) => {
        if (!cancelled) setError(err instanceof Error ? err.message : String(err));
      });
    return () => {
      cancelled = true;
    };
  }, [rows, envelope]);

  return (
    <div className="panel weight-balance">
      <h2>Weight &amp; Balance</h2>
      <p className="hint">Single-envelope check only — not a certified multi-envelope tool (DESIGN.md §9.3).</p>
      <table>
        <thead>
          <tr>
            <th>Station</th>
            <th>Weight (lb)</th>
            <th>Arm (in)</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((row, i) => (
            <tr key={row.label}>
              <td>{row.label}</td>
              <td>
                <input
                  type="number"
                  value={row.weight_lb}
                  onChange={(e) => updateRow(i, "weight_lb", e.target.value)}
                />
              </td>
              <td>
                <input type="number" value={row.arm_in} onChange={(e) => updateRow(i, "arm_in", e.target.value)} />
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      {error && <p className="hint">Couldn't check weight &amp; balance: {error}</p>}
      {result && (
        <p className={result.within_weight_limit && result.within_cg_limits ? "wb-pass" : "wb-fail"}>
          Total {result.total_weight_lb.toFixed(0)} lb (limit {envelope.max_gross_weight_lb.toFixed(0)} lb) · CG{" "}
          {result.cg_in.toFixed(1)} in (limits {envelope.forward_cg_limit_in}–{envelope.aft_cg_limit_in} in) ·{" "}
          {result.within_weight_limit && result.within_cg_limits ? "within limits" : "OUT OF LIMITS"}
        </p>
      )}
    </div>
  );
}
