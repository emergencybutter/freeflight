# TODO

## Get real chart imagery flowing

`ff-charts`' `geotiff_to_pmtiles` pipeline (gdalwarp → gdal_translate →
gdaladdo → pure-Rust MBTiles→PMTiles repack) is implemented and tested,
but only against a synthetic GeoTIFF — FAA's chart-imagery domains
(aeronav.faa.gov) are blocked from this environment's egress, the same
way FAA's CIFP/NASR domains were before those got fixed with
user-provided files.

To close this gap:

- Fetch or have the user upload a real FAA VFR raster chart GeoTIFF
  (sectional or TAC) from https://aeronav.faa.gov's raster chart
  products, or a mirror.
- Run it through `geotiff_to_pmtiles` for real, confirm the output
  PMTiles renders correctly (visually, in a MapLibre viewer once #2
  below lands).
- Wire a real chart into the demo bundle / catalog (`ChartCatalogEntry`)
  and into `apps/web`, following the same pattern used for CIFP/NASR
  data in `build_demo_bundle.rs`.
- Check chart licensing/attribution requirements for redistribution
  (FAA charts are public domain, but confirm before bundling one into
  the repo — likely want a small cropped extract rather than a full
  chart to keep repo size down).
