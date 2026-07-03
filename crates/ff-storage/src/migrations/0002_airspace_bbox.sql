-- Bounding box columns for the `airspace` table, mirroring
-- `chart_catalog`'s (min_lat, min_lon, max_lat, max_lon): needed so
-- `ff-api`'s `/data/airspace?bbox=` can filter with an indexed range
-- scan instead of parsing every row's `boundary_geojson` server-side.
ALTER TABLE airspace ADD COLUMN min_lat REAL NOT NULL DEFAULT 0;
ALTER TABLE airspace ADD COLUMN min_lon REAL NOT NULL DEFAULT 0;
ALTER TABLE airspace ADD COLUMN max_lat REAL NOT NULL DEFAULT 0;
ALTER TABLE airspace ADD COLUMN max_lon REAL NOT NULL DEFAULT 0;
CREATE INDEX IF NOT EXISTS idx_airspace_bbox ON airspace(min_lat, max_lat, min_lon, max_lon);
