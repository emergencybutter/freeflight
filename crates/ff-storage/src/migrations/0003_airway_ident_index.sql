-- Airway lookup is by user-typed ident (ff-api's /data/airways/:ident
-- and the unified ident search) — same access pattern that already
-- justifies idx_navaid_ident/idx_waypoint_ident.
CREATE INDEX IF NOT EXISTS idx_airway_ident ON airway(ident);
