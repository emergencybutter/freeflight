# freeflight web client

Vite + React + TypeScript. Two views, toggled from the top bar: a
read-only **Map** (MapLibre GL JS — chart imagery + airports + runway
centerlines + weather overlays, airport search → runways/procedures →
transitions/legs) and a **Flight Plan** view (aircraft profile, route
builder, nav log, basic weight & balance — DESIGN.md §9.3).

The Map view is the thin, connectivity-assuming client of DESIGN.md §8:
it has **no local database and no offline mode** — every view fetches
what it needs from `ff-api`'s `/data/*` JSON endpoints (§4.1), and chart
imagery streams from `ff-api`'s `/bundles/*` files via PMTiles range
requests. If `ff-api` is unreachable the app says so plainly and stops;
offline is Android's job.

The Flight Plan view runs `ff-planning`'s nav-log/W&B math client-side
via `ff-wasm` (`npm run dev`/`build` compile it automatically — see
`build:wasm`/`predev`/`prebuild` in `package.json`; requires `wasm-pack`
on `PATH`, `cargo install wasm-pack`). It's session-only: aircraft
profiles and routes live in React state and are lost on refresh — the
web client still has no local database (§8), and cross-device
persistence is an explicit Phase 4 concern, not this pass.

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
  flight category, plus every current FAA sectional nationwide (tiled
  by `ff-etl`, served by `ff-api`), runway centerlines, Class B/C/D +
  Special Use Airspace boundaries, CONUS-wide G-AIRMET/SIGMET polygons,
  and winds-aloft labels for in-view airports the NWS bulletin covers.
- The detail panel shows live METAR/TAF plus runways, frequencies, and
  SID/STAR/approach procedures with full leg tables. Selecting a
  procedure draws its path on the map — solid for the approach, dashed
  for the missed segment, with waypoint markers/altitude restrictions
  and rounded rather than angular turns.
- The Flight Plan view: build a route by searching airports, set an
  aircraft profile, and get an auto-computed nav log (no wind
  correction yet); fill in the profile's weight & CG limits to also get
  a basic single-envelope weight & balance check.

Weather overlays degrade independently (each logs a `console.warn` if
its fetch fails); the cycle data itself does not — no `ff-api`, no app,
by design.
