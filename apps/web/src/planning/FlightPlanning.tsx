import { useEffect, useState } from "react";
import { fetchAirwayDetail, fetchProcedureDetail, searchAirports, searchIdents } from "../data";
import { fetchWindsAloft } from "../weather";
import { buildResolvedProcedure, fetchProcedureOptions, transitionOptions } from "./procedureLookup";
import { windsForRoute } from "./windsAloft";
import type {
  Airport,
  AirspaceVolume,
  IdentSearchRow,
  Procedure,
  ProcedureDetail,
  ProcedureTransitionDetail,
  ResolvedProcedureRef,
  RouteState,
  RouteToken,
  RouteWaypoint,
  WindsAloftBulletin,
} from "../types";
import {
  checkWeightBalance,
  planRoute,
  type AircraftProfile,
  type PlanningWind,
  type RoutePlanSummary,
  type WeightAtStation,
  type WeightBalanceResult,
} from "./wasm";

/** A default profile shaped like a real Cessna 172 (same numbers used in
 * ff-planning's own tests) so the nav log/W&B sections have something
 * sensible to show before the user has typed anything in. No cruise
 * altitude by default — wind correction only kicks in once one's set. */
const DEFAULT_PROFILE: AircraftProfile = {
  name: "Cessna 172",
  cruise_tas_kt: 110,
  fuel_burn_gph: 8.5,
  max_gross_weight_lb: 2450,
  forward_cg_limit_in: 35,
  aft_cg_limit_in: 47.3,
  cruise_altitude_ft: null,
};

function formatWind(wind: PlanningWind | null): string {
  if (!wind) return "—";
  return `${wind.direction_true_deg.toFixed(0)}°/${wind.speed_kt.toFixed(0)}`;
}

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
 * for exactly this reason.
 *
 * Route state and its expansion into flat points are lifted up to
 * App.tsx so MapView can draw the planned route too — see
 * `expandRoute.ts` for the FOO V123 BAR airway semantics and
 * `procedureLookup.ts` for how a chosen SID/STAR resolves. */
export function FlightPlanning({
  route,
  onRouteChange,
  points,
  warnings,
  airspaceCrossings,
}: {
  route: RouteState;
  onRouteChange: (route: RouteState) => void;
  points: RouteWaypoint[];
  warnings: string[];
  airspaceCrossings: AirspaceVolume[];
}) {
  const [profile, setProfile] = useState<AircraftProfile>(DEFAULT_PROFILE);
  const [navLog, setNavLog] = useState<RoutePlanSummary | null>(null);
  const [navLogError, setNavLogError] = useState<string | null>(null);
  const [legWinds, setLegWinds] = useState<(PlanningWind | null)[]>([]);
  const [windsBulletin, setWindsBulletin] = useState<WindsAloftBulletin | null>(null);
  // Gates whether airspaceCrossings gets shown, not whether it's computed
  // (App.tsx always computes it) — an IFR flight is already on an ATC
  // clearance through controlled airspace, so a "you're entering Class B"
  // heads-up isn't the same kind of actionable warning it is for VFR.
  // Defaults to VFR since this is a GA-first tool.
  const [flightRules, setFlightRules] = useState<"VFR" | "IFR">("VFR");

  // Fetched once — same "low" (3,000-39,000ft) product MapView already
  // uses for its own overlay, now also the source for nav-log wind
  // correction (see windsAloft.ts). A fetch failure just means no wind
  // correction is available yet; it doesn't block the rest of planning.
  useEffect(() => {
    fetchWindsAloft("low").then(setWindsBulletin).catch(() => setWindsBulletin(null));
  }, []);

  useEffect(() => {
    if (points.length < 2) {
      setNavLog(null);
      setNavLogError(null);
      setLegWinds([]);
      return;
    }
    let cancelled = false;
    (async () => {
      const winds =
        profile.cruise_altitude_ft !== null && windsBulletin
          ? await windsForRoute(points, profile.cruise_altitude_ft, windsBulletin)
          : points.map(() => null).slice(1);
      if (cancelled) return;
      setLegWinds(winds);
      try {
        const summary = await planRoute(
          points.map((p) => ({ lat: p.lat, lon: p.lon })),
          profile,
          winds,
        );
        if (!cancelled) {
          setNavLog(summary);
          setNavLogError(null);
        }
      } catch (err: unknown) {
        if (!cancelled) setNavLogError(err instanceof Error ? err.message : String(err));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [points, profile, windsBulletin]);

  const hasWbEnvelope =
    profile.max_gross_weight_lb !== null &&
    profile.forward_cg_limit_in !== null &&
    profile.aft_cg_limit_in !== null;

  return (
    <div className="planning-layout">
      <AircraftProfileForm profile={profile} onChange={setProfile} />
      <div className="flight-rules-toggle">
        <button className={flightRules === "VFR" ? "selected" : ""} onClick={() => setFlightRules("VFR")}>
          VFR
        </button>
        <button className={flightRules === "IFR" ? "selected" : ""} onClick={() => setFlightRules("IFR")}>
          IFR
        </button>
      </div>
      <RouteBuilder route={route} onChange={onRouteChange} warnings={warnings} />
      <div className="panel nav-log">
        <h2>Nav Log</h2>
        {points.length < 2 && <p className="hint">Add at least two points to the route to see a nav log.</p>}
        {navLogError && <p className="hint">Couldn't compute nav log: {navLogError}</p>}
        {/* navLog is computed async (planRoute() in the effect above) while
            `points` updates synchronously the instant the route changes —
            e.g. removing an airway shrinks `points` immediately, but the
            old, longer navLog can still be around for one render. Guard
            against indexing `points` with a stale navLog's leg count
            rather than relying on the effect always winning the race. */}
        {navLog && navLog.legs.length === points.length - 1 && (
          <>
            <table>
              <thead>
                <tr>
                  <th>Leg</th>
                  <th>Dist (nm)</th>
                  <th>Course</th>
                  <th>Wind</th>
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
                      {points[i].ident} → {points[i + 1].ident}
                    </td>
                    <td>{leg.distance_nm.toFixed(1)}</td>
                    <td>{leg.true_course_deg.toFixed(0)}°</td>
                    <td>{formatWind(legWinds[i] ?? null)}</td>
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
            {profile.cruise_altitude_ft === null ? (
              <p className="hint">No wind correction — headings equal course, groundspeed equals TAS (DESIGN.md §9.3).</p>
            ) : (
              <p className="hint">
                Wind correction from the nearest winds-aloft station/altitude to each leg — a rough estimate, not a
                certified forecast tool (DESIGN.md §9.3).
              </p>
            )}
          </>
        )}
        {flightRules === "VFR" &&
          airspaceCrossings.map((volume) => (
            <p key={volume.id} className="hint route-warning">
              ⚠ {volume.class === "B" || volume.class === "C" || volume.class === "D" ? `Class ${volume.class}` : volume.class}
              : {volume.name} ({volume.floor}–{volume.ceiling})
            </p>
          ))}
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
      <label>
        Cruise altitude (ft)
        <input type="number" {...numberField("cruise_altitude_ft")} />
      </label>
      <p className="hint">Set an altitude to correct the nav log for real winds aloft (nearest station/level).</p>
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

const SEARCH_KIND_BADGES: Record<IdentSearchRow["kind"], string> = {
  airport: "APT",
  waypoint: "FIX",
  navaid: "NAV",
  airway: "AWY",
};

function tokenIdent(t: RouteToken): string {
  return t.kind === "point" ? t.point.ident : t.ident;
}

/** Departure/arrival airport picker: a debounced airport-only search
 * (same pattern as the map view's own airport search) that shows the
 * chosen airport with a clear button once set, matching the "choose
 * departure/arrival first" flow the SID/STAR pickers below depend on. */
function AirportSlot({
  label,
  value,
  onChange,
}: {
  label: string;
  value: RouteWaypoint | null;
  onChange: (value: RouteWaypoint | null) => void;
}) {
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

  if (value) {
    return (
      <div className="route-airport-slot">
        <span>
          <strong>{label}:</strong> {value.ident} {value.name && <span className="airport-name">{value.name}</span>}
        </span>
        <button className="clear-button" onClick={() => onChange(null)} aria-label={`Clear ${label}`}>
          ×
        </button>
      </div>
    );
  }

  return (
    <div className="route-airport-slot">
      <label>{label}</label>
      <input
        className="airport-search"
        type="search"
        placeholder={`Search ${label.toLowerCase()} airport…`}
        value={query}
        onChange={(e) => setQuery(e.target.value)}
      />
      {error && <p className="hint">search failed: {error}</p>}
      {results.length > 0 && (
        <ul>
          {results.map((a) => (
            <li key={a.icao}>
              <button
                onClick={() => {
                  onChange({ ident: a.icao, name: a.name, lat: a.lat, lon: a.lon });
                  setQuery("");
                  setResults([]);
                }}
              >
                <strong>{a.icao}</strong> — <span className="airport-name">{a.name}</span>
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

type ProcedurePickerState =
  | { step: "procedure"; options: Procedure[] }
  | { step: "transition"; detail: ProcedureDetail; options: ProcedureTransitionDetail[] };

/** "Set SID"/"Set STAR": browses the given airport's real procedures
 * (fetched live, not typed) — pick a procedure, then a transition (or
 * skip straight to resolving if it only has one, like most SIDs — see
 * procedureLookup.ts). Disabled until `airport` is chosen. */
function ProcedurePickerButton({
  label,
  kind,
  airport,
  resolved,
  onResolve,
}: {
  label: string;
  kind: "SID" | "STAR";
  airport: RouteWaypoint | null;
  resolved: ResolvedProcedureRef | null;
  onResolve: (resolved: ResolvedProcedureRef | null) => void;
}) {
  const [picker, setPicker] = useState<ProcedurePickerState | null>(null);
  const [error, setError] = useState<string | null>(null);

  const open = () => {
    if (!airport) return;
    setError(null);
    fetchProcedureOptions(airport.ident, kind)
      .then((options) => setPicker({ step: "procedure", options }))
      .catch((err: unknown) => setError(err instanceof Error ? err.message : String(err)));
  };

  const resolveWith = (detail: ProcedureDetail, transitionId: string) => {
    if (!airport) return;
    const built = buildResolvedProcedure(airport.ident, kind, detail, transitionId);
    onResolve(built);
    setPicker(null);
  };

  const chooseProcedure = (procedure: Procedure) => {
    fetchProcedureDetail(procedure.id)
      .then((detail) => {
        const options = transitionOptions(detail);
        if (options.length === 1) {
          resolveWith(detail, options[0].id);
        } else {
          setPicker({ step: "transition", detail, options });
        }
      })
      .catch((err: unknown) => setError(err instanceof Error ? err.message : String(err)));
  };

  return (
    <div className="procedure-picker">
      <span className="procedure-picker-header">
        <button onClick={open} disabled={!airport}>
          {resolved ? `${label}: ${resolved.procedureIdent}.${resolved.transitionIdent}` : `Set ${label}`}
        </button>
        {resolved && (
          <button className="clear-button" onClick={() => onResolve(null)} aria-label={`Clear ${label}`}>
            ×
          </button>
        )}
      </span>
      {error && <p className="hint">{error}</p>}
      {picker?.step === "procedure" && (
        <ul>
          {picker.options.length === 0 && <li className="hint">no {kind}s in this cycle</li>}
          {picker.options.map((p) => (
            <li key={p.id}>
              <button onClick={() => chooseProcedure(p)}>
                <strong>{p.ident}</strong>
                {p.runway_ident && <span className="airport-name"> rwy {p.runway_ident}</span>}
              </button>
            </li>
          ))}
        </ul>
      )}
      {picker?.step === "transition" && (
        <ul>
          {picker.options.map((t) => (
            <li key={t.id}>
              <button onClick={() => resolveWith(picker.detail, t.id)}>
                <strong>{t.ident}</strong>
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

/** The route builder: departure/arrival airports and an optional SID/
 * STAR are chosen explicitly (dedicated fields/buttons, not typed),
 * then fixes/navaids/airways in between via one unified search box —
 * routes read flight-plan-string style, e.g. KSFO FOO V123 BAR KLAX.
 * Airway tokens expand between their neighbor fixes (see
 * expandRoute.ts), and `warnings` reports ones that can't expand yet. */
function RouteBuilder({
  route,
  onChange,
  warnings,
}: {
  route: RouteState;
  onChange: (route: RouteState) => void;
  warnings: string[];
}) {
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<IdentSearchRow[]>([]);
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
      searchIdents(q)
        .then((rows) => {
          if (!cancelled) {
            setResults(rows);
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

  const { middleTokens } = route;
  const setMiddleTokens = (tokens: RouteToken[]) => onChange({ ...route, middleTokens: tokens });

  const addResult = (row: IdentSearchRow) => {
    setQuery("");
    setResults([]);
    if (row.kind === "airway") {
      fetchAirwayDetail(row.ident)
        .then((detail) => setMiddleTokens([...middleTokens, { kind: "airway", ident: detail.ident, detail }]))
        .catch((err: unknown) => setError(err instanceof Error ? err.message : String(err)));
      return;
    }
    if (row.lat === null || row.lon === null) return;
    setMiddleTokens([
      ...middleTokens,
      { kind: "point", point: { ident: row.ident, name: row.name, lat: row.lat, lon: row.lon } },
    ]);
  };
  const removeAt = (i: number) => setMiddleTokens(middleTokens.filter((_, idx) => idx !== i));
  const moveUp = (i: number) => {
    if (i === 0) return;
    const next = [...middleTokens];
    [next[i - 1], next[i]] = [next[i], next[i - 1]];
    setMiddleTokens(next);
  };
  const moveDown = (i: number) => {
    if (i === middleTokens.length - 1) return;
    const next = [...middleTokens];
    [next[i], next[i + 1]] = [next[i + 1], next[i]];
    setMiddleTokens(next);
  };

  return (
    <div className="panel route-builder">
      <h2>Route</h2>
      <AirportSlot
        label="Departure"
        value={route.departure}
        // Changing departure invalidates any SID resolved against the old one.
        onChange={(departure) => onChange({ ...route, departure, sid: null })}
      />
      <AirportSlot
        label="Arrival"
        value={route.arrival}
        onChange={(arrival) => onChange({ ...route, arrival, star: null })}
      />
      <div className="procedure-buttons">
        <ProcedurePickerButton
          label="SID"
          kind="SID"
          airport={route.departure}
          resolved={route.sid}
          onResolve={(sid) => onChange({ ...route, sid })}
        />
        <ProcedurePickerButton
          label="STAR"
          kind="STAR"
          airport={route.arrival}
          resolved={route.star}
          onResolve={(star) => onChange({ ...route, star })}
        />
      </div>

      <h3>Fixes / Airways</h3>
      <input
        className="airport-search"
        type="search"
        placeholder="Add a fix, navaid, or airway…"
        value={query}
        onChange={(e) => setQuery(e.target.value)}
      />
      {error && <p className="hint">search failed: {error}</p>}
      {results.length > 0 && (
        <ul>
          {results.map((r) => (
            <li key={`${r.kind}-${r.ident}`}>
              <button onClick={() => addResult(r)}>
                <span className="kind-badge">{SEARCH_KIND_BADGES[r.kind]}</span> <strong>{r.ident}</strong>
                {r.name && (
                  <>
                    {" "}
                    — <span className="airport-name">{r.name}</span>
                  </>
                )}
              </button>
            </li>
          ))}
        </ul>
      )}
      {middleTokens.length === 0 && (
        <p className="hint">
          No fixes yet — search above to add one. Insert an airway between two of its fixes (e.g. FIX1, V123, FIX2)
          to route along it.
        </p>
      )}
      <ol className="route-list">
        {middleTokens.map((t, i) => (
          <li key={`${tokenIdent(t)}-${i}`}>
            <span>
              <strong>{tokenIdent(t)}</strong>{" "}
              {t.kind === "point" && t.point.name && <span className="airport-name">{t.point.name}</span>}
              {t.kind === "airway" && <span className="airport-name">airway · {t.detail.legs.length} fixes</span>}
            </span>
            <span className="route-list-actions">
              <button onClick={() => moveUp(i)} disabled={i === 0} aria-label={`Move ${tokenIdent(t)} up`}>
                ↑
              </button>
              <button
                onClick={() => moveDown(i)}
                disabled={i === middleTokens.length - 1}
                aria-label={`Move ${tokenIdent(t)} down`}
              >
                ↓
              </button>
              <button onClick={() => removeAt(i)} aria-label={`Remove ${tokenIdent(t)}`}>
                ×
              </button>
            </span>
          </li>
        ))}
      </ol>
      {warnings.map((w) => (
        <p key={w} className="hint route-warning">
          ⚠ {w}
        </p>
      ))}
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
