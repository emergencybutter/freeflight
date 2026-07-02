# TODO

## Get real chart imagery flowing

All the plumbing is now built and tested end to end, just not with a
real FAA chart:

- `ff-charts`' `geotiff_to_pmtiles` pipeline (gdalwarp → gdal_translate
  → gdaladdo → pure-Rust MBTiles→PMTiles repack).
- `build_demo_bundle --chart-geotiff <path> --chart-pmtiles-out <path>`
  runs a GeoTIFF through that pipeline and adds a matching
  `chart_catalog` row.
- `apps/web`'s `MapView` renders a `chart_catalog` entry as a PMTiles
  raster layer (via the `pmtiles` package's MapLibre protocol handler),
  underneath the airport/runway/procedure overlays.

All three were verified together with a synthetic GeoTIFF (visually
confirmed rendering as a raster tile layer in a real browser, correctly
z-ordered and zoom-clamped), the same "prove the mechanism, not the
pixels" approach used to validate `ff-charts` itself. What's missing is
a real chart:

- FAA's chart-imagery domain (aeronav.faa.gov) is blocked from this
  environment's egress, and no small-enough mirror of a real VFR
  sectional/TAC GeoTIFF turned up in a search (unlike the CIFP/NASR
  files, which the user was able to upload directly — ask for one the
  same way if available).
- Once available: `cargo run -p ff-etl --example build_demo_bundle --
  <cifp> apps/web/public/demo-cycle.sqlite --chart-geotiff <path>
  --chart-pmtiles-out apps/web/public/demo-chart.pmtiles ...` (see
  `apps/web/README.md`).
- Check chart licensing/attribution requirements for redistribution
  (FAA charts are public domain, but confirm before bundling one into
  the repo — likely want a small cropped extract rather than a full
  chart to keep repo size down).

