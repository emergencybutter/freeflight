//! Glue for turning FAA GeoTIFF chart releases into offline-servable tiles.
//!
//! `ff-charts` deliberately does not reimplement raster reprojection/tiling
//! in Rust — that's a solved problem in mature, widely used tools (GDAL for
//! reprojection, `go-pmtiles`/`tippecanoe`-family tools for tiling). This
//! module is the seam `ff-etl` calls through; the concrete implementation
//! is expected to shell out to those tools and is not written yet.
use crate::catalog::{BoundingBox, ChartKind};
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ChartIngestError {
    #[error("chart ingest pipeline is not implemented yet")]
    NotImplemented,
    #[error("external tool invocation failed: {0}")]
    ToolFailed(String),
}

/// A GeoTIFF chart release ready to be converted into a tiled, offline
/// servable chart.
#[derive(Debug, Clone)]
pub struct GeoTiffSource {
    pub path: std::path::PathBuf,
    pub kind: ChartKind,
    pub cycle_id: String,
}

/// Reproject + tile a [`GeoTiffSource`] into a PMTiles archive at
/// `output_path`, returning the resulting bounding box for the catalog
/// entry. Not implemented — see module docs.
pub fn geotiff_to_pmtiles(
    _source: &GeoTiffSource,
    _output_path: &Path,
) -> Result<BoundingBox, ChartIngestError> {
    Err(ChartIngestError::NotImplemented)
}
