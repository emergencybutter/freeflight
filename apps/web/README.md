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

The map has no chart imagery yet — `ff-charts`' GeoTIFF→PMTiles pipeline
exists and works, but no real FAA chart has been run through it (see
`/TODO.md`), so the map is a plain background with airport points and
real runway centerlines (from CIFP's threshold coordinates) drawn as a
GeoJSON overlay. Clicking an airport point on the map selects it, same
as clicking it in the list.

## Running

```sh
npm install
npm run dev
```

## Regenerating the demo bundle

`public/demo-cycle.sqlite` is checked in (small, ~400KB) so `npm run dev`
works without any Rust tooling. To rebuild it from real source data:

```sh
cargo run -p ff-etl --example build_demo_bundle -- \
  <path-to-a-FAACIFP18-file> apps/web/public/demo-cycle.sqlite \
  --nasr-dir <path-to-an-unzipped-NASR-28-day-CSV-subscription> \
  KSFO KOAK KSJC KPAO KHWD
```

`--nasr-dir` is optional — omit it (and the flag) to build from CIFP
alone, which still gives real airports/runways/procedures, just without
surface type or frequencies.

## Known gotcha

`sql.js` must **not** be excluded from Vite's `optimizeDeps` — its
`dist/*.js` builds are CJS/UMD with no real ESM `default` export, and
Vite's esbuild pre-bundling is what synthesizes that interop. Excluding
it (the usual advice for wasm-heavy packages) breaks the dev server with
a `does not provide an export named 'default'` error.
