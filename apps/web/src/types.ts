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
  /** The FAA d-TPP plate chart for this procedure, if ff-etl's
   * best-effort ident matching found one — null doesn't mean there's no
   * real chart, just that it couldn't be confidently matched (common for
   * some approach types). */
  chart_name: string | null;
  chart_url: string | null;
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

/** `GET /data/nearest_fix` — the closest waypoint or navaid to a tapped
 * map point. `kind` is "WAYPOINT" for a plain enroute fix, else the
 * navaid's own type (e.g. "VOR", "NDB"). */
export interface NearestFix {
  kind: string;
  ident: string;
  lat: number;
  lon: number;
}

/** Everything a single map tap surfaces — airport selection happens
 * separately (`onSelectAirport`, unconditional on every tap), this is
 * the rest: the closest waypoint/navaid, plus whatever airspace/hazard
 * features were actually under the tapped point. Each array holds raw
 * GeoJSON feature properties (already deduped by id where relevant),
 * one entry per feature found — App.tsx's tab panels format these via
 * tapInfo.ts's *InfoHtml functions, same content the removed Popups
 * used to show. */
export interface MapTapResult {
  lngLat: { lng: number; lat: number };
  nearestFix: NearestFix | null;
  airspace: Record<string, unknown>[];
  gairmets: Record<string, unknown>[];
  sigmets: Record<string, unknown>[];
  cwas: Record<string, unknown>[];
  pireps: Record<string, unknown>[];
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

/** `GET /weather/atis` — datis.clowd.io D-ATIS for one airport. `type` is
 * "combined", or "dep"/"arr" where the ATIS is split; `code` is the info
 * letter; `datis` is the full broadcast text. Empty when the airport has
 * no Digital ATIS (most non-major fields). Mirrors ff-weather's Datis. */
export interface Datis {
  airport: string;
  type: string;
  code: string;
  datis: string;
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

export interface Cwa {
  cwsu: string;
  name: string;
  receiptTime: string;
  validTimeFrom: number;
  validTimeTo: number;
  seriesId: string;
  hazard: string;
  qualifier: string | null;
  base: number | null;
  top: number | null;
  geom: string;
  // Same string-lat/lon shape as GAirmetCoord (confirmed live) — see
  // hazards.rs.
  coords: GAirmetCoord[];
  rawText: string;
}

export interface PirepCloud {
  cover: string;
  base: number | null;
  top: number | null;
}

// Mirrors ff-weather's Pirep — a point report, unlike the area/line
// hazards above. icgBas2/icgTop2/tbBas2/tbTop2 etc. are a second
// reported layer, present only when the pilot reported two distinct
// layers.
export interface Pirep {
  receiptTime: string;
  obsTime: number;
  icaoId: string | null;
  acType: string | null;
  lat: number;
  lon: number;
  fltLvl: number | null;
  clouds: PirepCloud[];
  visib: unknown;
  wxString: string | null;
  temp: number | null;
  icgBas1: number | null;
  icgTop1: number | null;
  icgInt1: string | null;
  icgType1: string | null;
  icgBas2: number | null;
  icgTop2: number | null;
  icgInt2: string | null;
  icgType2: string | null;
  tbBas1: number | null;
  tbTop1: number | null;
  tbInt1: string | null;
  tbType1: string | null;
  tbFreq1: string | null;
  tbBas2: number | null;
  tbTop2: number | null;
  tbInt2: string | null;
  tbType2: string | null;
  tbFreq2: string | null;
  // "PIREP" | "AIREP" | "Urgent PIREP" (confirmed live, all three) — an
  // Urgent PIREP is a report severe enough to flag distinctly rather
  // than blend in with routine ones.
  pirepType: string;
  rawOb: string;
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
  /** Best-effort, resolved server-side against the current cycle bundle
   * (airport/navaid/waypoint ident match) — `null` if nothing matched
   * or no cycle is published yet. See planning/windsAloft.ts, the only
   * consumer that needs a station's location rather than just its
   * reported winds. */
  lat: number | null;
  lon: number | null;
}

export interface WindsAloftBulletin {
  data_based_on: string;
  valid_time: string;
  for_use: string;
  stations: StationWindsAloft[];
}
