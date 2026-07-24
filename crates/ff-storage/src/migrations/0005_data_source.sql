-- Data-source attribution shipped with a cycle bundle, so clients can
-- display who each dataset is from and how current it is. Required by the
-- French SIA (Licence Ouverte: attribution + update date); useful for the
-- FAA/NOAA sources too. Populated by ff-etl per source actually included
-- in the cycle. Part of the read-only, published bundle (above the
-- client-local marker in 0001_init.sql).

CREATE TABLE IF NOT EXISTS data_source (
    name            TEXT PRIMARY KEY,  -- e.g. "France (SIA)"
    effective_date  TEXT,              -- AIRAC effective date, e.g. "2026-07-09"
    licence         TEXT,              -- e.g. "Licence Ouverte"
    url             TEXT,              -- source homepage
    attribution     TEXT NOT NULL      -- the credit string to display
);
