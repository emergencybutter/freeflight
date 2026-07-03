# freeflight web client

Vite + React + TypeScript. A read-only viewer: a MapLibre GL JS map
(chart imagery + airports + runway centerlines + weather overlays)
above an airport list → runways/procedures → transitions/legs.

This is the thin, connectivity-assuming client of DESIGN.md §8: it has
**no local database and no offline mode** — every view fetches what it
needs from `ff-api`'s `/data/*` JSON endpoints (§4.1), and chart
imagery streams from `ff-api`'s `/bundles/*` files via PMTiles range
requests. If `ff-api` is unreachable the app says so plainly and stops;
offline is Android's job. (`ff-wasm` for shared planning logic is still
not wired in — that part of §4 remains future work.)

## Running

The web client is unusable without `ff-api`, and `ff-api` has nothing
to serve until `ff-etl` has published a cycle:

```sh
cargo run -p ff-etl    # fetch + build + publish a real cycle (needs GDAL
                       # CLI tools on PATH for the chart step: gdalwarp,
                       # gdal_translate, gdaladdo)
cargo run -p ff-api    # serve it (default :8080)
npm install
npm run dev            # web client (default :5173)
```

The client points at `http://localhost:8080` by default; override with
`VITE_FF_API_BASE_URL`.

What you get with everything running:

- The status line shows the served cycle ("Cycle 2026-07-09 · live from
  ff-api …").
- The map renders the region's real FAA sectional (tiled by `ff-etl`,
  served by `ff-api`), airport markers colored by live METAR flight
  category, runway centerlines, procedure paths, CONUS-wide
  G-AIRMET/SIGMET polygons, and a winds-aloft label at 9,000 ft for
  airports the NWS bulletin covers.
- The detail panel shows live METAR/TAF plus runways, frequencies, and
  SID/STAR/approach procedures with full leg tables.

Weather overlays degrade independently (each logs a `console.warn` if
its fetch fails); the cycle data itself does not — no `ff-api`, no app,
by design.
