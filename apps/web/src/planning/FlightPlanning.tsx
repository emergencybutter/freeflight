import { useEffect, useMemo, useState } from "react";
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
import type { Aircraft, AircraftDetail } from "../aircraft";
import { aircraftToProfile, cruisePowerSettings } from "./fromAircraft";
import {
  checkWeightBalance,
  planFlight,
  type AircraftProfile,
  type FlightPlanSummary,
  type FuelSummary,
  type PlanningWind,
  type ValueSource,
  type VerticalProfile,
  type WeightAtStation,
  type WeightBalanceResult,
} from "./wasm";

/** A default profile shaped like a real Cessna 172 (same numbers used in
 * ff-planning's own tests) so the nav log/W&B sections have something
 * sensible to show before the user has typed anything in. No cruise
 * altitude by default — wind correction only kicks in once one's set,
 * and so does the vertical profile. Climb/descent performance is filled
 * in (a 172 climbs ~700 fpm at Vy and comes down comfortably at 500 fpm)
 * so that setting a cruise altitude is the only thing standing between a
 * fresh session and a top of climb/descent.
 * Exported since App.tsx now owns this state (lifted so MapView can
 * default its own winds-aloft altitude selector to it — see
 * App.tsx/MapView.tsx). */
export const DEFAULT_PROFILE: AircraftProfile = {
  name: "Cessna 172",
  cruise_tas_kt: 110,
  fuel_burn_gph: 8.5,
  max_gross_weight_lb: 2450,
  forward_cg_limit_in: 35,
  aft_cg_limit_in: 47.3,
  cruise_altitude_ft: null,
  climb_rate_fpm: 700,
  climb_tas_kt: 75,
  descent_rate_fpm: 500,
  descent_tas_kt: 110,
};

function formatWind(wind: PlanningWind | null): string {
  if (!wind) return "—";
  return `${wind.direction_true_deg.toFixed(0)}°/${wind.speed_kt.toFixed(0)}`;
}

/** Magnetic variation as e.g. "13°E" / "8°W" (positive = East). */
function formatVar(deg: number): string {
  const r = Math.round(Math.abs(deg));
  if (r === 0) return "0°";
  return `${r}°${deg >= 0 ? "E" : "W"}`;
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
 * Route state and its expansion into flat points, and the aircraft
 * profile, are lifted up to App.tsx so MapView can draw the planned
 * route and default its winds-aloft altitude selector to the same
 * cruise altitude — see `expandRoute.ts` for the FOO V123 BAR airway
 * semantics and `procedureLookup.ts` for how a chosen SID/STAR
 * resolves. */
export function FlightPlanning({
  route,
  onRouteChange,
  points,
  warnings,
  airspaceCrossings,
  profile,
  onProfileChange,
  onVerticalProfileChange,
  fleet,
  selectedAircraft,
  onSelectAircraft,
}: {
  route: RouteState;
  onRouteChange: (route: RouteState) => void;
  points: RouteWaypoint[];
  warnings: string[];
  airspaceCrossings: AirspaceVolume[];
  profile: AircraftProfile;
  onProfileChange: (profile: AircraftProfile) => void;
  /** Reports the computed top of climb/descent back up to App.tsx so
   * MapView can mark them on the route — the same lift `route` and
   * `profile` already do, in the other direction. */
  onVerticalProfileChange?: (vertical: VerticalProfile | null) => void;
  /** The signed-in pilot's fleet (empty when signed out), the one
   * currently planned with, and how to change it. Aircraft live on the
   * server (§9.5.2); `selectedAircraft` null means "this session only",
   * which is the pre-accounts behaviour and still the signed-out path. */
  fleet: Aircraft[];
  selectedAircraft: AircraftDetail | null;
  onSelectAircraft: (id: number | null) => void;
}) {
  const [plan, setPlan] = useState<FlightPlanSummary | null>(null);
  const [navLogError, setNavLogError] = useState<string | null>(null);
  const [legWinds, setLegWinds] = useState<(PlanningWind | null)[]>([]);
  // Which cruise power setting to plan at. A property of the *flight*,
  // not the aeroplane — the same aircraft is flown at different settings
  // on different days — so it lives here rather than on the record.
  const [powerSetting, setPowerSetting] = useState<string | null>(null);
  const [windsBulletin, setWindsBulletin] = useState<WindsAloftBulletin | null>(null);
  // Gates whether airspaceCrossings gets shown, not whether it's computed
  // (App.tsx always computes it) — an IFR flight is already on an ATC
  // clearance through controlled airspace, so a "you're entering Class B"
  // heads-up isn't the same kind of actionable warning it is for VFR.
  // Also gates the SID/STAR pickers in RouteBuilder.
  //
  // Defaults to VFR since this is a GA-first tool, but seeds from the
  // route: a plan carrying a SID or STAR is an instrument plan by
  // definition, and this state is not persisted while the route is
  // (localStorage, and shared links). Without the seed, refreshing an
  // IFR plan would drop it to VFR and hide the pickers while the
  // procedure kept feeding routePoints in App.tsx.
  const [flightRules, setFlightRules] = useState<"VFR" | "IFR">(
    route.sid || route.star ? "IFR" : "VFR",
  );

  // The seed above only catches a route present on first render; a
  // shared link resolves its procedures asynchronously (share.ts) and
  // lands after. Re-asserting IFR here cannot fight the pilot, because
  // VFR hides the only controls that can set a procedure — so sid/star
  // appearing while VFR always means a load, never a choice.
  useEffect(() => {
    if (route.sid || route.star) setFlightRules("IFR");
  }, [route.sid, route.star]);

  // Fetched once — same "low" (3,000-39,000ft) product MapView already
  // uses for its own overlay, now also the source for nav-log wind
  // correction (see windsAloft.ts). A fetch failure just means no wind
  // correction is available yet; it doesn't block the rest of planning.
  useEffect(() => {
    fetchWindsAloft("low").then(setWindsBulletin).catch(() => setWindsBulletin(null));
  }, []);

  // Real field elevations for the two ends, when the airports came from
  // a search that carried them (see RouteWaypoint in types.ts). The
  // vertical profile climbs from / descends to these rather than
  // assuming sea level, and simply omits an end it doesn't know.
  const departureElevationFt = route.departure?.elevation_ft ?? null;
  const arrivalElevationFt = route.arrival?.elevation_ft ?? null;

  // What the planner actually flies. A selected aircraft supplies the
  // performance; the cruise altitude and power setting stay with the
  // plan, since they change flight to flight (see fromAircraft.ts).
  const effectiveProfile = useMemo<AircraftProfile>(
    () =>
      selectedAircraft
        ? aircraftToProfile(selectedAircraft, profile, profile.cruise_altitude_ft, powerSetting)
        : profile,
    [selectedAircraft, profile, powerSetting],
  );

  // Reset the power choice when the aircraft changes, and pre-select it
  // when there is only one — asking the pilot to choose between one
  // option is noise.
  useEffect(() => {
    const settings = selectedAircraft ? cruisePowerSettings(selectedAircraft) : [];
    setPowerSetting(settings.length === 1 ? settings[0] : null);
  }, [selectedAircraft]);

  useEffect(() => {
    if (points.length < 2) {
      setPlan(null);
      setNavLogError(null);
      onVerticalProfileChange?.(null);
      setLegWinds([]);
      return;
    }
    let cancelled = false;
    (async () => {
      const winds =
        effectiveProfile.cruise_altitude_ft !== null && windsBulletin
          ? await windsForRoute(points, effectiveProfile.cruise_altitude_ft, windsBulletin)
          : points.map(() => null).slice(1);
      if (cancelled) return;
      setLegWinds(winds);
      const coordinates = points.map((p) => ({ lat: p.lat, lon: p.lon }));
      try {
        // One pass: the nav log, the vertical profile and the fuel total
        // have to agree with each other, and fuel is only phase-aware if
        // the same call knows the climb/descent times (§9.5.6).
        const summary = await planFlight(
          coordinates,
          effectiveProfile,
          winds,
          departureElevationFt,
          arrivalElevationFt,
        );
        if (!cancelled) {
          setPlan(summary);
          setNavLogError(null);
          onVerticalProfileChange?.(summary.vertical);
        }
      } catch (err: unknown) {
        if (!cancelled) setNavLogError(err instanceof Error ? err.message : String(err));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [
    points,
    effectiveProfile,
    windsBulletin,
    departureElevationFt,
    arrivalElevationFt,
    onVerticalProfileChange,
  ]);

  // Read from `effectiveProfile`, not `profile`: with an aircraft
  // selected the planner flies that aircraft's envelope
  // (fromAircraft.ts takes W&B straight off the record), so gating and
  // checking against the *session* envelope asked the wrong question —
  // it could hide the panel for an aircraft that has limits, or worse,
  // pass a loading against the session's limits while the plan flew the
  // aircraft's.
  const hasWbEnvelope =
    effectiveProfile.max_gross_weight_lb !== null &&
    effectiveProfile.forward_cg_limit_in !== null &&
    effectiveProfile.aft_cg_limit_in !== null;

  return (
    <div className="planning-layout">
      <div className="flight-rules-toggle">
        <button
          className={flightRules === "VFR" ? "selected" : ""}
          /* Dropping to VFR clears any procedure already resolved. It is
             what lets the pickers be hidden rather than disabled: a SID
             left set here would go on contributing its points to the
             route, the map line and the chart links with no control on
             screen to show it, and the pilot would be flying a plan they
             could no longer see the shape of. */
          onClick={() => {
            setFlightRules("VFR");
            if (route.sid || route.star) onRouteChange({ ...route, sid: null, star: null });
          }}
        >
          VFR
        </button>
        <button className={flightRules === "IFR" ? "selected" : ""} onClick={() => setFlightRules("IFR")}>
          IFR
        </button>
      </div>
      <RouteBuilder
        route={route}
        onChange={onRouteChange}
        warnings={warnings}
        flightRules={flightRules}
      />
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
        {plan && plan.legs.length === points.length - 1 && (
          <>
            <table>
              <thead>
                <tr>
                  <th>Leg</th>
                  <th>Dist (nm)</th>
                  <th title="Magnetic course (true course − variation)">MC</th>
                  <th>Wind</th>
                  <th title="Magnetic variation (WMM 2025)">Var</th>
                  <th title="Magnetic heading — steer this">MH</th>
                  <th>GS (kt)</th>
                  <th>ETE</th>
                  <th>Fuel (gal)</th>
                </tr>
              </thead>
              <tbody>
                {plan.legs.map((leg, i) => (
                  <tr key={i}>
                    <td>
                      {points[i].ident} → {points[i + 1].ident}
                    </td>
                    <td>{leg.distance_nm.toFixed(1)}</td>
                    <td>{leg.magnetic_course_deg.toFixed(0)}°</td>
                    <td>{formatWind(legWinds[i] ?? null)}</td>
                    <td>{formatVar(leg.magnetic_variation_deg)}</td>
                    <td>
                      <strong>{leg.magnetic_heading_deg.toFixed(0)}°</strong>
                    </td>
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
                    <strong>{plan.total_distance_nm.toFixed(1)}</strong>
                  </td>
                  {/* MC, Wind, Var, MH, GS — no meaningful total */}
                  <td />
                  <td />
                  <td />
                  <td />
                  <td />
                  <td>
                    <strong>{formatHours(plan.total_ete_hours)}</strong>
                  </td>
                  <td>
                    <strong>{plan.fuel.trip_gal.toFixed(1)}</strong>
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
      {plan && points.length >= 2 && (
        <FuelPanel
          fuel={plan.fuel}
          cruiseFuelSource={plan.cruise_fuel_source}
          aircraftName={selectedAircraft?.registration ?? null}
          unverified={selectedAircraft !== null && selectedAircraft.verified_at === null}
        />
      )}
      {points.length >= 2 && (
        <VerticalProfilePanel
          vertical={plan?.vertical ?? null}
          points={points}
          profile={effectiveProfile}
          hasDepartureElevation={departureElevationFt !== null}
          hasArrivalElevation={arrivalElevationFt !== null}
        />
      )}
      <AircraftProfileForm
        profile={profile}
        onChange={onProfileChange}
        fleet={fleet}
        selectedAircraft={selectedAircraft}
        onSelectAircraft={onSelectAircraft}
        powerSetting={powerSetting}
        onPowerSettingChange={setPowerSetting}
        cruiseTasKt={plan?.cruise_tas_kt ?? null}
        cruiseTasSource={plan?.cruise_tas_source ?? null}
      />
      {hasWbEnvelope && (
        <WeightBalancePanel
          envelope={{
            max_gross_weight_lb: effectiveProfile.max_gross_weight_lb as number,
            forward_cg_limit_in: effectiveProfile.forward_cg_limit_in as number,
            aft_cg_limit_in: effectiveProfile.aft_cg_limit_in as number,
          }}
        />
      )}
    </div>
  );
}

/** Fuel required, decomposed by phase (DESIGN.md §9.5.6).
 *
 * The old nav-log total was cruise burn × total time, which under-counts
 * a climb and over-counts a descent. This shows the four phases so the
 * number is auditable rather than a single figure to trust — and flags
 * when it had to fall back to the old whole-flight-at-cruise-burn model. */
function FuelPanel({
  fuel,
  cruiseFuelSource,
  aircraftName,
  unverified,
}: {
  fuel: FuelSummary;
  cruiseFuelSource: ValueSource;
  aircraftName: string | null;
  unverified: boolean;
}) {
  const overCapacity = fuel.within_capacity === false;
  return (
    <div className="panel fuel-summary">
      <h2>Fuel</h2>
      {unverified && aircraftName && (
        <p className="hint route-warning">
          ⚠ {aircraftName}'s figures are unverified — book numbers, not this aeroplane. Check them
          against the POH before relying on this.
        </p>
      )}
      <table>
        <tbody>
          <tr>
            <td>Taxi</td>
            <td>{fuel.taxi_gal.toFixed(1)}</td>
            <td className="hint">allowance</td>
          </tr>
          <tr>
            <td>Climb</td>
            <td>{fuel.climb_gal.toFixed(1)}</td>
            <td className="hint">{formatMinutes(fuel.climb_minutes)}</td>
          </tr>
          <tr>
            <td>Cruise</td>
            <td>{fuel.cruise_gal.toFixed(1)}</td>
            <td className="hint">{formatHours(fuel.cruise_hours)}</td>
          </tr>
          <tr>
            <td>Descent</td>
            <td>{fuel.descent_gal.toFixed(1)}</td>
            <td className="hint">{formatMinutes(fuel.descent_minutes)}</td>
          </tr>
          <tr>
            <td>
              <strong>Trip</strong>
            </td>
            <td>
              <strong>{fuel.trip_gal.toFixed(1)}</strong>
            </td>
            <td />
          </tr>
          <tr>
            <td>Reserve</td>
            <td>{fuel.reserve_gal.toFixed(1)}</td>
            <td className="hint">at cruise burn</td>
          </tr>
          <tr>
            <td>
              <strong>Required</strong>
            </td>
            <td>
              <strong>{fuel.required_gal.toFixed(1)} gal</strong>
            </td>
            <td className="hint">
              {fuel.capacity_gal !== null ? `of ${fuel.capacity_gal.toFixed(0)} usable` : "no capacity set"}
            </td>
          </tr>
        </tbody>
      </table>
      {overCapacity && (
        <p className="wb-fail">
          ⚠ Needs {fuel.required_gal.toFixed(1)} gal but the tanks hold {fuel.capacity_gal?.toFixed(0)} —
          this flight does not fit without a stop.
        </p>
      )}
      {!fuel.phase_aware && (
        <p className="hint">
          Whole flight charged at cruise burn — set a cruise altitude and both field elevations to split
          it by phase.
        </p>
      )}
      <p className="hint">
        Burn is{" "}
        {cruiseFuelSource === "table"
          ? "interpolated from the aircraft's cruise table"
          : "the profile's single cruise figure"}
        . Phase times come from the climb/descent profile, so they won't match the nav log's ETE exactly
        — leg times are still flown at cruise TAS (DESIGN.md §9.5.6).
      </p>
    </div>
  );
}

function formatMinutes(minutes: number): string {
  if (!Number.isFinite(minutes)) return "—";
  return `${Math.round(minutes)} min`;
}

function formatAltitude(ft: number): string {
  return `${Math.round(ft).toLocaleString("en-US")} ft`;
}

/** Top of climb / top of descent (DESIGN.md §9.3): where the climb levels
 * off and where to start down, computed against the real departure/
 * arrival field elevations and the same per-leg winds as the nav log
 * (`planVertical` in ./wasm.ts). Each is reported as a distance, a time,
 * *and* a position — MapView draws the same two points on the route.
 *
 * On a short hop the climb and descent overlap and the cruise altitude is
 * never reached; the two points then collapse onto the single crossover
 * and the panel says so rather than showing a top of descent that comes
 * before the top of climb. */
function VerticalProfilePanel({
  vertical,
  points,
  profile,
  hasDepartureElevation,
  hasArrivalElevation,
}: {
  vertical: VerticalProfile | null;
  points: RouteWaypoint[];
  profile: AircraftProfile;
  hasDepartureElevation: boolean;
  hasArrivalElevation: boolean;
}) {
  const legLabel = (legIndex: number): string | null => {
    const from = points[legIndex];
    const to = points[legIndex + 1];
    return from && to ? `${from.ident} → ${to.ident}` : null;
  };
  const departureIdent = points[0]?.ident ?? "departure";
  const arrivalIdent = points[points.length - 1]?.ident ?? "arrival";

  const missing: string[] = [];
  if (profile.cruise_altitude_ft === null) missing.push("a cruise altitude");
  if (profile.climb_rate_fpm === null && profile.descent_rate_fpm === null) {
    missing.push("a climb or descent rate");
  }
  if (!hasDepartureElevation && !hasArrivalElevation) {
    missing.push("a departure or arrival airport (for field elevation)");
  }

  return (
    <div className="panel vertical-profile">
      <h2>Top of Climb / Descent</h2>
      {!vertical && (
        <p className="hint">
          {missing.length > 0
            ? `Needs ${missing.join(", ")} — set them in Aircraft below.`
            : "Not available for this route."}
        </p>
      )}
      {vertical && !vertical.cruise_reached && (
        <p className="hint route-warning">
          ⚠ {formatAltitude(vertical.cruise_altitude_ft)} isn't reachable in{" "}
          {vertical.total_distance_nm.toFixed(1)} nm — the climb and descent meet at{" "}
          {formatAltitude(vertical.peak_altitude_ft)}. Plan a lower cruise altitude.
        </p>
      )}
      {vertical && (
        <dl className="vertical-points">
          {vertical.top_of_climb && (
            <>
              <dt>Top of climb</dt>
              <dd>
                <strong>{vertical.top_of_climb.distance_from_departure_nm.toFixed(1)} nm</strong> from{" "}
                {departureIdent} · {formatAltitude(vertical.top_of_climb.altitude_ft)} ·{" "}
                {formatMinutes(vertical.top_of_climb.time_min)} after takeoff
                <span className="hint">
                  {legLabel(vertical.top_of_climb.leg_index)} · {vertical.top_of_climb.lat.toFixed(4)},{" "}
                  {vertical.top_of_climb.lon.toFixed(4)}
                </span>
              </dd>
            </>
          )}
          {vertical.top_of_descent && (
            <>
              <dt>Top of descent</dt>
              <dd>
                <strong>{vertical.top_of_descent.distance_to_arrival_nm.toFixed(1)} nm</strong> before{" "}
                {arrivalIdent} · start down {formatMinutes(vertical.top_of_descent.time_min)} out ·{" "}
                {vertical.top_of_descent.distance_from_departure_nm.toFixed(1)} nm along the route
                <span className="hint">
                  {legLabel(vertical.top_of_descent.leg_index)} · {vertical.top_of_descent.lat.toFixed(4)},{" "}
                  {vertical.top_of_descent.lon.toFixed(4)}
                </span>
              </dd>
            </>
          )}
          {vertical.cruise_reached && (
            <>
              <dt>Level cruise</dt>
              <dd>
                {vertical.cruise_distance_nm.toFixed(1)} nm at {formatAltitude(vertical.cruise_altitude_ft)}
              </dd>
            </>
          )}
        </dl>
      )}
      {vertical && (!vertical.top_of_climb || !vertical.top_of_descent) && (
        <p className="hint">
          {vertical.top_of_climb
            ? "No top of descent — set an arrival airport and a descent rate."
            : "No top of climb — set a departure airport and a climb rate."}
        </p>
      )}
      <p className="hint">
        Constant-rate climb and descent at the profile's rates, wind-corrected per leg — a planning estimate,
        not a performance chart. The nav log's ETE and fuel still assume cruise TAS for the whole route
        (DESIGN.md §9.3).
      </p>
    </div>
  );
}

function AircraftProfileForm({
  profile,
  onChange,
  fleet,
  selectedAircraft,
  onSelectAircraft,
  powerSetting,
  onPowerSettingChange,
  cruiseTasKt,
  cruiseTasSource,
}: {
  profile: AircraftProfile;
  onChange: (profile: AircraftProfile) => void;
  fleet: Aircraft[];
  selectedAircraft: AircraftDetail | null;
  onSelectAircraft: (id: number | null) => void;
  powerSetting: string | null;
  onPowerSettingChange: (setting: string | null) => void;
  cruiseTasKt: number | null;
  cruiseTasSource: ValueSource | null;
}) {
  const numberField = (key: keyof AircraftProfile) => ({
    value: profile[key] === null || profile[key] === undefined ? "" : String(profile[key]),
    onChange: (e: React.ChangeEvent<HTMLInputElement>) => {
      const raw = e.target.value;
      onChange({ ...profile, [key]: raw === "" ? (key === "name" ? "" : null) : Number(raw) });
    },
  });
  const powerOptions = selectedAircraft ? cruisePowerSettings(selectedAircraft) : [];

  /* With an aircraft selected the plan is built from the record, so most
     of the session fields below do nothing and are hidden rather than
     left on screen with a caption explaining that they are inert.

     "Unused" is per-field, not wholesale, which is why this is two flags
     and not one: `aircraftToProfile` falls back to the session cruise
     TAS and fuel burn when the aircraft itself leaves them blank
     (fromAircraft.ts), so in exactly that case those two are live inputs
     to the plan and have to stay editable. Everything else — name,
     climb/descent, W&B envelope — comes off the record unconditionally. */
  const usesSessionCruiseTas = selectedAircraft !== null && selectedAircraft.cruise_tas_kt === null;
  const usesSessionFuelBurn = selectedAircraft !== null && selectedAircraft.cruise_fuel_gph === null;
  const borrowed = [
    usesSessionCruiseTas ? "a cruise TAS" : null,
    usesSessionFuelBurn ? "a fuel burn" : null,
  ].filter((v): v is string => v !== null);

  return (
    <div className="panel aircraft-profile">
      <h2>Aircraft</h2>
      {fleet.length > 0 && (
        <label>
          Plan with
          <select
            value={selectedAircraft?.id ?? ""}
            onChange={(e) => onSelectAircraft(e.target.value === "" ? null : Number(e.target.value))}
          >
            <option value="">This session only</option>
            {fleet.map((aircraft) => (
              <option key={aircraft.id} value={aircraft.id}>
                {aircraft.registration}
                {aircraft.icao_type ? ` (${aircraft.icao_type})` : ""}
                {aircraft.verified_at === null ? " — unverified" : ""}
              </option>
            ))}
          </select>
        </label>
      )}

      {/* Cruise altitude belongs to the flight, not the aeroplane, so it
          stays editable whichever mode we're in. */}
      <label>
        Cruise altitude (ft)
        <input type="number" {...numberField("cruise_altitude_ft")} />
      </label>
      {powerOptions.length > 1 && (
        <label>
          Cruise power
          <select
            value={powerSetting ?? ""}
            onChange={(e) => onPowerSettingChange(e.target.value === "" ? null : e.target.value)}
          >
            <option value="">— choose —</option>
            {powerOptions.map((setting) => (
              <option key={setting} value={setting}>
                {setting}
              </option>
            ))}
          </select>
        </label>
      )}
      {powerOptions.length > 1 && powerSetting === null && (
        <p className="hint route-warning">
          ⚠ This aircraft's cruise table records several power settings. Pick one, or the table is
          ambiguous and the fallback figures are used instead.
        </p>
      )}

      {selectedAircraft && (
        <>
          <p className="hint">
            Performance comes from <strong>{selectedAircraft.registration}</strong>
            {cruiseTasKt !== null && (
              <>
                {" "}
                — cruising {cruiseTasKt.toFixed(0)} kt{" "}
                {cruiseTasSource === "table" ? "from its performance table" : "from its saved figures"}
              </>
            )}
            . Edit it in the Aircraft view.
          </p>
          {selectedAircraft.verified_at === null && (
            <p className="hint route-warning">⚠ Unverified against its POH.</p>
          )}
        </>
      )}

      {borrowed.length > 0 && selectedAircraft && (
        <p className="hint">
          {selectedAircraft.registration} does not record {borrowed.join(" or ")}, so the{" "}
          {borrowed.length > 1 ? "session figures" : "session figure"} below still{" "}
          {borrowed.length > 1 ? "feed" : "feeds"} this plan. Set{" "}
          {borrowed.length > 1 ? "them" : "it"} on the aircraft to plan entirely from the record.
        </p>
      )}
      {selectedAircraft === null && (
        <label>
          Name
          <input
            type="text"
            value={profile.name}
            onChange={(e) => onChange({ ...profile, name: e.target.value })}
          />
        </label>
      )}
      {(selectedAircraft === null || usesSessionCruiseTas) && (
        <label>
          Cruise TAS (kt)
          <input type="number" {...numberField("cruise_tas_kt")} />
        </label>
      )}
      {(selectedAircraft === null || usesSessionFuelBurn) && (
        <label>
          Fuel burn (gal/hr)
          <input type="number" {...numberField("fuel_burn_gph")} />
        </label>
      )}
      {/* Stays whatever is selected: cruise altitude belongs to the
          flight and is still shown above. */}
      <p className="hint">
        The cruise altitude above corrects the nav log for real winds aloft (nearest station/level) and
        drives the top of climb/descent.
      </p>
      {selectedAircraft === null && (
        <>
          <h3>Climb &amp; descent</h3>
          <p className="hint">Drives the top of climb/descent above — rates from your POH, TAS as you actually fly them.</p>
          <label>
            Climb rate (ft/min)
            <input type="number" {...numberField("climb_rate_fpm")} />
          </label>
          <label>
            Climb TAS (kt)
            <input type="number" {...numberField("climb_tas_kt")} />
          </label>
          <label>
            Descent rate (ft/min)
            <input type="number" {...numberField("descent_rate_fpm")} />
          </label>
          <label>
            Descent TAS (kt)
            <input type="number" {...numberField("descent_tas_kt")} />
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
        </>
      )}
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
                  // elevation_ft rides along so the vertical profile can
                  // climb from / descend to the real field elevation.
                  onChange({
                    ident: a.icao,
                    name: a.name,
                    lat: a.lat,
                    lon: a.lon,
                    elevation_ft: a.elevation_ft,
                  });
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
  flightRules,
}: {
  route: RouteState;
  onChange: (route: RouteState) => void;
  warnings: string[];
  flightRules: "VFR" | "IFR";
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
      {/* IFR only: a SID/STAR is an instrument clearance, so offering
          them on a VFR plan invites a route the flight will not be given.
          Safe to hide rather than disable because the VFR switch clears
          any that were already set (see the toggle in FlightPlanning) —
          a hidden procedure would otherwise keep contributing its points
          to routePoints in App.tsx with nothing on screen saying so. */}
      {flightRules === "IFR" && (
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
      )}

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
