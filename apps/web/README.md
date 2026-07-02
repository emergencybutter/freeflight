# freeflight web client

Vite + React + TypeScript. A read-only viewer: a MapLibre GL JS map
(airports + runway centerlines) above an airport list → runways/
procedures → transitions/legs, backed by `sql.js` reading a static
SQLite bundle (`public/demo-cycle.sqlite`).

This is a stand-in for the real architecture described in DESIGN.md
§5/§10 (`ff-wasm` for shared planning/parsing logic, OPFS-backed local
storage synced from `ff-api` cycle bundles) — that part isn't wired up
yet. The current UI proves the data pipeline end to end: real FAA CIFP
records (parsed by `ff-cifp`) merged with real FAA NASR records (parsed
by `ff-nasr`, for runway surface type and airport communication
frequencies — CIFP alone has neither), stored via the `ff-storage`
schema, rendered in a browser.

The map renders chart imagery as a PMTiles raster layer (via the
`pmtiles` package's MapLibre protocol handler) whenever the loaded
cycle bundle's `chart_catalog` table has an entry — plain background
otherwise. The checked-in demo bundle has one: a real FAA San Francisco
sectional (cycle 2026-07-09), cropped to the demo airports' area and
run through `ff-charts`' GeoTIFF→PMTiles pipeline via
`build_demo_bundle --chart-geotiff --chart-pmtiles-out`
(`public/demo-chart.pmtiles`, ~12MB). Airport points and real runway
centerlines (from CIFP's threshold coordinates) draw as a GeoJSON
overlay above the chart layer. Clicking an airport point on the map
selects it, same as clicking it in the list.

## Running

```sh
npm install
npm run dev
```

Live METAR/TAF for the selected airport (in the airport detail panel)
needs `services/ff-api` running separately — it's a Rust process, not
part of `npm run dev`:

```sh
cargo run -p ff-api
```

Defaults to `http://localhost:8080`; the web client points there by
default too (override with `VITE_FF_API_BASE_URL`). Without it running,
the weather section just shows a "couldn't reach ff-api" hint — the
rest of the app (charts, airports, procedures) is unaffected, since
that's all read from the static SQLite bundle. `ff-api` in turn proxies
the real `aviationweather.gov` Data API — no API key needed.

## Regenerating the demo bundle

`public/demo-cycle.sqlite` is checked in (small, ~400KB) so `npm run dev`
works without any Rust tooling. To rebuild it from real source data:

```sh
cargo run -p ff-etl --example build_demo_bundle -- \
  <path-to-a-FAACIFP18-file> apps/web/public/demo-cycle.sqlite \
  --nasr-dir <path-to-an-unzipped-NASR-28-day-CSV-subscription> \
  --chart-geotiff <path-to-a-FAA-VFR-chart-GeoTIFF> \
  --chart-pmtiles-out apps/web/public/demo-chart.pmtiles \
  KSFO KOAK KSJC KPAO KHWD
```

`--nasr-dir` is optional — omit it (and the flag) to build from CIFP
alone, which still gives real airports/runways/procedures, just without
surface type or frequencies.

`--chart-geotiff`/`--chart-pmtiles-out` are optional and must be given
together — they run the source GeoTIFF through `ff-charts`'
`geotiff_to_pmtiles` (needs `gdalwarp`/`gdal_translate`/`gdaladdo` on
`PATH`) and add a `chart_catalog` row pointing at the resulting file.
`--chart-pmtiles-out` should point somewhere under `apps/web/public/`
so Vite serves it statically at the same path relative to the site
root.

## Known gotcha

`sql.js` must **not** be excluded from Vite's `optimizeDeps` — its
`dist/*.js` builds are CJS/UMD with no real ESM `default` export, and
Vite's esbuild pre-bundling is what synthesizes that interop. Excluding
it (the usual advice for wasm-heavy packages) breaks the dev server with
a `does not provide an export named 'default'` error.
