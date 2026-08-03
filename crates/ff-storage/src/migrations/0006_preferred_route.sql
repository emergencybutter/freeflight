-- Suggested departure/arrival routings, from two FAA-published city-pair
-- route databases (see ff-etl's preferred_routes.rs): the NFDC Preferred
-- Routes Database ('PFR' rows -- altitude/aircraft-restricted routings
-- ATC expects to file, e.g. TEC/High/Low) and the ATCSCC Coded Departure
-- Routes database ('CDR' rows -- pre-coordinated reroute strings). Both
-- are "here is what is commonly flown between these two airports", not a
-- clearance or a guarantee either will be assigned.
--
-- route_string holds only the intermediate fixes/airways -- the
-- departure/arrival airports are stripped at ETL time, since the route
-- builder already has them as separate fields and would otherwise show
-- the departure airport twice. An empty string means a direct routing
-- with no filed intermediate fixes.
CREATE TABLE IF NOT EXISTS preferred_route (
    id                      INTEGER PRIMARY KEY AUTOINCREMENT,
    source                  TEXT NOT NULL, -- 'PFR' | 'CDR'
    orig_icao               TEXT NOT NULL REFERENCES airport(icao),
    dest_icao               TEXT NOT NULL REFERENCES airport(icao),
    route_string            TEXT NOT NULL,
    route_type              TEXT,          -- PFR's Type (H/L/HSD/LSD/SHD/SLD/TEC); NULL for CDR
    altitude                TEXT,          -- PFR only
    aircraft                TEXT,          -- PFR only: equipment/type restriction text
    direction               TEXT,          -- PFR only
    area                    TEXT,          -- PFR only
    code                    TEXT,          -- CDR's route code, e.g. "ABECLTGV"; NULL for PFR
    dep_fix                 TEXT,          -- CDR only
    coordination_required   TEXT,          -- CDR's Y/N; NULL for PFR
    nav_equipment           TEXT,          -- CDR only
    dep_artcc               TEXT,
    arr_artcc               TEXT,
    seq                     INTEGER
);
CREATE INDEX IF NOT EXISTS idx_preferred_route_orig_dest ON preferred_route(orig_icao, dest_icao);
