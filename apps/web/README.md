# freeflight web client

Vite + React + TypeScript. A read-only viewer: a MapLibre GL JS map
(airports + runway centerlines) above an airport list → runways/
procedures → transitions/legs, backed by `sql.js` reading a SQLite
bundle. That bundle is synced from `ff-api`'s published cycle when
reachable (checksum-verified, cached in IndexedDB — see `src/sync.ts`),
falling back to a previously-cached cycle or the static
`public/demo-cycle.sqlite` otherwise. A status line at the top of the
page always says which.

This is a stand-in for part of the real architecture described in
DESIGN.md §5/§8/§10 — `ff-wasm` for shared planning/parsing logic isn't
wired up yet, and storage is sql.js + IndexedDB rather than the
sqlite-wasm + OPFS DESIGN.md specifies (a deliberate, smaller-scope
stand-in: same "persists across reloads, verified before use" behavior
without the bigger migration off sql.js — see `src/sync.ts`'s doc
comment). The cycle-sync and data-pipeline parts otherwise work end to
end with real data: real FAA CIFP records (parsed by `ff-cifp`) merged
with real FAA NASR records (parsed by `ff-nasr`, for runway surface
type and airport communication frequencies — CIFP alone has neither),
stored via the `ff-storage` schema, rendered in a browser.

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

Live weather needs `services/ff-api` running separately — it's a Rust
process, not part of `npm run dev`:

```sh
cargo run -p ff-api
```

Defaults to `http://localhost:8080`; the web client points there by
default too (override with `VITE_FF_API_BASE_URL`). Without it running,
the weather section just shows a "couldn't reach ff-api" hint and the
map's weather overlays silently stay empty (each logs a `console.warn`
rather than failing) — the rest of the app (charts, airports,
procedures) is unaffected, since that's all read from the static
SQLite bundle. `ff-api` in turn proxies the real `aviationweather.gov`
Data API — no API key needed.

With `ff-api` running, the map also shows:
- Airport markers colored by METAR flight category (green VFR, blue
  MVFR, red IFR, magenta LIFR).
- Current G-AIRMET (turbulence/icing/IFR/mountain obscuration/freezing
  level) and SIGMET (convective) polygons/lines for the whole US —
  visible when zoomed out past the Bay Area demo region.
- A winds-aloft label at 9,000 ft for any demo airport the NWS forecast
  actually covers (only KSFO, in this demo's 5 airports — small GA
  fields generally aren't winds-aloft reporting points, which is
  correct behavior to show, not a gap).

## Syncing a cycle

The status line at the top of the page (`"Cycle 2026-07-09 · synced
from ff-api"` / `"... offline (cached copy ...)"` / `"Using bundled
demo data ..."`) says which cycle bundle is actually loaded and where
it came from. `ff-api` only has a cycle to serve once `ff-etl` has
published one:

```sh
cargo run -p ff-etl    # fetches + publishes a real cycle into FF_ETL_DATA_DIR (default data/)
cargo run -p ff-api    # serves it at /cycles/latest, /cycles/:id/bundle.sqlite
```

Without that (or without `ff-api` running at all), the client falls
back to a previously-synced cycle cached in IndexedDB, and from there
to the static bundled `public/demo-cycle.sqlite` — same "degrade, don't
break" pattern as the weather features above. See `src/sync.ts` for the
checksum-verification and fallback logic.

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
