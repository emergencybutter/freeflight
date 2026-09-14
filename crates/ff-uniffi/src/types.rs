//! The records that cross the FFI boundary.
//!
//! Field names mirror the `ff-storage` schema columns one-to-one, the same
//! convention `ff-api`'s `/data/*` responses follow — so a row means the
//! same thing whichever client is reading it, and the Android UI can be
//! read against the schema (DESIGN.md §6) without a translation table in
//! between.

/// A map viewport, in the same order as `ff-api`'s `?bbox=` query
/// (`minLon,minLat,maxLon,maxLat` is that route's *wire* order; this is a
/// named record precisely so the ordering can't be got wrong here).
#[derive(Debug, Clone, Copy, uniffi::Record)]
pub struct BoundingBox {
    pub min_lat: f64,
    pub min_lon: f64,
    pub max_lat: f64,
    pub max_lon: f64,
}

/// What the app knows about the cycle it is flying on. Every field here
/// exists to be *displayed*: §11 requires the UI to always show the AIRAC
/// cycle in use, so this is the one query the map's status line makes.
#[derive(Debug, Clone, uniffi::Record)]
pub struct CycleInfo {
    pub cycle_id: String,
    /// From the bundle's own `airac_cycle` row, which is the authority —
    /// not the directory name it happens to be installed under.
    pub effective_date: Option<String>,
    pub airport_count: u32,
    pub procedure_count: u32,
    pub bundle_bytes: u64,
    /// Chart ids from `chart_catalog` whose PMTiles archive is on disk.
    pub installed_chart_ids: Vec<String>,
}

/// `ff-api`'s `GET /cycles/latest` body, parsed by `ff-sync`'s own type so
/// the client and the server cannot drift apart (DESIGN.md §4.1 records
/// that they once did).
#[derive(Debug, Clone, uniffi::Record)]
pub struct CycleManifest {
    pub cycle_id: String,
    pub sqlite_url: String,
    pub sqlite_sha256: String,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct Airport {
    pub icao: String,
    pub faa_id: Option<String>,
    pub iata: Option<String>,
    pub name: String,
    pub lat: f64,
    pub lon: f64,
    pub elevation_ft: i64,
    /// "Airport" | "Heliport" | "Seaplane" | "Ultralight", as written by
    /// `ff-etl`'s `airport_type_str`.
    pub airport_type: String,
    /// Whether this airport has any procedure in the bundle — the cheap
    /// signal the map uses to draw towered/IFR-served fields larger, and
    /// the airport sheet uses to decide whether to offer a Procedures tab
    /// at all. Only populated by the queries that need it.
    pub has_procedures: bool,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct Runway {
    pub ident: String,
    pub length_ft: i64,
    pub width_ft: i64,
    pub surface: String,
    pub le_ident: String,
    pub le_lat: f64,
    pub le_lon: f64,
    pub le_heading_deg: f64,
    pub he_ident: String,
    pub he_lat: f64,
    pub he_lon: f64,
    pub he_heading_deg: f64,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct Frequency {
    pub kind: String,
    pub freq_mhz: f64,
    pub remarks: Option<String>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct AirportDetail {
    pub airport: Airport,
    pub runways: Vec<Runway>,
    pub frequencies: Vec<Frequency>,
    /// The FAA d-TPP airport diagram, when this cycle confidently matched
    /// one. `None` means "not linked", not "doesn't exist".
    pub airport_diagram_url: Option<String>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct Procedure {
    pub id: String,
    pub airport_icao: String,
    /// "SID" | "STAR" | "APPROACH".
    pub kind: String,
    pub ident: String,
    pub runway_ident: Option<String>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct ProcedureLeg {
    pub seq: i64,
    pub path_and_term: String,
    pub fix_ident: Option<String>,
    pub course_deg: Option<f64>,
    pub altitude_constraint: Option<String>,
    pub speed_constraint: Option<String>,
    pub turn_direction: Option<String>,
    /// Resolved here rather than left to the client, for the same reason
    /// `ff-api` resolves it server-side: drawing a procedure shouldn't
    /// require the caller to run its own waypoint/navaid lookups. `None`
    /// for a leg whose fix isn't a point in this bundle.
    pub lat: Option<f64>,
    pub lon: Option<f64>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct ProcedureTransition {
    pub id: String,
    pub ident: String,
    /// "ENROUTE" | "COMMON" | "APPROACH" | "MISSED".
    pub kind: String,
    pub legs: Vec<ProcedureLeg>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct ProcedureDetail {
    pub procedure: Procedure,
    pub transitions: Vec<ProcedureTransition>,
    pub chart_name: Option<String>,
    pub chart_url: Option<String>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct Airspace {
    pub id: String,
    pub name: String,
    pub class: String,
    pub floor: String,
    pub ceiling: String,
    /// A GeoJSON `Polygon` *geometry* (not a `Feature`), exactly as the
    /// bundle stores it, so the caller can hand it straight to MapLibre.
    pub boundary_geojson: String,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct Chart {
    pub id: String,
    pub name: String,
    /// `ChartKind` as written into the bundle, e.g. "Sectional",
    /// "IfrEnrouteLow".
    pub kind: String,
    pub bbox: BoundingBox,
    /// Path on `ff-api` the PMTiles archive is served from, e.g.
    /// `/bundles/2026-07-09/chart-seattle.pmtiles`. The client joins this
    /// onto its configured API base to download the chart for offline use.
    pub tile_url: String,
    /// Published size of the archive, from the catalogue — what this chart
    /// will cost to download. `None` for bundles older than migration
    /// 0008, where the UI has to say the size is unknown rather than
    /// guess; sizes here range from about 50MB to 600MB, so guessing
    /// would be worse than admitting it.
    pub download_bytes: Option<u64>,
    /// Whether that archive is already on this device.
    pub installed: bool,
    pub installed_bytes: u64,
    /// The zoom levels the installed archive actually holds, read from its
    /// header. Both zero when the chart isn't installed — there is no
    /// archive to ask, and nothing will be drawn from it either way. The
    /// map must use these rather than assume a range; see
    /// `charts::zoom_range`.
    pub min_zoom: u8,
    pub max_zoom: u8,
}

/// One hit from the unified search box: an airport, or a navaid/waypoint
/// that can be routed through.
#[derive(Debug, Clone, uniffi::Record)]
pub struct SearchHit {
    /// "airport" | "navaid" | "waypoint".
    pub kind: String,
    pub ident: String,
    pub name: Option<String>,
    pub lat: f64,
    pub lon: f64,
}

/// A credit the UI is legally required to show for data in the bundle
/// (DESIGN.md §11 — the French SIA's Licence Ouverte, FAA/NOAA terms).
#[derive(Debug, Clone, uniffi::Record)]
pub struct DataSourceCredit {
    pub name: String,
    pub effective_date: Option<String>,
    pub licence: Option<String>,
    pub url: Option<String>,
    pub attribution: String,
}
