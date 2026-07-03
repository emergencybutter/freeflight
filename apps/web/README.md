# freeflight web client

Vite + React + TypeScript. A read-only viewer: a MapLibre GL JS map
(chart imagery + airports + runway centerlines + weather overlays)
above an airport search → runways/procedures → transitions/legs.

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

The client points at ff-api on the page's own hostname, port 8080, by
default; override with `VITE_FF_API_BASE_URL`.

What you get with everything running:

- The status line shows the served cycle ("Cycle 2026-07-09 · live from
  ff-api …").
- A search box covering the whole nationwide cycle (~13k US airports) —
  ident (ICAO/FAA/IATA) or name.
- The map shows every airport in the current view (bbox-queried per
  pan/zoom, hidden when zoomed out past ~zoom 6), colored by live METAR
  flight category, plus the region's real FAA sectional (tiled by
  `ff-etl`, served by `ff-api` — currently San Francisco only), runway
  centerlines, procedure paths, CONUS-wide G-AIRMET/SIGMET polygons,
  and winds-aloft labels for in-view airports the NWS bulletin covers.
- The detail panel shows live METAR/TAF plus runways, frequencies, and
  SID/STAR/approach procedures with full leg tables.

Weather overlays degrade independently (each logs a `console.warn` if
its fetch fails); the cycle data itself does not — no `ff-api`, no app,
by design.
