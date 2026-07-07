-- FAA d-TPP (Digital Terminal Procedures Publication) SID/STAR/Approach
-- plate chart links — one row per procedure ident that could be
-- confidently matched against a chart in the current cycle's d-TPP
-- metafile (see ff-etl's dtpp.rs). pdf_url is precomputed at ETL time
-- (embeds the d-TPP cycle number, which isn't the same as this app's own
-- CIFP cycle_date), same pattern chart_catalog.tile_url already uses.
CREATE TABLE IF NOT EXISTS dtpp_chart (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    airport_icao    TEXT NOT NULL,
    procedure_ident TEXT NOT NULL,
    chart_name      TEXT NOT NULL,
    pdf_url         TEXT NOT NULL,
    cycle           TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_dtpp_chart_procedure ON dtpp_chart(airport_icao, procedure_ident);
