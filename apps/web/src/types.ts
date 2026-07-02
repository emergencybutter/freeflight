// Row shapes mirror the ff-storage schema (DESIGN.md §6) directly —
// there's no separate wire format yet, so these match the SQLite columns
// one-to-one.

export interface Airport {
  icao: string;
  faa_id: string | null;
  iata: string | null;
  name: string;
  lat: number;
  lon: number;
  elevation_ft: number;
  airport_type: string;
}

export interface Runway {
  airport_icao: string;
  ident: string;
  length_ft: number;
  width_ft: number;
  surface: string;
  le_ident: string;
  le_heading_deg: number;
  he_ident: string;
  he_heading_deg: number;
}

export interface Frequency {
  airport_icao: string;
  kind: string;
  freq_mhz: number;
  remarks: string | null;
}

export interface Procedure {
  id: string;
  airport_icao: string;
  kind: string;
  ident: string;
  runway_ident: string | null;
}

export interface ProcedureTransition {
  id: string;
  procedure_id: string;
  ident: string;
  kind: string;
}

export interface ProcedureLeg {
  transition_id: string;
  seq: number;
  path_and_term: string;
  fix_ident: string | null;
  course_deg: number | null;
  altitude_constraint: string | null;
  speed_constraint: string | null;
  turn_direction: string | null;
}
