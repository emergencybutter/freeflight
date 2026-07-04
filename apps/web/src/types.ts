// Shapes mirror ff-api's /data/* JSON responses (DESIGN.md §4.1), whose
// field names in turn mirror the ff-storage schema columns one-to-one —
// they were originally written against those columns back when this
// client queried the SQLite bundle itself, and the wire format kept
// them.

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

/** `GET /data/airports/:icao` — the airport row plus its runways and
 * frequencies in one response. */
export interface AirportDetail extends Airport {
  runways: Runway[];
  frequencies: Frequency[];
}

export interface Procedure {
  id: string;
  airport_icao: string;
  kind: string;
  ident: string;
  runway_ident: string | null;
}

export interface ProcedureLeg {
  seq: number;
  path_and_term: string;
  fix_ident: string | null;
  course_deg: number | null;
  altitude_constraint: string | null;
  speed_constraint: string | null;
  turn_direction: string | null;
}

export interface ProcedureTransitionDetail {
  id: string;
  ident: string;
  kind: string;
  legs: ProcedureLeg[];
}

export interface FixCoord {
  lat: number;
  lon: number;
}

/** `GET /data/procedures/:id` — the procedure with its transitions,
 * legs, and server-resolved coordinates for every referenced fix that
 * exists in the cycle bundle (runway-threshold pseudo-fixes like
 * "RW28L" won't appear). */
export interface ProcedureDetail extends Procedure {
  transitions: ProcedureTransitionDetail[];
  fixes: Record<string, FixCoord>;
}

export interface AirwayLegRow {
  seq: number;
  fix_ident: string;
  min_altitude_ft: number | null;
  max_altitude_ft: number | null;
}

/** `GET /data/airways/:ident` — the airway's seq-ordered legs plus
 * server-resolved coordinates for every fix that exists in the cycle
 * bundle, mirroring `ProcedureDetail.fixes`. */
export interface AirwayDetail {
  ident: string;
  kind: string;
  legs: AirwayLegRow[];
  fixes: Record<string, FixCoord>;
}

/** `GET /data/search_idents` — one row per match across airports,
 * waypoints, navaids, and airways. `lat`/`lon` are null for airways
 * (an airway is a path, not a point); `name` is the airport name, or
 * the airway kind (VICTOR/JET/...), or null. */
export interface IdentSearchRow {
  kind: "airport" | "waypoint" | "navaid" | "airway";
  ident: string;
  name: string | null;
  lat: number | null;
  lon: number | null;
}

/** One resolved point on a planned route — an airport, waypoint, or
 * navaid (airway tokens expand into these; see planning/expandRoute). */
export interface RouteWaypoint {
  ident: string;
  name: string | null;
  lat: number;
  lon: number;
}

/** A resolved SID/STAR — see planning/procedureLookup.ts. `points` is
 * already the final ordered fix sequence, ready to splice into a route.
 * The departure/arrival airport + SID/STAR are dedicated slots in the
 * route builder (App.tsx), not tokens in the reorderable middle list —
 * picked explicitly (departure/arrival first, then "Set SID"/"Set
 * STAR" browses that airport's real procedures) rather than typed as
 * flight-plan-string notation, since which airport's procedures to
 * look up is then always already known. */
export interface ResolvedProcedureRef {
  airportIcao: string;
  kind: "SID" | "STAR";
  procedureIdent: string;
  transitionIdent: string;
  points: RouteWaypoint[];
}

/** One entry in the route builder's reorderable middle list (fixes and
 * airways between the SID and STAR) — flight-plan-string style: points
 * stand alone; an airway token takes its meaning from its neighbors
 * (`FOO V123 BAR` — expansion inserts V123's fixes strictly between
 * FOO and BAR, in that direction). */
export type RouteToken =
  | { kind: "point"; point: RouteWaypoint }
  | { kind: "airway"; ident: string; detail: AirwayDetail };

/** The whole route builder's state (App.tsx, lifted so MapView can draw
 * it too): departure/arrival airports and an optional SID/STAR are
 * dedicated slots, chosen explicitly and in that order; `middleTokens`
 * is the reorderable fixes/airways list strung between them. */
export interface RouteState {
  departure: RouteWaypoint | null;
  arrival: RouteWaypoint | null;
  sid: ResolvedProcedureRef | null;
  star: ResolvedProcedureRef | null;
  middleTokens: RouteToken[];
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

/** `GET /data/airspace` — Class B/C/D + Special Use Airspace boundaries.
 * `class` is `"B"|"C"|"D"|"E"|"G"` or a special-use kind
 * (`"MOA"|"RESTRICTED"|"PROHIBITED"|"WARNING"|"ALERT"`) — see
 * `ff-etl/src/bundle.rs`'s `airspace_class_str`. `floor`/`ceiling` are
 * pre-formatted strings (e.g. `"MSL:7000"`, `"SFC"`, `"FL180"`,
 * `"UNLTD"`), not structured, since the map only displays them.
 * `boundary_geojson` is a GeoJSON `Polygon` geometry object (not a whole
 * `Feature`), stored/served as an unparsed string. */
export interface AirspaceVolume {
  id: string;
  name: string;
  class: string;
  floor: string;
  ceiling: string;
  boundary_geojson: string;
  min_lat: number;
  min_lon: number;
  max_lat: number;
  max_lon: number;
}

/** `GET /cycles/latest` — mirrors ff-sync's CycleManifest. */
export interface CycleManifest {
  cycle_id: string;
  sqlite_url: string;
  sqlite_sha256: string;
  pmtiles_url: string | null;
  pmtiles_sha256: string | null;
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
  fltCat: string | null;
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

// Mirror ff-weather's GAirmet/Sigmet wire shape (crates/ff-weather/src/hazards.rs).

export interface GAirmetCoord {
  // Numeric-looking but really strings on the wire — see hazards.rs.
  lat: string;
  lon: string;
}

export interface GAirmet {
  tag: string;
  forecastHour: number;
  validTime: string;
  hazard: string;
  geometryType: string;
  latlonpairs: number;
  frequency: string | null;
  severity: string | null;
  due_to: string | null;
  status: string;
  top: string | null;
  base: string | null;
  fzltop: string | null;
  fzlbase: string | null;
  level: string | null;
  receiptTime: number;
  issueTime: number;
  expireTime: number;
  product: string;
  geom: string;
  coords: GAirmetCoord[];
}

export interface SigmetCoord {
  lat: number;
  lon: number;
}

export interface Sigmet {
  icaoId: string;
  alphaChar: string;
  seriesId: string;
  receiptTime: string;
  creationTime: string;
  validTimeFrom: number;
  validTimeTo: number;
  airSigmetType: string;
  hazard: string;
  altitudeHi1: number | null;
  altitudeHi2: number | null;
  altitudeLow1: number | null;
  altitudeLow2: number | null;
  movementDir: number | null;
  movementSpd: number | null;
  rawAirSigmet: string;
  postProcessFlag: number;
  severity: number;
  coords: SigmetCoord[];
}

// Mirror ff-weather's winds_aloft wire shape
// (crates/ff-weather/src/winds_aloft.rs). `Wind` is a Rust enum with the
// default serde external tagging: the unit variant serializes as the bare
// string `"LightAndVariable"`, the struct variant as
// `{ "Directional": { direction_deg, speed_kt } }` — confirmed against
// ff-api's actual /weather/windtemp response.
export type Wind = "LightAndVariable" | { Directional: { direction_deg: number; speed_kt: number } };

export interface WindsAloftLevel {
  altitude_ft: number;
  wind: Wind;
  temp_c: number | null;
}

export interface StationWindsAloft {
  station_id: string;
  levels: WindsAloftLevel[];
}

export interface WindsAloftBulletin {
  data_based_on: string;
  valid_time: string;
  for_use: string;
  stations: StationWindsAloft[];
}
