# TODO

## Chart imagery — done

Real chart imagery now flows end to end:

- `ff-charts`' `geotiff_to_pmtiles` pipeline (gdalwarp → gdal_translate
  → gdaladdo → pure-Rust MBTiles→PMTiles repack).
- `build_demo_bundle --chart-geotiff <path> --chart-pmtiles-out <path>`
  runs a GeoTIFF through that pipeline and adds a matching
  `chart_catalog` row.
- `apps/web`'s `MapView` renders a `chart_catalog` entry as a PMTiles
  raster layer (via the `pmtiles` package's MapLibre protocol handler),
  underneath the airport/runway/procedure overlays.

The checked-in demo bundle now includes a real chart: the FAA San
Francisco sectional (cycle 2026-07-09, downloaded from
`aeronav.faa.gov` — that domain turned out not to be blocked from this
environment after all), cropped with `gdalwarp` to the demo airports'
bounding box (~37.2–37.85N, ~122.55–121.75W) before running through the
pipeline, keeping `apps/web/public/demo-chart.pmtiles` to ~12MB instead
of bundling the full multi-hundred-MB regional chart. Verified visually
in a real browser: correct sectional colors (palette expanded to RGB
before the pipeline's bilinear resample, avoiding the color-table
corruption bilinear would otherwise cause), real runway centerlines and
airport markers correctly z-ordered above the chart tiles.

FAA charts are public domain; no attribution/licensing blocker.

## Possible follow-ups

- Only one sectional is bundled (San Francisco, covering the 5 demo
  airports). Expanding demo coverage to other regions would need
  additional cropped GeoTIFFs run through the same pipeline.
- The crop bounding box is hand-picked around the 5 demo ICAOs; no
  tooling yet derives it automatically from the airport list.
