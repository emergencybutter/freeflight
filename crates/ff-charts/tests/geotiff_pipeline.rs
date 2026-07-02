//! Opt-in integration test that runs the full GDAL-backed
//! `geotiff_to_pmtiles` pipeline end to end. Not run by default — it
//! shells out to GDAL's CLI tools (`gdal_create`, `gdalwarp`,
//! `gdal_translate`, `gdaladdo`), which may not be installed everywhere.
//!
//! ```sh
//! cargo test -p ff-charts --test geotiff_pipeline -- --ignored --nocapture
//! ```
//!
//! The source GeoTIFF is synthesized with `gdal_create` rather than
//! downloaded, since real FAA chart imagery isn't available in this
//! environment (egress to FAA domains is blocked) and isn't checked into
//! the repo. This still validates the thing `ff-charts` is actually
//! responsible for — reprojecting, tiling, and repacking into PMTiles,
//! including the MBTiles/PMTiles row-convention flip — just not the
//! pixel content of a real sectional.
use ff_charts::{geotiff_to_pmtiles, ChartKind, GeoTiffSource};
use pmtiles2::PMTiles;
use std::process::Command;

#[test]
#[ignore]
fn converts_a_synthetic_geotiff_end_to_end() {
    let workdir = tempfile::tempdir().unwrap();
    let source_path = workdir.path().join("source.tif");
    let output_path = workdir.path().join("out.pmtiles");

    // A small 3-band GeoTIFF covering the SF Bay Area, in geographic
    // (EPSG:4326) coordinates, matching what FAA GeoTIFF chart releases
    // look like before reprojection.
    let status = Command::new("gdal_create")
        .args([
            "-outsize",
            "512",
            "512",
            "-bands",
            "3",
            "-burn",
            "120 140 160",
            "-a_srs",
            "EPSG:4326",
            "-a_ullr",
            "-122.6",
            "37.9",
            "-121.8",
            "37.3",
            "-ot",
            "Byte",
        ])
        .arg(&source_path)
        .status()
        .expect("gdal_create must be on PATH to run this test");
    assert!(status.success(), "gdal_create failed");

    let source = GeoTiffSource {
        path: source_path,
        kind: ChartKind::Sectional,
        cycle_id: "2026-07".into(),
    };

    let bbox = geotiff_to_pmtiles(&source, &output_path).expect("pipeline should succeed");

    // gdalwarp's reprojected extent won't exactly match the source
    // bounds (resampling/edge effects), but should be close.
    assert!((bbox.min_lon - -122.6).abs() < 0.1);
    assert!((bbox.max_lon - -121.8).abs() < 0.1);
    assert!((bbox.min_lat - 37.3).abs() < 0.1);
    assert!((bbox.max_lat - 37.9).abs() < 0.1);

    let file = std::fs::File::open(&output_path).unwrap();
    let pmtiles = PMTiles::from_reader(file).unwrap();
    assert!(
        pmtiles.num_tiles() > 0,
        "expected at least one tile in the output archive"
    );
}
