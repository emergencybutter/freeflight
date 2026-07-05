//! One-off tool to build a small SQLite demo bundle (a handful of real
//! airports) from local FAA/NOAA source files, for cases that want a
//! bundle without waiting on/depending on live network access — the real
//! pipeline (`cargo run -p ff-etl`, DESIGN.md §7) fetches its own
//! nationwide inputs (every airport/procedure, every current FAA
//! sectional); this example takes any CIFP file/NASR extract/ICAO list
//! (and at most one chart) you already have on disk instead.
//!
//! Usage:
//! ```sh
//! cargo run -p ff-etl --example build_demo_bundle -- <cifp-file> <output.sqlite> \
//!   [--nasr-dir <dir>] [--chart-geotiff <path> --chart-pmtiles-out <path>] ICAO1 [ICAO2 ...]
//! ```
//!
//! `--nasr-dir` points at a directory containing an unzipped NASR 28-day
//! CSV subscription (`APT_BASE.csv`, `APT_RWY.csv`, `APT_RWY_END.csv`,
//! `FRQ.csv`). When given, real runway surface type and airport
//! communication frequencies are merged in on top of the CIFP-derived
//! data — CIFP alone has neither.
//!
//! `--chart-geotiff`/`--chart-pmtiles-out` run a source chart GeoTIFF
//! through `ff_charts::geotiff_to_pmtiles` and add a matching
//! `chart_catalog` row whose `tile_url` is the output filename at the
//! site root — i.e. this assumes some static server will serve the
//! output directory. Both flags are required together; omit both to
//! build a bundle with no chart imagery.
use ff_etl::bundle::{build_bundle, BundleSource, ChartSource};
use std::collections::HashSet;
use std::env;
use std::path::PathBuf;

struct Args {
    cifp_path: String,
    output_path: String,
    nasr_dir: Option<String>,
    chart_geotiff: Option<String>,
    chart_pmtiles_out: Option<String>,
    icaos: HashSet<String>,
}

fn parse_args() -> Args {
    let raw: Vec<String> = env::args().collect();
    if raw.len() < 4 {
        eprintln!(
            "usage: build_demo_bundle <cifp-file> <output.sqlite> [--nasr-dir <dir>] \
             [--chart-geotiff <path> --chart-pmtiles-out <path>] ICAO1 [ICAO2 ...]"
        );
        std::process::exit(1);
    }
    let cifp_path = raw[1].clone();
    let output_path = raw[2].clone();
    let mut nasr_dir = None;
    let mut chart_geotiff = None;
    let mut chart_pmtiles_out = None;
    let mut icaos = HashSet::new();
    let mut rest = raw[3..].iter().peekable();
    while let Some(arg) = rest.next() {
        if arg == "--nasr-dir" {
            nasr_dir = rest.next().cloned();
        } else if arg == "--chart-geotiff" {
            chart_geotiff = rest.next().cloned();
        } else if arg == "--chart-pmtiles-out" {
            chart_pmtiles_out = rest.next().cloned();
        } else {
            icaos.insert(arg.to_uppercase());
        }
    }
    Args {
        cifp_path,
        output_path,
        nasr_dir,
        chart_geotiff,
        chart_pmtiles_out,
        icaos,
    }
}

fn main() {
    let args = parse_args();

    let chart = match (&args.chart_geotiff, &args.chart_pmtiles_out) {
        (Some(geotiff_path), Some(pmtiles_out)) => {
            let pmtiles_out = PathBuf::from(pmtiles_out);
            // Site-root-relative, for a static server (e.g. Vite) serving
            // the output directory — unlike the real pipeline, which
            // stores ff-api's /bundles/<cycle>/chart.pmtiles route here.
            let tile_url = format!(
                "/{}",
                pmtiles_out
                    .file_name()
                    .expect("--chart-pmtiles-out must be a file path")
                    .to_string_lossy()
            );
            Some(ChartSource {
                id: "demo-sectional".to_string(),
                geotiff_path: PathBuf::from(geotiff_path),
                pmtiles_out,
                cycle_id: "demo".to_string(),
                name: "Demo Sectional Excerpt".to_string(),
                tile_url,
                kind: ff_charts::ChartKind::Sectional,
            })
        }
        (None, None) => None,
        _ => {
            eprintln!("--chart-geotiff and --chart-pmtiles-out must be given together");
            std::process::exit(1);
        }
    };

    let source = BundleSource {
        cifp_path: PathBuf::from(&args.cifp_path),
        nasr_dir: args.nasr_dir.map(PathBuf::from),
        chart,
        icaos: Some(args.icaos),
    };

    let stats =
        build_bundle(&source, &PathBuf::from(&args.output_path)).expect("failed to build bundle");

    println!(
        "wrote {} airports, {} runways, {} frequencies, {} procedures, {} transitions, {} legs, \
         {} navaids, {} waypoints, {} chart to {}",
        stats.airports,
        stats.runways,
        stats.frequencies,
        stats.procedures,
        stats.transitions,
        stats.legs,
        stats.navaids,
        stats.waypoints,
        stats.has_chart as u8,
        args.output_path,
    );
}
