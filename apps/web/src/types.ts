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
  le_lat: number;
  le_lon: number;
  le_heading_deg: number;
  he_ident: string;
  he_lat: number;
  he_lon: number;
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

/** Not a distinct table — a lat/lon lookup merged from `waypoint` and
 * `navaid`, keyed by ident, so procedure legs can be plotted on the map. */
export interface Fix {
  ident: string;
  lat: number;
  lon: number;
}

export interface ChartCatalogEntry {
  id: string;
  name: string;
  kind: string;
  cycle_id: string;
  min_lat: number;
  min_lon: number;
  max_lat: number;
  max_lon: number;
  tile_url: string;
}

// Mirror ff-weather's Metar/Taf wire shape (crates/ff-weather/src/records.rs)
// one-to-one — these come straight from ff-api's proxy, which re-serializes
// the same struct fields with the same #[serde(rename = ...)] names.

export interface CloudLayer {
  cover: string;
  base: number | null;
}

export interface Metar {
  icaoId: string;
  obsTime: number;
  rawOb: string;
  temp: number | null;
  dewp: number | null;
  wdir: number | string | null;
  wspd: number | null;
  wgst: number | null;
  visib: number | string | null;
  altim: number | null;
  wxString: string | null;
  clouds: CloudLayer[];
  lat: number | null;
  lon: number | null;
  elev: number | null;
  name: string | null;
}

export interface TafForecastPeriod {
  timeFrom: number;
  timeTo: number;
  fcstChange: string | null;
  wdir: number | string | null;
  wspd: number | null;
  wgst: number | null;
  visib: number | string | null;
  wxString: string | null;
  clouds: CloudLayer[];
}

export interface Taf {
  icaoId: string;
  issueTime: string;
  validTimeFrom: number;
  validTimeTo: number;
  rawTAF: string;
  fcsts: TafForecastPeriod[];
}
