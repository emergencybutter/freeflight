//! Chart catalog types and the GeoTIFF-to-tile ingest seam (DESIGN.md §5,
//! §6, §7, §9.1).

pub mod catalog;
pub mod ingest;
pub mod mbtiles;

pub use catalog::{BoundingBox, ChartCatalog, ChartCatalogEntry, ChartKind};
pub use ingest::{geotiff_to_pmtiles, ChartIngestError, GeoTiffSource};
pub use mbtiles::{mbtiles_to_pmtiles, MbtilesError};
