-- Account storage: users, sessions, and the aircraft records the flight
-- planner reads (DESIGN.md §9.5).
--
-- This is deliberately NOT the cycle-bundle schema (crates/ff-storage,
-- SQLite): ff-etl republishes that wholesale every AIRAC cycle, which
-- would destroy user data every 28 days. Different database, different
-- engine, different lifecycle. Nothing here is ever part of a published
-- bundle.

CREATE TABLE app_user (
    id            BIGSERIAL PRIMARY KEY,
    -- Which provider vouched for this identity, and its provider-local
    -- stable id — the same pair ff-api's OAuth callback already
    -- normalizes to (routes::auth::User).
    provider      TEXT NOT NULL CHECK (provider <> ''),
    subject       TEXT NOT NULL CHECK (subject <> ''),
    display_name  TEXT NOT NULL,
    email         TEXT,
    avatar_url    TEXT,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_seen_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (provider, subject)
);

-- Only a SHA-256 of the bearer token is stored, never the token itself,
-- so a leaked dump does not hand over live sessions (§9.5.5/§9.5.9).
CREATE TABLE session (
    token_hash  BYTEA PRIMARY KEY CHECK (octet_length(token_hash) = 32),
    user_id     BIGINT NOT NULL REFERENCES app_user(id) ON DELETE CASCADE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at  TIMESTAMPTZ NOT NULL
);

-- Supports the periodic expiry sweep that replaces "sessions vanish when
-- the process does".
CREATE INDEX session_expires_at_idx ON session (expires_at);

CREATE TABLE aircraft (
    id             BIGSERIAL PRIMARY KEY,
    user_id        BIGINT NOT NULL REFERENCES app_user(id) ON DELETE CASCADE,
    -- Registration is the identity users search and pick by, unique per
    -- user rather than globally (two pilots may both keep a record for a
    -- shared club aircraft). Stored uppercase so 'n172sp' and 'N172SP'
    -- cannot become two aircraft.
    registration   TEXT NOT NULL CHECK (registration <> '' AND registration = upper(registration)),
    -- Manufacturer serial: the more durable key (survives re-registration)
    -- but not what anyone recognizes their airplane by, so it rides along
    -- rather than leading.
    serial_number  TEXT,
    -- The type designator this record's numbers came from. Explicitly not
    -- the identity: the whole point is that two C172s differ.
    icao_type      TEXT,
    name           TEXT,

    -- Scalar fallbacks, used when no performance-table row applies.
    cruise_tas_kt        DOUBLE PRECISION CHECK (cruise_tas_kt > 0),
    cruise_fuel_gph      DOUBLE PRECISION CHECK (cruise_fuel_gph >= 0),
    climb_rate_fpm       DOUBLE PRECISION CHECK (climb_rate_fpm > 0),
    climb_tas_kt         DOUBLE PRECISION CHECK (climb_tas_kt > 0),
    climb_fuel_gph       DOUBLE PRECISION CHECK (climb_fuel_gph >= 0),
    descent_rate_fpm     DOUBLE PRECISION CHECK (descent_rate_fpm > 0),
    descent_tas_kt       DOUBLE PRECISION CHECK (descent_tas_kt > 0),
    descent_fuel_gph     DOUBLE PRECISION CHECK (descent_fuel_gph >= 0),

    -- Fuel planning. Taxi is a fixed allowance in gallons, not a rate.
    taxi_fuel_gal        DOUBLE PRECISION CHECK (taxi_fuel_gal >= 0),
    fuel_capacity_gal    DOUBLE PRECISION CHECK (fuel_capacity_gal > 0),
    reserve_minutes      INTEGER CHECK (reserve_minutes >= 0),

    -- Weight & balance envelope (§9.3's existing single-envelope check).
    max_gross_weight_lb  DOUBLE PRECISION CHECK (max_gross_weight_lb > 0),
    forward_cg_limit_in  DOUBLE PRECISION,
    aft_cg_limit_in      DOUBLE PRECISION,
    CHECK (forward_cg_limit_in IS NULL OR aft_cg_limit_in IS NULL
           OR forward_cg_limit_in < aft_cg_limit_in),

    -- Provenance (§9.5.3): what template seeded this, and whether the
    -- pilot has confirmed the numbers against their own POH. NULL
    -- verified_at means "book figures, unverified" and is surfaced
    -- wherever this feeds a plan.
    template_icao  TEXT,
    verified_at    TIMESTAMPTZ,

    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now(),

    UNIQUE (user_id, registration)
);

CREATE INDEX aircraft_user_id_idx ON aircraft (user_id);

-- One row per (phase, altitude, power setting). Keyed by pressure
-- altitude only: temperature and weight axes are a documented v1
-- limitation (§9.5.2), additive later.
CREATE TABLE aircraft_performance (
    aircraft_id           BIGINT NOT NULL REFERENCES aircraft(id) ON DELETE CASCADE,
    phase                 TEXT NOT NULL CHECK (phase IN ('climb', 'cruise', 'descent')),
    pressure_altitude_ft  INTEGER NOT NULL CHECK (pressure_altitude_ft BETWEEN -2000 AND 60000),
    -- Free text ('65%', '2400 RPM') because POHs disagree on what they
    -- key cruise rows on. NOT NULL DEFAULT '' because a primary-key
    -- column cannot be NULL, and climb/descent rows have no power
    -- dimension — '' is that "not applicable" slot.
    power_setting         TEXT NOT NULL DEFAULT '',
    -- Positive magnitude in both climb and descent rows; the phase
    -- supplies the sign. NULL for cruise.
    vertical_speed_fpm    DOUBLE PRECISION CHECK (vertical_speed_fpm > 0),
    tas_kt                DOUBLE PRECISION NOT NULL CHECK (tas_kt > 0),
    fuel_gph              DOUBLE PRECISION NOT NULL CHECK (fuel_gph >= 0),
    PRIMARY KEY (aircraft_id, phase, pressure_altitude_ft, power_setting)
);
