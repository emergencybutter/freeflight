# freeflight — Design Document

Status: Draft v0.1
Scope: Phase 1 (US-only, free data sources)

## 1. Vision

`freeflight` is an electronic flight bag (EFB)-style application, inspired by
ForeFlight, built in Rust with clients for the **web** and **Android**. It
gives pilots fast access to aeronautical procedures, charts, and weather,
plus lightweight flight planning and post-flight analysis — using only
**free, publicly available data sources**, starting with **US airspace
only**.

`freeflight` is a situational-awareness and planning tool. It is **not**
airworthy or certified navigation equipment, and is not a filing/dispatch
system. It supplements paper charts and certified avionics; it does not
replace them. This must be stated clearly in the app (splash/EULA) and in
this document, because pilots will otherwise assume ForeFlight-equivalent
certification.

### 1.1 Goals (Phase 1)

- View current, correctly-georeferenced FAA VFR and IFR charts.
- Browse airports, runways, frequencies, navaids, and instrument
  procedures (SIDs, STARs, approaches) sourced from the FAA CIFP
  (coded in ARINC 424 format).
- View current METAR/TAF/NOTAM/AIRMET-SIGMET for a route or airport.
- Build a simple route (airport → fixes/airways → airport), see distance,
  course, ETE, fuel burn from a basic aircraft profile, and a text
  briefing.
- Log a flight's GPS track (recorded on-device) and review it after
  landing: route flown vs. planned, altitude/speed profile, landings
  detected, taxi/flight time split.
- **Android** works fully offline once the current data cycle has been
  downloaded — it's the in-cockpit device, so it's the one that has to
  work with no signal (§8). **Web** assumes connectivity; it's a
  planning/briefing companion for before/after the flight, not something
  relied on airborne with no data.

### 1.2 Non-goals (Phase 1)

- Flight plan filing (FAA/NAS), IFR clearance integration, ADS-B In/Out
  traffic or own-ship display, synthetic vision, weight & balance with
  full CG envelope certification, terrain/obstacle alerting, panel/EFIS
  integration, iOS, non-US airspace, paid data sources (Jeppesen, etc.).
- These are explicitly deferred to later phases (§10) so Phase 1 stays
  shippable.

## 2. Primary Use Cases

1. A pilot planning a VFR cross-country wants to see the sectional, check
   weather along the route, and get a rough time/fuel estimate.
2. A pilot planning an IFR flight wants to pull up the CIFP-coded
   departure/arrival/approach procedures for the airports involved and
   cross-check them against the plate.
3. A pilot in the aircraft (Android phone/tablet in the cockpit, no data
   connection) wants offline access to the last-downloaded charts,
   procedures, and the weather briefing pulled before departure — this
   is squarely the Android app's job; the web client assumes
   connectivity and isn't the tool for this use case (§8).
4. A pilot who just landed wants to see their track over the chart, how
   many landings they logged, and basic flight-time numbers for the
   logbook.

## 3. Data Sources (free, US, Phase 1)

All primary sources are US federal government data and are public domain
under 17 U.S.C. §105 (no copyright restriction), though attribution and
"not for navigation" disclaimers are still required by FAA terms of use
for some products.

| Data | Source | Format | Update cycle |
|---|---|---|---|
| Coded instrument flight procedures (SIDs, STARs, approaches, airways, navaids, waypoints) | FAA CIFP | ARINC 424 fixed-width records | 28-day AIRAC cycle |
| Airport/facility directory (runways, frequencies, remarks, services) | FAA NASR subscription | Fixed-width / CSV | 28-day AIRAC cycle |
| VFR charts (Sectional, TAC, Helicopter) | FAA digital raster charts | GeoTIFF | 56-day cycle |
| IFR charts (Enroute Low/High, Area) | FAA digital raster charts | GeoTIFF | 56-day cycle |
| Approach plates (visual reference) | FAA d-TPP | PDF, geo-referenced | 28-day cycle |
| Airspace boundaries (Class B/C/D, SUA, MOA) | FAA NASR shapefiles | Shapefile/CSV | 28-day cycle |
| Obstacles | FAA Digital Obstacle File (DOF) | Fixed-width | 56-day cycle |
| METAR / TAF / PIREP | aviationweather.gov Data API | JSON/XML | real-time |
| AIRMET / SIGMET / G-AIRMET, winds/temps aloft | aviationweather.gov Data API | JSON/XML/GeoJSON | real-time |
| NOTAMs | FAA NOTAM Management Service (NMS) API (`api-nms.aim.faa.gov`) | GeoJSON/AIXM | real-time |
| Terrain elevation | USGS 3DEP / SRTM1 | GeoTIFF (public domain) | static |
| GPS track (flight recording) | On-device GPS (browser Geolocation / Android FusedLocationProvider) | — | live |

Notes:
- The user's spec said "ARINC 425"; the correct standard for CIFP is
  **ARINC 424** ("Navigation System Database"). The FAA's CIFP is the
  ARINC 424-formatted product this design targets.
- No scraping of paid providers (Jeppesen, Garmin Pilot, ForeFlight's own
  feeds). If a free source disappears or changes terms, that feature is
  disabled rather than replaced with a workaround that violates ToS.
- Own GPS track is the only "traffic"/track data in Phase 1 — no
  ADS-B/FlightAware/ADSBExchange dependency, which keeps post-flight
  analysis usable with zero external accounts.

## 4. Architecture Overview

```
                         ┌─────────────────────────┐
                         │   Data Pipeline (batch)  │
                         │  ff-etl (Rust, cron)     │
                         │  downloads FAA/NOAA data │
                         │  each AIRAC/56-day cycle │
                         └────────────┬─────────────┘
                                      │ publishes
                                      ▼
                         ┌─────────────────────────┐
                         │  Object storage / CDN     │
                         │  cycle.sqlite (procedures,│
                         │  airports, airspace)      │
                         │  charts.pmtiles           │
                         └────────────┬─────────────┘
                                      │ HTTPS (range requests,
                                      │ resumable download)
                                      ▼
                         ┌───────────────────────────┐
                         │       ff-api server        │
                         │       (axum, Rust)         │
                         │  weather/NOTAM proxy+cache │
                         │  cycle bundle hosting      │
                         │  JSON data queries (web)   │
                         └─────────────┬───────────────┘
                    ┌───────────────────┴───────────────────┐
                    │ JSON per view                          │ full cycle bundle,
                    │ (assumes connectivity)                 │ checksum-verified,
                    ▼                                         ▼ cached for offline use
          ┌───────────────────────┐               ┌───────────────────────┐
          │      Web client        │               │    Android client      │
          │  React/TS + WASM core  │               │  Kotlin/Compose +       │
          │  (ff-core) + MapLibre  │               │  ff-core via UniFFI +   │
          │  GL JS. No local DB —  │               │  MapLibre. Owns a local │
          │  fetches per view.     │               │  SQLite copy (offline). │
          └───────────────────────┘               └───────────────────────┘
```

Shared Rust "core" logic, thin native UI per platform. Maps are the
hardest part of this app to get right, so we do **not** try to write a
Rust GUI/map renderer from scratch or via a Rust-native GUI framework.
Instead:

- **`ff-core`** (pure Rust, `no_std`-friendly where practical): domain
  models, ARINC 424/NASR parsers, route/planning math, track analysis,
  storage schema. No I/O beyond trait-based ports.
- Compiled to **WebAssembly** (`wasm-bindgen`) for the web client, and to
  a native library exposed via **UniFFI** (Kotlin bindings) for Android.
  Same business logic, two thin bindings — no duplicated flight-planning
  or ARINC 424 parsing code between platforms. The web binding
  (`ff-wasm`) does **not** include `ff-sync` — the web client has
  nothing local to sync (§8).
- UI and maps are platform-native: **MapLibre GL JS** on web, **MapLibre
  Android SDK** on Android. Both consume the same **PMTiles**/**MBTiles**
  chart tiles and the same GeoJSON procedure/airspace overlays produced
  by the data pipeline — so the map styling is the only thing duplicated,
  and even that can share a MapLibre style JSON. Android reads its
  GeoJSON overlays from its local cycle SQLite; web reads them from
  `ff-api`'s JSON responses (§8, §9.1).
- A small **`ff-api`** backend (axum) proxies weather/NOTAM APIs (adds
  caching, works around any CORS/rate-limit issues on those government
  endpoints), serves the versioned cycle bundles produced by the ETL job
  (for Android's offline sync), and answers ad hoc airport/runway/
  procedure/chart-catalog queries as JSON for the web client. **Android
  is the offline-first client** — it owns a local SQLite copy of the
  cycle, synced via `ff-sync`. **The web client holds no local
  persistent store** and assumes connectivity to `ff-api`; a real
  ARTCC-scale cycle would be too large to want to ship whole to a
  browser tab anyway (§8).

This mirrors the architecture used by apps like 1Password and Mozilla
products: one Rust core, native UI shells, so the map/GPU-heavy surface
gets a mature, purpose-built renderer instead of a young Rust GUI stack.

## 5. Workspace / Crate Layout

```
freeflight/
  crates/
    ff-core/          domain types: Airport, Runway, Navaid, Fix, Airway,
                       Procedure (SID/STAR/Approach + legs), AirspaceVol,
                       Frequency — plus Cycle/AIRAC metadata
    ff-cifp/           ARINC 424 record parser → ff-core types
    ff-nasr/            FAA NASR fixed-width/CSV parser → ff-core types
    ff-charts/          GeoTIFF → tile pipeline glue, chart index/catalog
    ff-weather/         aviationweather.gov client + METAR/TAF/SIGMET
                        decoders
    ff-notam/           FAA NOTAM API client + parser
    ff-planning/        route builder, dead-reckoning nav log, fuel/time
                        estimate, simple weight & balance
    ff-postflight/      GPS track ingestion, phase-of-flight detection
                        (taxi/climb/cruise/descent/landing), logbook entry
                        generation
    ff-storage/         SQLite schema + migrations (sqlx), used by the
                        ETL job, ff-api (server-side queries for the web
                        client), and (via uniffi) the Android client
    ff-sync/            cycle bundle download/verify/apply, delta logic
                        — Android-only; the web client has no local
                        cycle copy to sync (§8)
    ff-wasm/            wasm-bindgen bindings over ff-core/ff-planning/
                        ff-postflight for the web client (no ff-sync
                        binding — nothing local to sync, see §8)
    ff-uniffi/          UniFFI bindings (Kotlin) for the Android client
  services/
    ff-api/             axum server: weather/NOTAM proxy+cache, cycle
                        bundle hosting (Android's offline sync), JSON
                        data query endpoints (airports/runways/
                        procedures/charts, for the web client — §8),
                        health/version endpoints
    ff-etl/              batch job: fetch FAA/NOAA sources on their
                        publication cycle, parse via ff-cifp/ff-nasr/
                        ff-charts, emit versioned SQLite + PMTiles bundle
  apps/
    web/                React + TypeScript, MapLibre GL JS, loads
                        ff-wasm for shared planning/parsing logic. No
                        local database or offline cache — fetches
                        airport/procedure/chart/weather data from ff-api
                        per view and assumes connectivity (§8).
    android/            Kotlin, Jetpack Compose, MapLibre Android SDK,
                        loads ff-uniffi, on-device SQLite (via the same
                        ff-storage schema, driven through Rust) — the
                        one offline-capable client (§8); syncs cycles
                        via ff-uniffi bindings over ff-sync
  docs/
    DESIGN.md            (this file)
```

Rationale for the split: `ff-cifp`/`ff-nasr`/`ff-charts` are pure parsers
with no async/runtime dependencies, so they're trivially unit-testable
against fixture files and reusable from both the ETL job and (if a client
ever needs to parse a locally-supplied CIFP file) the client core.

## 6. Data Model (`ff-core` / `ff-storage`, simplified)

```
airac_cycle(id, effective_date, source_version)

airport(icao, iata, faa_id, name, lat, lon, elevation_ft, airport_type,
        fuel_types, tower_freq_id, ctaf_freq_id, ...)
runway(id, airport_icao, ident, length_ft, width_ft, surface,
       le_lat, le_lon, he_lat, he_lon, le_heading, he_heading)
frequency(id, airport_icao, kind, freq_mhz, remarks)

navaid(id, ident, type, lat, lon, elevation_ft, freq_khz, ...)
waypoint(id, ident, lat, lon, region)
airway(id, ident, kind)        -- V/J/T routes
airway_leg(airway_id, seq, waypoint_id, min_alt, max_alt)

procedure(id, airport_icao, kind, ident, runway_ident)  -- SID/STAR/APPROACH
procedure_transition(id, procedure_id, ident, kind)     -- enroute/common/approach/missed
procedure_leg(id, transition_id, seq, path_and_term, fix_id, course,
              altitude_constraint, speed_constraint, turn_direction, ...)

airspace(id, name, class, floor_ft, ceiling_ft, geometry_geojson)

chart_catalog(id, name, kind, cycle_id, bbox, tile_url)

-- client-local only (not part of the published cycle bundle):
route_plan(id, name, created_at, aircraft_profile_id)
route_leg(route_plan_id, seq, waypoint_ref, altitude, notes)
flight_track(id, started_at, ended_at, aircraft_profile_id)
track_point(flight_track_id, seq, ts, lat, lon, alt_ft, gs_kt, track_deg)
flight_log_entry(id, flight_track_id, route_plan_id, departure, arrival,
                 total_time, taxi_time, landings, ...)
```

`procedure_leg.path_and_term` follows ARINC 424's leg-type coding (IF, TF,
CF, DF, CA, VA, etc.) directly, so the CIFP parser is a near-literal
translation rather than a lossy simplification — this matters if we ever
need to render procedures with the same fidelity as certified tools.

## 7. Data Pipeline (`ff-etl`)

- Runs on a schedule aligned to FAA's 28-day AIRAC and 56-day chart
  cycles (cron via CI, e.g. GitHub Actions scheduled workflow, or a small
  worker on the same host as `ff-api`).
- Steps: fetch raw CIFP/NASR/DOF/shapefiles/GeoTIFFs → parse with
  `ff-cifp`/`ff-nasr`/`ff-charts` → validate (row counts vs. previous
  cycle, geometry sanity checks) → write a single versioned
  `cycle-YYYY-MM-DD.sqlite` and a matching `charts-YYYY-MM-DD.pmtiles` →
  upload to object storage behind a CDN → flip a `latest` pointer only
  after both artifacts pass validation.
- Clients never talk to FAA/NOAA chart/procedure endpoints directly.
  Android pulls the pre-processed bundle from `ff-api`/CDN and queries it
  locally (offline-capable, §8). The web client never downloads the
  bundle at all — `ff-api` opens/queries it server-side (via `rusqlite`)
  and answers the web client's requests as JSON, since the web client
  assumes connectivity and has nowhere offline-durable to put a whole
  cycle anyway. Either way, this keeps client code simple, makes offline
  bundles reproducible, and isolates the app from upstream format quirks.
- Weather/NOTAM are the exception: those are real-time, so `ff-api` proxies
  them live (with a short cache, e.g. 2–5 min for METAR/TAF, cache-bust on
  new NOTAMs) rather than baking them into the cycle bundle.

## 8. Offline & Sync Strategy

**Android is the only offline-capable client.** The web client assumes
connectivity — it's a planning/briefing tool used before or after a
flight, not something relied on airborne with no signal (§12 already
flagged web's background-GPS limits as an Android-primary concern for
the same underlying reason; this generalizes that call to the whole
client, not just track recording). Consequences:

- **Android** owns a local SQLite database — the same `ff-storage`
  schema opened directly with `rusqlite` inside the UniFFI core. On app
  start (or manually), it checks `ff-api /cycles/latest` for a newer
  cycle id; if found, downloads the new SQLite + PMTiles bundle in the
  background (resumable, range-request friendly) via `ff-sync`, verifies
  a checksum, and swaps it in atomically. The previous cycle stays usable
  until the swap completes, so the app is never mid-download-unusable.
  Charts/procedures/airports work fully offline once a cycle is
  downloaded; so does route planning. Flight tracking (GPS logging) is
  inherently offline-capable.
- **Web** has no local database and no `ff-sync`/`ff-wasm` sync bindings
  — there's nothing to keep offline-durable. It fetches whatever the
  current view needs (airports, runways, procedures, chart tile URLs)
  from `ff-api`'s JSON query endpoints per request. If `ff-api` is
  unreachable, the web app shows that plainly rather than silently
  failing or serving stale data — there's no cached/bundled fallback to
  reach for, by design, not by omission.
- Weather/NOTAM are fetched opportunistically whenever online on both
  platforms, with a "briefing snapshot" concept: the pilot explicitly
  takes a briefing before flight, which freezes the weather/NOTAM data
  used for planning and time-stamps it clearly in the UI ("Briefing
  taken 45 min ago"). On Android this snapshot persists locally so it
  survives going offline after departure. On web it only needs to
  survive the current session/tab — web assumes connectivity anyway, so
  there's no separate durable-persistence story required there.

## 9. Feature Design

### 9.1 Charts & procedures viewer

- MapLibre GL map with layer toggles: VFR sectional/TAC, IFR low/high
  enroute, and a vector overlay of airspace, airports, navaids. On
  Android, drawn from the local cycle SQLite (converted to GeoJSON on
  load). On web, drawn from GeoJSON `ff-api` returns for the current
  view (§8) — same overlay shape either way, different source.
- Airport detail view: runways, frequencies, remarks, and a procedure
  list (SIDs/STARs/approaches) pulled from `procedure`/`procedure_leg`.
- Selecting a procedure draws it on the map (leg-by-leg from
  `procedure_leg`) and optionally overlays the FAA d-TPP plate image
  (PDF rendered client-side, e.g. `pdf.js` on web / `PdfRenderer` on
  Android) for visual cross-check.

### 9.2 Weather & NOTAMs

- Route/airport briefing screen: METAR (raw + decoded), TAF, applicable
  AIRMET/SIGMET polygons drawn on the map, winds/temps aloft along the
  route, and NOTAMs (filterable by relevance: runway/taxiway closures
  first).
- Fully driven by `ff-weather`/`ff-notam` through the `ff-api` proxy;
  decoded METAR/TAF share the same Rust decoder on both clients (compiled
  into `ff-wasm`/`ff-uniffi`) so "what does this METAR mean" logic is
  written once.

### 9.3 Simple flight planning

- Route builder: pick departure/destination airports, add
  fixes/navaids/airways in between (autocomplete against the local cycle
  DB on Android — works offline; against `ff-api` query endpoints on
  web — requires connectivity, §8).
- Per-leg: great-circle/rhumb distance & course from `ff-planning`, ETE
  and fuel burn from a user-defined aircraft profile (cruise TAS, fuel
  burn GPH, optional simple winds-aloft correction using the fetched
  winds data).
- Output: a nav log table and a text briefing the pilot can screenshot or
  print. No filing integration in Phase 1 (see §1.2).
- Weight & balance is **basic** in Phase 1: a single-envelope check
  (empty weight + fuel + pax/bags vs. max gross and a simple forward/aft
  CG limit) per user-entered aircraft profile — not a certified
  multi-envelope tool.

### 9.4 Post-flight analysis

- The app can record a GPS track during flight (foreground service on
  Android with a persistent notification per platform requirements;
  `wakeLock`/background-tab caveats documented for web, since browsers
  throttle background GPS aggressively — Android is the primary target
  for in-flight recording).
- `ff-postflight` takes the raw track and derives: phase-of-flight
  segmentation (ground/taxi/takeoff-roll/airborne/landing) from
  groundspeed + altitude-rate heuristics, landing count (touch-and-go vs.
  full stop, by detecting brief vs. sustained ground contact), total/taxi/
  airborne time, and a simple altitude/speed profile chart.
- Track is drawn over the chart alongside the originally planned route
  for a visual "did I fly what I planned" comparison.
- Output feeds a `flight_log_entry` the pilot can review/edit and export
  (CSV) for their paper or third-party logbook — `freeflight` is not
  itself a logbook system in Phase 1, just a generator of log-worthy
  data.

## 10. Tech Stack Summary

| Layer | Choice | Why |
|---|---|---|
| Core language | Rust (2021 edition) | one implementation of parsing/planning/analysis logic, shared everywhere |
| Web/Android bridge | `wasm-bindgen` (web), `uniffi-rs` (Android/Kotlin) | mature, widely used, avoids hand-written FFI |
| Backend | `axum` + `tokio` | small, well-supported async Rust web framework; matches core language |
| Client DB | SQLite (`rusqlite`; server + Android only) | single schema for ETL, `ff-api`'s server-side queries, and Android's offline copy — no client-side DB in the browser (§8) |
| Chart tiles | PMTiles (served statically, no tile server needed) | cheap to host (single file + HTTP range requests), works well with MapLibre |
| Web map | MapLibre GL JS | open-source, vector+raster, no API key needed |
| Web UI shell | React + TypeScript | large ecosystem for the non-map UI (forms, lists, briefing screens) |
| Android map | MapLibre Android SDK | same rendering engine/style format as web |
| Android UI shell | Kotlin + Jetpack Compose | current native Android standard |
| CI/hosting for ETL | GitHub Actions (scheduled) + object storage/CDN (e.g. S3/Cloudflare R2) | free/cheap tiers sufficient for public-domain data re-hosting |

## 11. Non-functional Requirements

- **Offline-first — Android only** (§8): every Phase 1 feature on
  Android except live weather/NOTAM fetch and cycle updates must work
  with no network. The web client is online-only by design; its
  non-functional bar instead is to fail *visibly* (a clear "can't reach
  the server" state) rather than silently, never to fake offline support
  it doesn't have.
- **Data freshness is explicit, never silent**: UI always shows the AIRAC
  cycle date in use and the age of the last weather briefing. Never let a
  pilot mistake stale data for current.
- **Performance**: full CONUS CIFP+NASR cycle bundle should be tens of
  MB, not hundreds — favor compact binary encodings over verbose
  JSON/text for anything embedded in the SQLite bundle. Chart tiles are
  downloaded per-region-of-interest, not the whole country at once.
- **Testability**: `ff-cifp`/`ff-nasr` parsers get golden-file tests
  against real (public) FAA cycle excerpts; `ff-planning`/`ff-postflight`
  get unit tests with synthetic tracks/routes; `ff-api` gets integration
  tests with mocked upstream weather responses.
- **Legal/compliance**: prominent "not for navigation, VFR/IFR
  supplemental use only" disclaimer; FAA/NOAA data attribution per each
  source's terms of use; no redistribution of any non-public-domain data.

## 12. Open Questions / Risks

1. **Browser background GPS limits**: web is a poor fit for in-flight
   track recording (tab throttling, no reliable background execution).
   Phase 1 web client may need to treat "record a flight" as an
   Android-only feature and let web users *import* a track (GPX) instead.
2. **Web offline scope, decided**: after building a first pass at
   web-side offline sync (checksum-verified IndexedDB cache of the cycle
   bundle, `ff-sync`'s `CycleManifest` reconciled against `ff-api` — see
   TODO.md), decided to scope offline capability to Android only rather
   than maintain two parallel sync/storage stacks. Web becomes a thin
   client that queries `ff-api` per view and assumes connectivity — the
   same reasoning as risk #1's GPS limits, generalized from "recording a
   track" to the whole client. Trades a simpler web architecture (no
   local SQLite/IndexedDB/checksum-verification stack, no `ff-wasm` sync
   bindings) for web being unusable with zero connectivity. `ff-api`
   picks up a responsibility it didn't have before: serving ad hoc
   airport/runway/procedure/chart-catalog JSON queries, not just
   proxying weather/NOTAM and hosting cycle bundles (§4, §7, §8). The
   web-side sync code built under the old design (`apps/web/src/sync.ts`,
   its IndexedDB cache, `db.ts`'s sql.js loader) predates this decision
   and is now slated for removal in favor of direct `ff-api` queries —
   not yet done; tracked in TODO.md.
3. **d-TPP plate rendering**: FAA plates are PDF, not vector — rendering
   quality/perf on low-end Android devices needs a spike before
   committing to in-app PDF rendering vs. "open externally."
4. **ARINC 424 leg coding completeness**: implementing the full leg-type
   state machine (RF legs, vectors-to-final, holding patterns) is
   nontrivial; Phase 1 should scope down to the common leg types (IF, TF,
   CF, DF, CA/CD/VA/VI) and explicitly flag procedures using unsupported
   leg types rather than silently mis-rendering them.
5. **NOTAM API stability**: this prediction proved literally true — the
   FAA NOTAM Search API this section originally named
   (`external-api.faa.gov/notamapi/v1/notams`) was retired outright
   sometime before 2026-07 (confirmed live: it now 404s with "No
   context-path matches the request URI"). Its replacement, the NOTAM
   Management Service (NMS) at `api-nms.aim.faa.gov`, also changed how
   credentials are issued — self-service portal signup is gone, a
   `client_id`/`client_secret` pair must now be requested by emailing
   NOTAMS@faa.gov — and switched from static header credentials to an
   OAuth2 `client_credentials` Bearer-token flow returning GeoJSON/AIXM
   instead of the old `coreNOTAMData` JSON shape. `ff-notam` targets the
   new API as of this note, confirmed reachable (live 401 on both the
   token endpoint and `/nmsapi/notams` with bogus credentials), but the
   actual NOTAM record shape is still unvalidated pending real
   credentials — `ff-api`'s proxy/cache layer should keep assuming this
   is the flakiest dependency.
6. **Chart hosting cost/rights**: re-hosting converted FAA raster charts
   as PMTiles is public-domain data, but bandwidth cost for chart tiles
   at scale should be estimated before wide release (this is why the ETL
   step produces a single static-hostable file format rather than
   standing up a tile server).

## 13. Roadmap

- **Phase 0 — Foundation**: done. Workspace scaffolding, `ff-core` domain
  types, `ff-storage` schema/migrations, `ff-cifp`/`ff-nasr` parsers with
  fixture tests (and validated against real cycle files — see TODO.md),
  and `ff-etl` producing a first cycle bundle end to end: fetches the
  current CIFP/NASR cycle live from FAA, builds an `ff-storage`-schema
  bundle, validates it against the previously published cycle, and
  publishes it for `ff-api` to serve (`/cycles/latest`,
  `/cycles/:id/bundle.sqlite`). Scoped to the same 5-airport Bay Area
  region as the web demo rather than a full ARTCC boundary, and doesn't
  fetch/tile chart imagery yet (that pipeline exists and is validated,
  see "Chart imagery" in TODO.md, just not wired into this automated
  loop) — real object storage for `publish_bundle` is also still a local
  directory, not a bucket. See TODO.md for the specifics.
- **Phase 1 — MVP (read-only)**: web + Android chart/procedure/airport
  viewer, weather/NOTAM briefing. Offline cycle sync is Android-only
  (§8) — the milestone this phase is really chasing is "can I look
  things up offline on the device that's actually in the cockpit"; web
  is a connected-only companion for the same data. No planning or track
  recording yet.
- **Phase 2 — Flight planning**: route builder, nav log, basic W&B,
  aircraft profiles.
- **Phase 3 — Post-flight analysis**: GPS track recording (Android),
  GPX import (web), phase-of-flight detection, flight log export.
- **Phase 4 — Accounts & sync** (optional): let a pilot's route plans,
  aircraft profiles, and flight logs follow them between web and Android
  — still no server-side flight-plan filing.
- **Phase 5 — Expand beyond Phase 1 scope**: broader leg-type/procedure
  coverage, non-US airspace (would require different, likely non-free,
  data sources per country), evaluate paid data partnerships if the
  product justifies it.
