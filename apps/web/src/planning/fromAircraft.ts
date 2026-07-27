// Turns a saved aircraft (ff-api's /aircraft, see ../aircraft.ts) into
// the profile ff-planning takes (DESIGN.md §9.5.6).
//
// The two shapes are close but not identical, and the difference is the
// point: an *aircraft* is a physical thing with fixed performance, while
// a *flight* has a cruise altitude and a chosen power setting. Those two
// live in the plan, not on the record, so they are passed in here rather
// than read off the aircraft.
import type { AircraftDetail, PerformanceRow } from "../aircraft";
import type { AircraftPerformance, AircraftProfile, PerformancePoint } from "./wasm";

function toPoint(row: PerformanceRow): PerformancePoint {
  return {
    pressure_altitude_ft: row.pressure_altitude_ft,
    tas_kt: row.tas_kt,
    fuel_gph: row.fuel_gph,
    vertical_speed_fpm: row.vertical_speed_fpm,
    power_setting: row.power_setting,
  };
}

function toPerformance(aircraft: AircraftDetail): AircraftPerformance | null {
  const performance: AircraftPerformance = {
    climb: aircraft.climb.map(toPoint),
    cruise: aircraft.cruise.map(toPoint),
    descent: aircraft.descent.map(toPoint),
  };
  const empty =
    performance.climb.length === 0 &&
    performance.cruise.length === 0 &&
    performance.descent.length === 0;
  return empty ? null : performance;
}

/** The distinct cruise power settings this aircraft records, sorted.
 * Empty when it has no cruise table, one entry when there's nothing to
 * choose between — the UI only needs to ask when there are two or more. */
export function cruisePowerSettings(aircraft: AircraftDetail): string[] {
  const settings = new Set(aircraft.cruise.map((row) => row.power_setting));
  return [...settings].sort();
}

/** Build the planner's profile for one flight in this aircraft.
 *
 * `cruiseAltitudeFt` and `cruisePowerSetting` belong to the *plan*: the
 * same aeroplane flown at 4,500 ft and at 9,500 ft is two different sets
 * of numbers out of the same tables.
 *
 * Note the fallbacks: `cruise_tas_kt`/`fuel_burn_gph` are required by
 * ff-planning's profile but nullable on a saved aircraft (a pilot may
 * have entered only tables, or nothing yet). Zero would be a lie that
 * propagates — an aircraft with no cruise TAS at all should produce an
 * obviously-unusable plan, not a plausible one — so they fall back to the
 * session defaults the caller passes in. */
export function aircraftToProfile(
  aircraft: AircraftDetail,
  fallback: AircraftProfile,
  cruiseAltitudeFt: number | null,
  cruisePowerSetting: string | null,
): AircraftProfile {
  return {
    name: aircraft.name ?? aircraft.registration,
    cruise_tas_kt: aircraft.cruise_tas_kt ?? fallback.cruise_tas_kt,
    fuel_burn_gph: aircraft.cruise_fuel_gph ?? fallback.fuel_burn_gph,
    max_gross_weight_lb: aircraft.max_gross_weight_lb,
    forward_cg_limit_in: aircraft.forward_cg_limit_in,
    aft_cg_limit_in: aircraft.aft_cg_limit_in,
    // Plan-level, not aircraft-level.
    cruise_altitude_ft: cruiseAltitudeFt,
    cruise_power_setting: cruisePowerSetting,
    climb_rate_fpm: aircraft.climb_rate_fpm,
    climb_tas_kt: aircraft.climb_tas_kt,
    descent_rate_fpm: aircraft.descent_rate_fpm,
    descent_tas_kt: aircraft.descent_tas_kt,
    climb_fuel_gph: aircraft.climb_fuel_gph,
    descent_fuel_gph: aircraft.descent_fuel_gph,
    taxi_fuel_gal: aircraft.taxi_fuel_gal,
    fuel_capacity_gal: aircraft.fuel_capacity_gal,
    reserve_minutes: aircraft.reserve_minutes,
    performance: toPerformance(aircraft),
  };
}
