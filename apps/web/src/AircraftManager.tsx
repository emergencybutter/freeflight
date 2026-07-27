import { useCallback, useEffect, useState } from "react";
import {
  AircraftApiError,
  createAircraft,
  deleteAircraft,
  fetchAircraft,
  fetchFleet,
  fetchTemplates,
  replacePerformance,
  updateAircraft,
  type Aircraft,
  type AircraftDetail,
  type AircraftInput,
  type AircraftTemplate,
  type PerformanceRow,
  type Phase,
} from "./aircraft";

/** The aircraft manager (DESIGN.md §9.5.7): a pilot's fleet, each with
 * scalar performance and three per-phase tables out of their POH.
 *
 * An in-app view rather than a URL route. `/aircraft` is the API prefix
 * nginx proxies to ff-api, so a page there would be unreachable on a
 * browser refresh — see §9.5.8. The Map/Flight Plan toggle is component
 * state for the same reason, so this matches how the app already works.
 *
 * Requires sign-in, because aircraft are server-side and user-scoped
 * (§9.5.2). Signed out, the Flight Plan view's own inline profile keeps
 * working exactly as before — nothing here is load-bearing for planning
 * a route. */
export function AircraftManager({
  signedIn,
  onFleetChanged,
}: {
  signedIn: boolean;
  /** Fired after any create/edit/delete so the Flight Plan view picks up
   * the change without the pilot re-selecting the aircraft. */
  onFleetChanged?: () => void;
}) {
  const [fleet, setFleet] = useState<Aircraft[] | null>(null);
  const [templates, setTemplates] = useState<AircraftTemplate[]>([]);
  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);
  // Removal is deliberately two-step and its own state: the aircraft
  // being confirmed, and the one actually in flight (so the button can
  // disable and a failure can be reported next to the confirmation
  // rather than vanishing).
  const [confirmingRemoval, setConfirmingRemoval] = useState<Aircraft | null>(null);
  const [removingId, setRemovingId] = useState<number | null>(null);
  const [removeError, setRemoveError] = useState<string | null>(null);

  // Deliberately depends on nothing: this runs from an effect, so an
  // unstable identity would re-fire it on every render. It also must not
  // call `onFleetChanged` — reading the fleet is not a change, and
  // notifying from here would drive the parent's state, re-render this,
  // and loop. Mutations notify explicitly, via `reloadAndNotify`.
  const reloadFleet = useCallback(async () => {
    try {
      const list = await fetchFleet();
      setFleet(list);
      setError(null);
      return list;
    } catch (err) {
      setFleet([]);
      setError(err instanceof Error ? err.message : String(err));
      return [];
    }
  }, []);

  /** After a create/edit/delete: refresh, then tell the parent so the
   * Flight Plan view picks the change up. Only ever called from event
   * handlers, where an unstable `onFleetChanged` is harmless. */
  const reloadAndNotify = useCallback(async () => {
    const list = await reloadFleet();
    onFleetChanged?.();
    return list;
  }, [reloadFleet, onFleetChanged]);

  useEffect(() => {
    if (!signedIn) {
      setFleet(null);
      return;
    }
    void reloadFleet();
  }, [signedIn, reloadFleet]);

  /** Delete an aircraft and everything hanging off it. */
  const remove = async (aircraft: Aircraft) => {
    setRemovingId(aircraft.id);
    setRemoveError(null);
    try {
      await deleteAircraft(aircraft.id);
      // Close the editor if it was showing the aircraft just removed,
      // rather than leaving a form bound to a row that no longer exists.
      setSelectedId((current) => (current === aircraft.id ? null : current));
      setConfirmingRemoval(null);
      await reloadAndNotify();
    } catch (err) {
      setRemoveError(err instanceof AircraftApiError ? err.message : String(err));
    } finally {
      setRemovingId(null);
    }
  };

  // The catalog is public, so it loads regardless of sign-in — the "add"
  // form needs it the moment the user gets that far.
  useEffect(() => {
    fetchTemplates()
      .then(setTemplates)
      .catch(() => setTemplates([]));
  }, []);

  if (!signedIn) {
    return (
      <div className="panel aircraft-manager">
        <h2>Aircraft</h2>
        <p className="hint">
          Sign in to keep a fleet. Aircraft and their performance tables are stored on the server so they
          survive a cleared browser and can reach the cockpit later (DESIGN.md §9.5) — a hand-typed POH
          table is not something to lose to a cache wipe.
        </p>
        <p className="hint">
          You can still plan without an account: the Flight Plan view's Aircraft section takes the same
          numbers for this session only.
        </p>
      </div>
    );
  }

  return (
    <div className="aircraft-layout">
      <div className="panel aircraft-fleet">
        <h2>Aircraft</h2>
        {error && <p className="hint route-warning">⚠ {error}</p>}
        {fleet === null && <p className="hint">Loading your fleet…</p>}
        {fleet?.length === 0 && !adding && (
          <p className="hint">No aircraft yet. Add one to plan against real performance figures.</p>
        )}
        {/* Remove is offered here as well as in the editor. The editor's
            Delete sits below ~700px of form, so on a normal screen you
            cannot see it without scrolling — managing a fleet should not
            require opening an aircraft and hunting. */}
        <ul className="fleet-list">
          {fleet?.map((aircraft) => (
            <li key={aircraft.id}>
              <button
                className={aircraft.id === selectedId ? "selected" : ""}
                onClick={() => {
                  setSelectedId(aircraft.id);
                  setAdding(false);
                }}
              >
                <strong>{aircraft.registration}</strong>
                {aircraft.icao_type && <span className="kind-badge">{aircraft.icao_type}</span>}
                {aircraft.name && <span className="airport-name"> {aircraft.name}</span>}
                {!aircraft.verified_at && <span className="unverified-flag">unverified</span>}
              </button>
              <button
                className="clear-button"
                aria-label={`Remove ${aircraft.registration}`}
                title={`Remove ${aircraft.registration}`}
                disabled={removingId === aircraft.id}
                onClick={() => setConfirmingRemoval(aircraft)}
              >
                ×
              </button>
            </li>
          ))}
        </ul>

        {confirmingRemoval && (
          /* An in-page confirmation rather than `window.confirm`: it can
             name what is actually lost (the performance tables, which are
             hand-entered and unrecoverable), and it cannot be suppressed
             by a browser that blocks dialogs. */
          <div className="confirm-removal">
            <p>
              Remove <strong>{confirmingRemoval.registration}</strong>?
            </p>
            <p className="hint">
              Its performance tables go with it. If you typed those out of a POH they cannot be
              recovered — there is no undo.
            </p>
            {removeError && <p className="hint route-warning">⚠ {removeError}</p>}
            <div className="button-row">
              <button
                className="danger"
                disabled={removingId !== null}
                onClick={() => void remove(confirmingRemoval)}
              >
                {removingId !== null ? "Removing…" : "Remove"}
              </button>
              <button disabled={removingId !== null} onClick={() => setConfirmingRemoval(null)}>
                Cancel
              </button>
            </div>
          </div>
        )}
        <button
          onClick={() => {
            setAdding(true);
            setSelectedId(null);
          }}
        >
          + Add aircraft
        </button>
      </div>

      {adding && (
        <AddAircraft
          templates={templates}
          onCancel={() => setAdding(false)}
          onCreated={async (created) => {
            setAdding(false);
            await reloadAndNotify();
            setSelectedId(created.id);
          }}
        />
      )}

      {selectedId !== null && (
        <AircraftEditor
          key={selectedId}
          aircraftId={selectedId}
          onChanged={reloadAndNotify}
          onRequestRemove={() => {
            const target = fleet?.find((a) => a.id === selectedId);
            if (target) setConfirmingRemoval(target);
          }}
        />
      )}
    </div>
  );
}

/** Registration first, then an optional type whose template fills in the
 * rest. The type is skippable — a blank aircraft is valid, and anything
 * typed here beats the book (the server only fills gaps). */
function AddAircraft({
  templates,
  onCancel,
  onCreated,
}: {
  templates: AircraftTemplate[];
  onCancel: () => void;
  onCreated: (created: AircraftDetail) => void;
}) {
  const [registration, setRegistration] = useState("");
  const [fromType, setFromType] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = async () => {
    setBusy(true);
    setError(null);
    try {
      const created = await createAircraft({
        registration,
        from_type: fromType === "" ? null : fromType,
      });
      onCreated(created);
    } catch (err) {
      setError(err instanceof AircraftApiError ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="panel aircraft-add">
      <h2>Add aircraft</h2>
      <label>
        Registration
        <input
          type="text"
          value={registration}
          placeholder="N172SP"
          autoFocus
          onChange={(e) => setRegistration(e.target.value)}
        />
      </label>
      <label>
        Type (optional)
        <select value={fromType} onChange={(e) => setFromType(e.target.value)}>
          <option value="">— none —</option>
          {templates.map((t) => (
            <option key={t.icao_type} value={t.icao_type}>
              {t.icao_type} — {t.name}
            </option>
          ))}
        </select>
      </label>
      <p className="hint">
        Picking a type fills in typical published figures and their performance tables as a starting
        point. They are book numbers for a new airframe on a standard day, not your aeroplane — check
        them against your POH and mark the aircraft verified.
      </p>
      {error && <p className="hint route-warning">⚠ {error}</p>}
      <div className="button-row">
        <button onClick={submit} disabled={busy || registration.trim() === ""}>
          {busy ? "Adding…" : "Add"}
        </button>
        <button onClick={onCancel} disabled={busy}>
          Cancel
        </button>
      </div>
    </div>
  );
}

/** Number field helper: an empty box means "not recorded" (null), not
 * zero. Zero is a real value for a fuel burn and must not be conflated
 * with absence. */
function numberOrNull(raw: string): number | null {
  const trimmed = raw.trim();
  if (trimmed === "") return null;
  const value = Number(trimmed);
  return Number.isFinite(value) ? value : null;
}

function text(value: number | null | undefined): string {
  return value === null || value === undefined ? "" : String(value);
}

const SCALAR_FIELDS: { key: keyof AircraftInput; label: string }[] = [
  { key: "cruise_tas_kt", label: "Cruise TAS (kt)" },
  { key: "cruise_fuel_gph", label: "Cruise burn (gal/hr)" },
  { key: "climb_rate_fpm", label: "Climb rate (ft/min)" },
  { key: "climb_tas_kt", label: "Climb TAS (kt)" },
  { key: "climb_fuel_gph", label: "Climb burn (gal/hr)" },
  { key: "descent_rate_fpm", label: "Descent rate (ft/min)" },
  { key: "descent_tas_kt", label: "Descent TAS (kt)" },
  { key: "descent_fuel_gph", label: "Descent burn (gal/hr)" },
  { key: "taxi_fuel_gal", label: "Taxi allowance (gal)" },
  { key: "fuel_capacity_gal", label: "Usable fuel (gal)" },
  { key: "reserve_minutes", label: "Reserve (min)" },
  { key: "max_gross_weight_lb", label: "Max gross weight (lb)" },
  { key: "forward_cg_limit_in", label: "Forward CG limit (in)" },
  { key: "aft_cg_limit_in", label: "Aft CG limit (in)" },
];

function AircraftEditor({
  aircraftId,
  onChanged,
  onRequestRemove,
}: {
  aircraftId: number;
  onChanged: () => void;
  /** Hands removal back to the fleet list rather than duplicating the
   * confirmation and the delete call in two places. */
  onRequestRemove: () => void;
}) {
  const [detail, setDetail] = useState<AircraftDetail | null>(null);
  const [form, setForm] = useState<AircraftInput | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [status, setStatus] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    fetchAircraft(aircraftId)
      .then((loaded) => {
        if (cancelled) return;
        setDetail(loaded);
        setForm(toInput(loaded));
        setError(null);
      })
      .catch((err: unknown) => {
        if (!cancelled) setError(err instanceof Error ? err.message : String(err));
      });
    return () => {
      cancelled = true;
    };
  }, [aircraftId]);

  if (error && !detail) return <div className="panel">⚠ {error}</div>;
  if (!detail || !form) return <div className="panel hint">Loading…</div>;

  const save = async () => {
    setError(null);
    setStatus(null);
    try {
      const saved = await updateAircraft(aircraftId, form);
      setDetail({ ...detail, ...saved });
      setForm(toInput({ ...detail, ...saved }));
      setStatus("Saved");
      onChanged();
      setTimeout(() => setStatus(null), 1500);
    } catch (err) {
      setError(err instanceof AircraftApiError ? err.message : String(err));
    }
  };

  const savePhase = async (phase: Phase, rows: PerformanceRow[]) => {
    setError(null);
    try {
      const updated = await replacePerformance(aircraftId, phase, rows);
      setDetail(updated);
      setStatus(`${phase} table saved`);
      setTimeout(() => setStatus(null), 1500);
    } catch (err) {
      setError(err instanceof AircraftApiError ? err.message : String(err));
      throw err;
    }
  };


  return (
    <div className="panel aircraft-editor">
      <h2>
        {detail.registration}
        {detail.name && <span className="airport-name"> {detail.name}</span>}
      </h2>

      {!detail.verified_at && (
        <p className="hint route-warning">
          ⚠ Unverified{detail.template_icao ? ` — seeded from the ${detail.template_icao} template` : ""}.
          These are book figures, not your aeroplane. Check them against your POH, then mark verified so
          plans built on them stop carrying this warning.
        </p>
      )}
      {detail.verified_at && (
        <p className="hint">Verified against your POH on {new Date(detail.verified_at).toLocaleDateString()}.</p>
      )}

      <label>
        Registration
        <input
          type="text"
          value={form.registration}
          onChange={(e) => setForm({ ...form, registration: e.target.value })}
        />
      </label>
      <label>
        Name
        <input
          type="text"
          value={form.name ?? ""}
          onChange={(e) => setForm({ ...form, name: e.target.value === "" ? null : e.target.value })}
        />
      </label>
      <label>
        Serial number
        <input
          type="text"
          value={form.serial_number ?? ""}
          onChange={(e) =>
            setForm({ ...form, serial_number: e.target.value === "" ? null : e.target.value })
          }
        />
      </label>
      <label>
        ICAO type
        <input
          type="text"
          value={form.icao_type ?? ""}
          onChange={(e) => setForm({ ...form, icao_type: e.target.value === "" ? null : e.target.value })}
        />
      </label>

      <h3>Performance</h3>
      <p className="hint">
        These are the fallbacks. Where a table below covers the planned altitude, the table wins.
      </p>
      {SCALAR_FIELDS.map(({ key, label }) => (
        <label key={key}>
          {label}
          <input
            type="number"
            value={text(form[key] as number | null)}
            onChange={(e) => setForm({ ...form, [key]: numberOrNull(e.target.value) })}
          />
        </label>
      ))}
      <p className="hint">
        CG limits are not seeded from any template: an envelope is usually not a single forward/aft pair,
        so those two must come from your POH (§9.5.3).
      </p>

      <label className="checkbox-row">
        <input
          type="checkbox"
          checked={form.verified ?? false}
          onChange={(e) => setForm({ ...form, verified: e.target.checked })}
        />
        I have checked these figures against this aircraft's POH
      </label>

      {error && <p className="hint route-warning">⚠ {error}</p>}
      <div className="button-row">
        <button onClick={save}>Save</button>
        {/* Removal lives in the fleet list, which is where a fleet is
            managed and where the confirmation can be seen without
            scrolling past this whole form. */}
        <button onClick={onRequestRemove}>Remove…</button>
        {status && <span className="hint">{status}</span>}
      </div>

      <PerformanceGrid phase="climb" rows={detail.climb} onSave={savePhase} />
      <PerformanceGrid phase="cruise" rows={detail.cruise} onSave={savePhase} />
      <PerformanceGrid phase="descent" rows={detail.descent} onSave={savePhase} />
    </div>
  );
}

function toInput(detail: AircraftDetail | Aircraft): AircraftInput {
  return {
    registration: detail.registration,
    serial_number: detail.serial_number,
    icao_type: detail.icao_type,
    name: detail.name,
    cruise_tas_kt: detail.cruise_tas_kt,
    cruise_fuel_gph: detail.cruise_fuel_gph,
    climb_rate_fpm: detail.climb_rate_fpm,
    climb_tas_kt: detail.climb_tas_kt,
    climb_fuel_gph: detail.climb_fuel_gph,
    descent_rate_fpm: detail.descent_rate_fpm,
    descent_tas_kt: detail.descent_tas_kt,
    descent_fuel_gph: detail.descent_fuel_gph,
    taxi_fuel_gal: detail.taxi_fuel_gal,
    fuel_capacity_gal: detail.fuel_capacity_gal,
    reserve_minutes: detail.reserve_minutes,
    max_gross_weight_lb: detail.max_gross_weight_lb,
    forward_cg_limit_in: detail.forward_cg_limit_in,
    aft_cg_limit_in: detail.aft_cg_limit_in,
    template_icao: detail.template_icao,
    verified: detail.verified_at !== null,
  };
}

const PHASE_LABEL: Record<Phase, string> = {
  climb: "Climb",
  cruise: "Cruise",
  descent: "Descent",
};

/** One phase's table. Edited as a whole document and saved with a single
 * whole-table PUT, matching the API — there are no per-row ids, and a
 * rejected row leaves the stored table untouched (§9.5.4). */
function PerformanceGrid({
  phase,
  rows,
  onSave,
}: {
  phase: Phase;
  rows: PerformanceRow[];
  onSave: (phase: Phase, rows: PerformanceRow[]) => Promise<void>;
}) {
  const [draft, setDraft] = useState<PerformanceRow[]>(rows);
  const [dirty, setDirty] = useState(false);
  const [busy, setBusy] = useState(false);

  // Adopt whatever the server last returned whenever it changes — after a
  // save this is the authoritative, altitude-sorted version.
  useEffect(() => {
    setDraft(rows);
    setDirty(false);
  }, [rows]);

  const isCruise = phase === "cruise";
  const update = (index: number, patch: Partial<PerformanceRow>) => {
    setDraft(draft.map((row, i) => (i === index ? { ...row, ...patch } : row)));
    setDirty(true);
  };
  const addRow = () => {
    setDraft([
      ...draft,
      {
        pressure_altitude_ft: 0,
        power_setting: isCruise ? "65%" : "",
        vertical_speed_fpm: isCruise ? null : 500,
        tas_kt: 0,
        fuel_gph: 0,
      },
    ]);
    setDirty(true);
  };
  const removeRow = (index: number) => {
    setDraft(draft.filter((_, i) => i !== index));
    setDirty(true);
  };

  return (
    <div className="performance-grid">
      <h3>{PHASE_LABEL[phase]} table</h3>
      {draft.length === 0 && (
        <p className="hint">
          No {phase} rows — the scalar {phase} figures above are used instead.
        </p>
      )}
      {draft.length > 0 && (
        <table>
          <thead>
            <tr>
              <th>Alt (ft)</th>
              {isCruise && <th title="Free text — whatever your POH keys the table on">Power</th>}
              {!isCruise && <th>{phase === "climb" ? "Rate up" : "Rate down"} (fpm)</th>}
              <th>TAS (kt)</th>
              <th>Burn (gph)</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {draft.map((row, i) => (
              <tr key={i}>
                <td>
                  <input
                    type="number"
                    value={row.pressure_altitude_ft}
                    onChange={(e) =>
                      update(i, { pressure_altitude_ft: Number(e.target.value) })
                    }
                  />
                </td>
                {isCruise ? (
                  <td>
                    <input
                      type="text"
                      value={row.power_setting}
                      onChange={(e) => update(i, { power_setting: e.target.value })}
                    />
                  </td>
                ) : (
                  <td>
                    <input
                      type="number"
                      value={row.vertical_speed_fpm ?? ""}
                      onChange={(e) => update(i, { vertical_speed_fpm: numberOrNull(e.target.value) })}
                    />
                  </td>
                )}
                <td>
                  <input
                    type="number"
                    value={row.tas_kt}
                    onChange={(e) => update(i, { tas_kt: Number(e.target.value) })}
                  />
                </td>
                <td>
                  <input
                    type="number"
                    value={row.fuel_gph}
                    onChange={(e) => update(i, { fuel_gph: Number(e.target.value) })}
                  />
                </td>
                <td>
                  <button className="clear-button" onClick={() => removeRow(i)} aria-label="Remove row">
                    ×
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      <div className="button-row">
        <button onClick={addRow}>+ Row</button>
        <button
          disabled={!dirty || busy}
          onClick={async () => {
            setBusy(true);
            try {
              await onSave(phase, draft);
            } catch {
              // The editor above shows the reason; keep the draft so the
              // pilot can fix the offending row rather than losing it.
            } finally {
              setBusy(false);
            }
          }}
        >
          {busy ? "Saving…" : `Save ${phase} table`}
        </button>
      </div>
      <p className="hint">
        {isCruise
          ? "Values are interpolated between rows by altitude, and held flat outside the range rather than extrapolated. Keep each power setting on its own set of rows."
          : "Rates are positive magnitudes — the phase supplies the direction."}
      </p>
    </div>
  );
}
