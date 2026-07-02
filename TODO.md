# TODO

## Project state (handoff notes)

Written for a fresh session/agent picking this up cold. Full rationale
for every decision below is in `git log` — each commit message explains
what was verified, what broke, and why a given approach was chosen, in
more depth than this summary. `DESIGN.md` is the original architecture
doc; treat it as directional, not gospel — a few of its details (e.g.
which record type is "Airway" vs "Waypoint" in ARINC 424) turned out
wrong once checked against real data, and the code is the corrected
source of truth.

### What's real vs. scaffolded

Built and validated against real FAA/NOAA data end to end:
- `ff-cifp` (ARINC 424 CIFP parser): airports, runways, SID/STAR/
  approach procedures, VHF/NDB navaids, waypoints. Validated against a
  real, current FAACIFP18 cycle file (0% error rate across every record
  category — see `crates/ff-cifp/tests/real_cifp.rs`, run with
  `FF_CIFP_TEST_FILE=<path> cargo test -p ff-cifp --test real_cifp --
  --ignored --nocapture`).
- `ff-nasr` (FAA NASR CSV parser): airports, runways, runway ends,
  communication frequencies. Validated against a real 28-day NASR CSV
  subscription.
- `ff-charts` (GeoTIFF → PMTiles tiling): `geotiff_to_pmtiles` shells
  out to real GDAL CLI tools (gdalwarp/gdal_translate/gdaladdo), then a
  pure-Rust step repacks MBTiles into PMTiles. Validated against a real
  FAA sectional GeoTIFF (see "Chart imagery" below).
- `apps/web`: a real (if minimal) MapLibre-based viewer reading a
  SQLite cycle bundle via `sql.js` — airport list, runway/procedure
  detail, a map with real chart imagery, airport markers, runway
  centerlines, and procedure-leg paths.
- `ff-weather` (aviationweather.gov client): `Metar`/`Taf` deserialization
  validated against real captured METAR/TAF responses (see "ff-weather"
  below) — two type bugs found and fixed.

Scaffolded but not validated against real/live data:
- `ff-notam` (FAA NOTAM client): the API it originally targeted turned
  out to be retired; rewritten against its replacement and confirmed
  *reachable*, but the actual NOTAM record shape is still unvalidated —
  no credentials available in this environment (see "ff-notam" below).
- `ff-planning` (nav-log/route math) and `ff-postflight` (track
  analysis) — unit-tested with synthetic inputs, no real flight data.
- `services/ff-api` (axum backend) and `services/ff-etl` (batch cycle
  builder, distinct from the `build_demo_bundle` example) — skeletons
  only, not run against real traffic or a real scheduled pipeline.
- `ff-sync` (client-side cycle bundle sync) — skeleton only.
- `apps/android` — just a placeholder `README.md`, no Kotlin project.
- No route-planning UI in `apps/web` yet (DESIGN.md's nav-log/flight-
  plan builder) — the client is still read-only.

### Environment/network quirks (read before assuming a domain is blocked)

This sandbox's egress goes through a policy-enforcing proxy. Confirmed
open: `registry.npmjs.org`, `crates.io` (needs a `User-Agent` header),
`raw.githubusercontent.com`. Confirmed blocked at various points this
session: `aeronav.faa.gov`, `www.faa.gov`, `github.com` (web UI),
`api.github.com` (direct, non-MCP), most general web domains (even a
user's personal site was blocked once). **However**, a later session
successfully downloaded a real FAA sectional directly from
`aeronav.faa.gov` — so this policy may vary by session/environment
configuration rather than being a fixed blocklist. Don't assume a
domain is blocked without testing in the current session; don't assume
one is open based on an earlier session either. When a needed domain
is blocked, the fallback that's worked repeatedly: ask the user to
upload the file directly (this is how the real CIFP and NASR data
arrived) rather than trying to route around the policy.

### Validation checklist for any change

```sh
cargo fmt --all
cargo check --workspace
cargo clippy --workspace --all-targets
cargo test --workspace
cd apps/web && npm run build
```

For UI changes, there's no `playwright` npm dependency in the repo —
use the globally-installed one (`/opt/node22/lib/node_modules/playwright`,
Chromium at `/opt/pw-browsers/chromium`) via a throwaway script, the
pattern used throughout this session's browser verifications.

### Notable gotchas already paid for (don't rediscover these)

- **ARINC 424 record classification**: CIFP Section `E` subsection `A`
  is Waypoint and subsection `R` is Airway — the reverse of what the
  letters suggest, and what an earlier unverified pass assumed. Fixed
  in `crates/ff-cifp/src/record.rs`.
- **ARINC 424 navaid/waypoint region field**: two similarly-named
  columns exist ("ICAO Code" and "ICAO Code (2)"); the real region is
  in the second one. The first is blank except on airport-associated
  records.
- **VOR vs. NDB frequency encoding**: same 5-digit field width, but VOR
  is hundredths-of-a-MHz and NDB is tenths-of-a-kHz — using the wrong
  scale silently produces a plausible-looking wrong number.
- **Standalone DME/TACAN navaids** (no VOR component) leave the primary
  lat/lon fields blank and put real coordinates in the DME lat/lon
  fields instead; the NAVAID Class field (`VD`/`VT`/`V `/` D`/` I`/` T`/
  ` M`) reliably distinguishes VOR/VOR-DME/VORTAC/DME/ILS-DME/TACAN —
  see `navaid_class_type` in `crates/ff-cifp/src/extract.rs`.
- **MBTiles vs. PMTiles row numbering**: MBTiles uses TMS convention
  (row 0 = south), PMTiles/XYZ uses row 0 = north. Getting this backwards
  silently renders every chart tile upside down — see
  `crates/ff-charts/src/mbtiles.rs`.
- **`sql.js` and Vite**: must NOT be excluded from `optimizeDeps` (the
  usual advice for wasm-heavy packages) — its CJS/UMD build needs
  esbuild's pre-bundling to get a usable `default` export.
- **PMTiles in MapLibre**: needs the `pmtiles` npm package's `Protocol`
  registered via `maplibregl.addProtocol("pmtiles", ...)` before any
  `pmtiles://` source URL resolves.

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

## ff-weather — validated against live data, two bugs fixed

Deserialized real `aviationweather.gov` METAR/TAF responses (captured
live, checked in as test fixtures under `crates/ff-weather/tests/`)
against the existing `Metar`/`Taf` structs. METAR matched as-is. TAF
did not:

- `Taf::issue_time` was typed `i64` (matching the other TAF timestamp
  fields, which really are epoch integers) but the live API returns
  `issueTime` as an ISO 8601 string. Fixed: now `String`.
- `TafForecastPeriod::wdir` was typed `Option<i32>`, but wind direction
  can be the string `"VRB"` in TAF forecast periods too (confirmed on
  live KATL/KDEN/KMIA TAFs with `PROB`/`TEMPO` groups), same as
  `Metar::wdir` already handled. Fixed: now `Option<serde_json::Value>`.

`crates/ff-weather/tests/real_weather.rs` pins both fixes with checked-in
real-response fixtures (run by default, no network) plus an opt-in
`--ignored` test that hits the live API through the actual
`WeatherClient` methods.

## ff-notam — old API retired, client rewritten (unvalidated)

Went looking for real NOTAM data to validate against and found the API
`ff-notam` targeted (`external-api.faa.gov/notamapi/v1/notams`) no
longer exists — confirmed live, it now 404s with "No context-path
matches the request URI" on FAA's gateway. This matches (and confirms)
DESIGN.md §12's old prediction that this endpoint was the flakiest
dependency in the project.

FAA replaced it with the NOTAM Management Service (NMS) at
`api-nms.aim.faa.gov`. Rewrote `ff-notam` to target it:

- New base URLs (`DEFAULT_AUTH_URL`, `DEFAULT_API_BASE_URL`), OAuth2
  `client_credentials` flow (`POST /v1/auth/token` with HTTP Basic
  auth, cached Bearer token) instead of the old static
  `client_id`/`client_secret` headers.
- Credentials are no longer self-service — request a
  `client_id`/`client_secret` pair by emailing NOTAMS@faa.gov.
- Response format is GeoJSON/AIXM now, not the old `coreNOTAMData`
  shape. Still returned as raw `serde_json::Value` (unchanged
  design choice) rather than typed structs, since the actual NOTAM
  record shape is unvalidated.

Confirmed live (both the token endpoint and `/nmsapi/notams` respond
with real structured errors, not connection failures or generic
gateway 404s — see `crates/ff-notam/tests/real_notam.rs`, opt-in), but
**not validated against a real NOTAM response** — this environment has
no `client_id`/`client_secret` and self-service signup no longer
exists. Base URLs and request shapes were reverse-engineered from a
third-party client (`faa-nms-api` on npm), not FAA's own docs.

Next step once credentials exist (request via NOTAMS@faa.gov): call
`fetch_notams_raw` for a real airport, inspect `data.geojson[]`, and
model it as typed structs the same way this pass fixed `ff-weather`.
