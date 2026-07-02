-- Initial schema, mirrors DESIGN.md §6.
--
-- Tables above the "client-local only" marker are populated by ff-etl and
-- shipped as part of a read-only cycle bundle; tables below it live only
-- in each client's local database and are never part of a published
-- bundle.

CREATE TABLE IF NOT EXISTS schema_migrations (
    version     INTEGER PRIMARY KEY,
    applied_at  TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS airac_cycle (
    id              TEXT PRIMARY KEY,
    effective_date  TEXT NOT NULL,
    source_version  TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS airport (
    icao            TEXT PRIMARY KEY,
    faa_id          TEXT,
    iata            TEXT,
    name            TEXT NOT NULL,
    lat             REAL NOT NULL,
    lon             REAL NOT NULL,
    elevation_ft    INTEGER NOT NULL,
    airport_type    TEXT NOT NULL,
    fuel_types      TEXT NOT NULL DEFAULT ''  -- comma-separated
);

CREATE TABLE IF NOT EXISTS runway (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    airport_icao    TEXT NOT NULL REFERENCES airport(icao),
    ident           TEXT NOT NULL,
    length_ft       INTEGER NOT NULL,
    width_ft        INTEGER NOT NULL,
    surface         TEXT NOT NULL,
    le_ident        TEXT NOT NULL,
    le_lat          REAL NOT NULL,
    le_lon          REAL NOT NULL,
    le_heading_deg  REAL NOT NULL,
    he_ident        TEXT NOT NULL,
    he_lat          REAL NOT NULL,
    he_lon          REAL NOT NULL,
    he_heading_deg  REAL NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_runway_airport ON runway(airport_icao);

CREATE TABLE IF NOT EXISTS frequency (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    airport_icao    TEXT NOT NULL REFERENCES airport(icao),
    kind            TEXT NOT NULL,
    freq_mhz        REAL NOT NULL,
    remarks         TEXT
);
CREATE INDEX IF NOT EXISTS idx_frequency_airport ON frequency(airport_icao);

CREATE TABLE IF NOT EXISTS navaid (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    ident           TEXT NOT NULL,
    navaid_type     TEXT NOT NULL,
    lat             REAL NOT NULL,
    lon             REAL NOT NULL,
    elevation_ft    INTEGER,
    freq_khz        INTEGER,
    region          TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_navaid_ident ON navaid(ident);

CREATE TABLE IF NOT EXISTS waypoint (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    ident           TEXT NOT NULL,
    lat             REAL NOT NULL,
    lon             REAL NOT NULL,
    region          TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_waypoint_ident ON waypoint(ident);

CREATE TABLE IF NOT EXISTS airway (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    ident           TEXT NOT NULL,
    kind            TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS airway_leg (
    airway_id       INTEGER NOT NULL REFERENCES airway(id),
    seq             INTEGER NOT NULL,
    fix_ident       TEXT NOT NULL,
    min_altitude_ft INTEGER,
    max_altitude_ft INTEGER,
    PRIMARY KEY (airway_id, seq)
);

CREATE TABLE IF NOT EXISTS procedure (
    id              TEXT PRIMARY KEY,
    airport_icao    TEXT NOT NULL REFERENCES airport(icao),
    kind            TEXT NOT NULL,  -- SID | STAR | APPROACH
    ident           TEXT NOT NULL,
    runway_ident    TEXT
);
CREATE INDEX IF NOT EXISTS idx_procedure_airport ON procedure(airport_icao);

CREATE TABLE IF NOT EXISTS procedure_transition (
    id              TEXT PRIMARY KEY,
    procedure_id    TEXT NOT NULL REFERENCES procedure(id),
    ident           TEXT NOT NULL,
    kind            TEXT NOT NULL  -- ENROUTE | COMMON | APPROACH | MISSED
);
CREATE INDEX IF NOT EXISTS idx_transition_procedure ON procedure_transition(procedure_id);

CREATE TABLE IF NOT EXISTS procedure_leg (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    transition_id       TEXT NOT NULL REFERENCES procedure_transition(id),
    seq                 INTEGER NOT NULL,
    path_and_term       TEXT NOT NULL,
    fix_ident           TEXT,
    course_deg          REAL,
    altitude_constraint TEXT,
    speed_constraint    TEXT,
    turn_direction      TEXT
);
CREATE INDEX IF NOT EXISTS idx_leg_transition ON procedure_leg(transition_id);

CREATE TABLE IF NOT EXISTS airspace (
    id                  TEXT PRIMARY KEY,
    name                TEXT NOT NULL,
    class               TEXT NOT NULL,
    floor               TEXT NOT NULL,   -- e.g. "MSL:3000", "SFC"
    ceiling             TEXT NOT NULL,
    boundary_geojson    TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS chart_catalog (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    kind        TEXT NOT NULL,
    cycle_id    TEXT NOT NULL,
    min_lat     REAL NOT NULL,
    min_lon     REAL NOT NULL,
    max_lat     REAL NOT NULL,
    max_lon     REAL NOT NULL,
    tile_url    TEXT NOT NULL
);

-- ===================== client-local only below ==========================

CREATE TABLE IF NOT EXISTS aircraft_profile (
    id                      INTEGER PRIMARY KEY AUTOINCREMENT,
    name                    TEXT NOT NULL,
    cruise_tas_kt           REAL NOT NULL,
    fuel_burn_gph           REAL NOT NULL,
    max_gross_weight_lb     REAL,
    forward_cg_limit_in     REAL,
    aft_cg_limit_in         REAL
);

CREATE TABLE IF NOT EXISTS route_plan (
    id                      INTEGER PRIMARY KEY AUTOINCREMENT,
    name                    TEXT NOT NULL,
    created_at              TEXT NOT NULL,
    aircraft_profile_id     INTEGER REFERENCES aircraft_profile(id)
);

CREATE TABLE IF NOT EXISTS route_leg (
    route_plan_id   INTEGER NOT NULL REFERENCES route_plan(id),
    seq             INTEGER NOT NULL,
    waypoint_ref    TEXT NOT NULL,
    altitude_ft     INTEGER,
    notes           TEXT,
    PRIMARY KEY (route_plan_id, seq)
);

CREATE TABLE IF NOT EXISTS flight_track (
    id                      INTEGER PRIMARY KEY AUTOINCREMENT,
    started_at              TEXT NOT NULL,
    ended_at                TEXT,
    aircraft_profile_id     INTEGER REFERENCES aircraft_profile(id)
);

CREATE TABLE IF NOT EXISTS track_point (
    flight_track_id INTEGER NOT NULL REFERENCES flight_track(id),
    seq             INTEGER NOT NULL,
    ts              TEXT NOT NULL,
    lat             REAL NOT NULL,
    lon             REAL NOT NULL,
    alt_ft          REAL,
    gs_kt           REAL,
    track_deg       REAL,
    PRIMARY KEY (flight_track_id, seq)
);

CREATE TABLE IF NOT EXISTS flight_log_entry (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    flight_track_id     INTEGER REFERENCES flight_track(id),
    route_plan_id       INTEGER REFERENCES route_plan(id),
    departure           TEXT,
    arrival             TEXT,
    total_time_seconds  INTEGER,
    taxi_time_seconds   INTEGER,
    landings            INTEGER
);
