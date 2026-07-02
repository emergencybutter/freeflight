//! Reads an MBTiles (SQLite) tile set — the intermediate format GDAL's
//! `MBTiles` driver produces — and repacks it into a PMTiles archive.
//! Pure Rust, no external tools; kept separate from [`crate::ingest`] so
//! it's testable without GDAL installed.
use crate::catalog::BoundingBox;
use pmtiles2::{util::tile_id, Compression, PMTiles, TileType};
use rusqlite::Connection;
use std::fs::File;
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MbtilesError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("pmtiles error: {0}")]
    Pmtiles(String),
    #[error("MBTiles metadata is missing required key '{0}'")]
    MissingMetadata(&'static str),
    #[error("MBTiles metadata key '{key}' has non-numeric value '{value}'")]
    InvalidMetadataNumber { key: &'static str, value: String },
    #[error("MBTiles 'bounds' metadata value '{0}' is not 4 comma-separated numbers")]
    InvalidBounds(String),
    #[error("unsupported MBTiles tile format '{0}' (only png is supported)")]
    UnsupportedFormat(String),
}

fn metadata_value(conn: &Connection, key: &str) -> Result<Option<String>, MbtilesError> {
    match conn.query_row("SELECT value FROM metadata WHERE name = ?1", [key], |row| {
        row.get(0)
    }) {
        Ok(value) => Ok(Some(value)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

fn required_metadata(conn: &Connection, key: &'static str) -> Result<String, MbtilesError> {
    metadata_value(conn, key)?.ok_or(MbtilesError::MissingMetadata(key))
}

fn required_metadata_u8(conn: &Connection, key: &'static str) -> Result<u8, MbtilesError> {
    let value = required_metadata(conn, key)?;
    value
        .parse()
        .map_err(|_| MbtilesError::InvalidMetadataNumber { key, value })
}

fn parse_bounds(raw: &str) -> Result<BoundingBox, MbtilesError> {
    let parts: Result<Vec<f64>, _> = raw.split(',').map(|s| s.trim().parse::<f64>()).collect();
    let parts = parts.map_err(|_| MbtilesError::InvalidBounds(raw.to_string()))?;
    let [min_lon, min_lat, max_lon, max_lat]: [f64; 4] = parts
        .try_into()
        .map_err(|_| MbtilesError::InvalidBounds(raw.to_string()))?;
    Ok(BoundingBox {
        min_lat,
        min_lon,
        max_lat,
        max_lon,
    })
}

/// Converts a GDAL-produced MBTiles file at `mbtiles_path` into a PMTiles
/// archive at `output_path`. Handles the row-numbering flip between
/// MBTiles' TMS convention (tile row 0 = south edge) and PMTiles'/XYZ's
/// slippy-map convention (tile row 0 = north edge) — getting this wrong
/// silently renders every tile upside down.
pub fn mbtiles_to_pmtiles(
    mbtiles_path: &Path,
    output_path: &Path,
) -> Result<BoundingBox, MbtilesError> {
    let conn = Connection::open(mbtiles_path)?;

    let format = metadata_value(&conn, "format")?.unwrap_or_else(|| "png".to_string());
    if format != "png" {
        return Err(MbtilesError::UnsupportedFormat(format));
    }
    let bounds = parse_bounds(&required_metadata(&conn, "bounds")?)?;
    let min_zoom = required_metadata_u8(&conn, "minzoom")?;
    let max_zoom = required_metadata_u8(&conn, "maxzoom")?;

    let mut pmtiles = PMTiles::new(TileType::Png, Compression::None);
    pmtiles.min_zoom = min_zoom;
    pmtiles.max_zoom = max_zoom;
    pmtiles.center_zoom = min_zoom;
    pmtiles.min_longitude = bounds.min_lon;
    pmtiles.min_latitude = bounds.min_lat;
    pmtiles.max_longitude = bounds.max_lon;
    pmtiles.max_latitude = bounds.max_lat;
    pmtiles.center_longitude = (bounds.min_lon + bounds.max_lon) / 2.0;
    pmtiles.center_latitude = (bounds.min_lat + bounds.max_lat) / 2.0;

    let mut stmt =
        conn.prepare("SELECT zoom_level, tile_column, tile_row, tile_data FROM tiles")?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let zoom: u8 = row.get(0)?;
        let x: i64 = row.get(1)?;
        let tms_y: i64 = row.get(2)?;
        let data: Vec<u8> = row.get(3)?;
        // MBTiles: row 0 is the southernmost row (TMS). PMTiles/XYZ: row 0
        // is the northernmost row.
        let xyz_y = (1i64 << zoom) - 1 - tms_y;
        pmtiles
            .add_tile(tile_id(zoom, x as u64, xyz_y as u64), data)
            .map_err(|e| MbtilesError::Pmtiles(format!("{e:?}")))?;
    }

    let mut file = File::create(output_path)?;
    pmtiles
        .to_writer(&mut file)
        .map_err(|e| MbtilesError::Pmtiles(format!("{e:?}")))?;

    Ok(bounds)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pmtiles2::PMTiles as PMTilesReader;
    use std::fs::File as StdFile;

    /// Builds a minimal MBTiles SQLite file matching what GDAL's MBTiles
    /// driver produces: `metadata(name, value)` and
    /// `tiles(zoom_level, tile_column, tile_row, tile_data)`, using TMS
    /// (south-origin) row numbering.
    fn build_test_mbtiles(path: &Path) {
        let conn = Connection::open(path).unwrap();
        conn.execute_batch(
            "CREATE TABLE metadata (name TEXT, value TEXT);
             CREATE TABLE tiles (zoom_level INTEGER, tile_column INTEGER, tile_row INTEGER, tile_data BLOB);",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO metadata (name, value) VALUES ('format', 'png')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO metadata (name, value) VALUES ('bounds', '-122.6,37.3,-121.8,37.9')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO metadata (name, value) VALUES ('minzoom', '1')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO metadata (name, value) VALUES ('maxzoom', '1')",
            [],
        )
        .unwrap();
        // Zoom 1 has a 2x2 grid of tiles. Tag each tile's data with its
        // (x, tms_y) so the row-flip can be checked after conversion.
        for x in 0..2i64 {
            for tms_y in 0..2i64 {
                let data = format!("tile-x{x}-tmsy{tms_y}").into_bytes();
                conn.execute(
                    "INSERT INTO tiles (zoom_level, tile_column, tile_row, tile_data) VALUES (1, ?1, ?2, ?3)",
                    rusqlite::params![x, tms_y, data],
                )
                .unwrap();
            }
        }
    }

    #[test]
    fn converts_mbtiles_to_pmtiles_and_flips_row_convention() {
        let dir = tempfile::tempdir().unwrap();
        let mbtiles_path = dir.path().join("in.mbtiles");
        let pmtiles_path = dir.path().join("out.pmtiles");
        build_test_mbtiles(&mbtiles_path);

        let bounds = mbtiles_to_pmtiles(&mbtiles_path, &pmtiles_path).unwrap();
        assert_eq!(
            bounds,
            BoundingBox {
                min_lat: 37.3,
                min_lon: -122.6,
                max_lat: 37.9,
                max_lon: -121.8,
            }
        );

        let file = StdFile::open(&pmtiles_path).unwrap();
        let mut pmtiles = PMTilesReader::from_reader(file).unwrap();
        assert_eq!(pmtiles.num_tiles(), 4);

        // MBTiles tms_y=0 (south row) must land at PMTiles/XYZ y=1 (bottom
        // row at zoom 1), and tms_y=1 (north row) at XYZ y=0 (top row).
        let tile = pmtiles.get_tile(0, 1, 1).unwrap().unwrap();
        assert_eq!(tile, b"tile-x0-tmsy0");
        let tile = pmtiles.get_tile(0, 0, 1).unwrap().unwrap();
        assert_eq!(tile, b"tile-x0-tmsy1");
    }

    #[test]
    fn rejects_non_png_format() {
        let dir = tempfile::tempdir().unwrap();
        let mbtiles_path = dir.path().join("in.mbtiles");
        let conn = Connection::open(&mbtiles_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE metadata (name TEXT, value TEXT);
             CREATE TABLE tiles (zoom_level INTEGER, tile_column INTEGER, tile_row INTEGER, tile_data BLOB);
             INSERT INTO metadata (name, value) VALUES ('format', 'jpg');",
        )
        .unwrap();
        drop(conn);

        let err = mbtiles_to_pmtiles(&mbtiles_path, &dir.path().join("out.pmtiles")).unwrap_err();
        assert!(matches!(err, MbtilesError::UnsupportedFormat(f) if f == "jpg"));
    }

    #[test]
    fn reports_missing_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let mbtiles_path = dir.path().join("in.mbtiles");
        let conn = Connection::open(&mbtiles_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE metadata (name TEXT, value TEXT);
             CREATE TABLE tiles (zoom_level INTEGER, tile_column INTEGER, tile_row INTEGER, tile_data BLOB);
             INSERT INTO metadata (name, value) VALUES ('format', 'png');",
        )
        .unwrap();
        drop(conn);

        let err = mbtiles_to_pmtiles(&mbtiles_path, &dir.path().join("out.pmtiles")).unwrap_err();
        assert!(matches!(err, MbtilesError::MissingMetadata("bounds")));
    }
}
