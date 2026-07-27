# freeflight — Design Document

Status: Draft v0.3
Scope: Phase 1 (US-only, free data sources)

Revision history (newest first; details in `git log` for this file):

- **v0.3 (2026-07)**: `ff-etl` bundle and chart coverage both went
  nationwide — every airport/procedure in the CIFP file, and every
  current FAA sectional (§3, §7, §13), each tiled at its own full extent
  rather than cropped to one region. `ff-api` gained `/data/search`
  (§4.1). `CycleManifest`/`/cycles/latest` leave `pmtiles_*` `None`
  pending real multi-chart sync support (§4.1). Folded `TODO.md`'s
  handoff notes into this document and retired that file — implementation
  status now lives here (§3, §13), with rationale/history in `git log`.
- **v0.2 (2026-07)**: offline capability scoped to Android only — web
  becomes a thin, connectivity-assuming client of `ff-api` (§8, risk
  [web-offline]). NOTAM source updated to the FAA NMS API after the
  original API was retired (§3, risk [notam-api]). Phase 0 marked done
  (§13). Added `ff-api` HTTP surface (§4.1), data-source status column
  (§3), briefing-snapshot tables (§6), and security/availability notes
  (§11, §12).
- **v0.1**: initial design.

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
- These are explicitly deferred to later phases (§13) so Phase 1 stays
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

The Status column keeps this table honest about what's actually
consumed today vs. designed-for; update it as sources come online.

| Data | Source | Format | Update cycle | Status |
|---|---|---|---|---|
| Coded instrument flight procedures (SIDs, STARs, approaches, airways, navaids, waypoints) | FAA CIFP | ARINC 424 fixed-width records | 28-day AIRAC cycle | Implemented; validated against a real cycle file, airways included (~1.5k airways / ~19k legs nationwide) |
| Airport/facility directory (runways, frequencies, remarks, services) | FAA NASR subscription | Fixed-width / CSV | 28-day AIRAC cycle | Implemented (runways, surfaces, frequencies); validated against a real subscription |
| VFR charts (Sectional, TAC, Helicopter) | FAA digital raster charts | GeoTIFF | 56-day cycle | Sectionals implemented in the automated `ff-etl` loop, nationwide: discover the current chart cycle → download every FAA sectional (CONUS + Alaska + Hawaii + a few Canadian border charts) → expand palette to RGB → tile each to its own PMTiles archive, one `chart_catalog` row per sectional per cycle. TAC/Helicopter charts unstarted |
| IFR Enroute Low/High Altitude charts | FAA digital raster charts | GeoTIFF | 56-day cycle | Implemented in the same `ff-etl` loop: FAA publishes these as real georeferenced GeoTIFFs too, at an analogous URL shape (`aeronav.faa.gov/enroute/<date>/enr_l##.zip`/`enr_h##.zip`), discovered from the FAA IFR digital-products page (which embeds the panel links directly — no separate directory-listing fetch needed, unlike sectionals). Unlike sectionals, these charts' *body* is mostly white background (line symbology, not colored terrain fill) and the GeoTIFFs are already 3-band RGB, not palette-indexed — confirmed against two real downloaded panels before building this, which is also why the existing legend-crop heuristic isn't applied to this chart type (it assumes "mostly white = legend," which inverts for these charts and was confirmed live to crop away 95% of a real panel) — panels tile with their legend column left in place instead |
| IFR charts (Enroute Low/High, Area) | FAA digital raster charts | GeoTIFF | 56-day cycle | Unstarted (same pipeline as VFR should apply) |
| SID/STAR/Approach plate charts (visual reference) | FAA d-TPP | PDF (not geo-referenced — a scanned/typeset plate image, unlike the raster charts above) | 28-day cycle | Implemented for web: `ff-etl`'s `dtpp` module fetches the current cycle's metadata XML (~16MB, one per cycle) and matches its chart entries against this bundle's own procedure idents — exact match via the metafile's `faanfd18` field for SIDs/STARs (confirmed live: formatted `{ident}.{transition}` for departures, `{transition}.{ident}` for arrivals), a best-effort ARINC-424 type-code/runway/suffix heuristic for approaches (no equivalent field exists for those; ~95% match rate measured against real data, verified empirically rather than assumed). The PDF itself isn't re-hosted — only the constructed `https://aeronav.faa.gov/d-tpp/<cycle>/<pdf_name>` URL is stored, since the file is already public and stable enough per-cycle |
| Airspace boundaries (Class B/C/D, SUA, MOA) | FAA ArcGIS Hub feature services (`Class_Airspace`, `Special_Use_Airspace` — not the NASR CSV subscription, which only has per-airport Class B/C/D flags, no geometry) | GeoJSON via REST query | continuously current | Implemented in the automated `ff-etl` loop and validated against live data (~1286 Class B/C/D shelves + ~1533 Special Use Airspace areas) |
| Obstacles | FAA Digital Obstacle File (DOF) | Fixed-width | 56-day cycle | Unstarted |
| METAR / TAF / PIREP | aviationweather.gov Data API | JSON/XML | real-time | METAR/TAF implemented + validated live; PIREP unstarted |
| AIRMET / SIGMET / G-AIRMET, winds/temps aloft | aviationweather.gov Data API | JSON/XML/GeoJSON | real-time | Implemented + validated live (G-AIRMET, SIGMET, intl SIGMET, winds/temps aloft) |
| NOTAMs | FAA NOTAM Management Service (NMS) API (`api-nms.aim.faa.gov`) | GeoJSON/AIXM | real-time | Client implemented, endpoint confirmed live; record shape unvalidated pending credentials (risk [notam-api]) |
| Terrain elevation | USGS 3DEP / SRTM1 | GeoTIFF (public domain) | static | Unstarted |
| GPS track (flight recording) | On-device GPS (browser Geolocation / Android FusedLocationProvider) | — | live | Unstarted (Phase 3) |

Notes:
- The correct standard for CIFP is **ARINC 424** ("Navigation System
  Database"). The FAA's CIFP is the ARINC 424-formatted product this
  design targets.
- No scraping of paid providers (Jeppesen, Garmin Pilot, ForeFlight's own
  feeds). If a free source disappears or changes terms, that feature is
  disabled rather than replaced with a workaround that violates ToS.
- Own GPS track is the only "traffic"/track data in Phase 1 — no
  ADS-B/FlightAware/ADSBExchange dependency, which keeps post-flight
  analysis usable with zero external accounts.

### 3.1 Non-US data (Phase 2): National eAIP / AIXM, starting with France

Phase 1 is US-only because every source above is US-federal public
domain. For non-US airports, navaids, waypoints, airways, and airspace the
chosen source is each state's **official AIS publication** — its **AIXM**
dataset (the ICAO-standard XML aeronautical exchange format), the same
data its own charts are cut from. Brought online **a state at a time**;
the first is **France (SIA)**.

(An earlier draft adopted OpenAIP; reverted, because it is
community-maintained and, decisively, doesn't carry enroute RNAV
fixes/airways at official completeness. National AIXM does. That revert
still stands for the question it answered — OpenAIP is not a substitute
for official AIXM. It is revisited in **§3.1.2** for the narrower case it
does not cover: states whose official data may not be re-hosted at all,
where the real comparison is OpenAIP versus no coverage. The revert also
cited a CC BY-NC licence; that reading was **wrong** and is corrected in
§3.1.2 — commercial use is permitted.)

**AIXM version — 4.5, not 5.1, and why.** The obvious target was AIXM 5.1
(GML-based, current). But France's SIA publishes its **operational** export
in **AIXM 4.5**; its only 5.1 is a frozen 2017 EUROCONTROL *demo*
("not for operational use", unmaintained, no airways). Since France is the
first region, we build the 4.5 parser: it's the real, maintained, free
feed. 4.5 is also structurally simpler than 5.1 (flat feature XML — no GML
geometry, timeslices, or xlink), so it's the pragmatic start. A future
state that ships operational 5.1 will need a 5.1 reader alongside; the two
formats are different enough to be separate parsers, not a version flag.

| Data | Source | Format | Licence | Update cycle | Status |
|---|---|---|---|---|---|
| Non-US airports, navaids, waypoints (designated points), runways, airways — then airspace | National AIS AIXM; first target **France / SIA** (`AIXM4.5_all_FR_OM_*.xml`, in `export_xml_bd_SIA*.zip`) | AIXM 4.5 (flat feature XML, UTF-8) | France: **Licence Ouverte** (Etalab) — redistribution + commercial OK, attribution required | 28-day AIRAC | `ff-aixm` implemented + **validated against the real SIA export** (AIRAC 07/26, full 43 MB file: 878 airports / 394 navaids / 4285 waypoints / 779 runways / 425 airways·1895 legs, golden test in `tests/real_sia.rs`): `Ahp`→airport, `Vor/Ndb/Dme/Tcn`→navaid, `Dpn`→waypoint, `Rwy`+`Rdn`→runway, `Rte`+`Rsg`→airway (segments chained), `Ase`+`Abd`→airspace (arc/circle borders expanded; joined by feature `mid`; single-point non-area zones skipped). **`ff-etl` integration done**: `aixm` module loads a locally-provided export (SIA is cart-gated, not auto-fetchable) gated on `FF_AIXM_FR_PATH`, `bundle::add_aixm` + the shared `add_airspace` persist it into the same tables the FAA sources use — verified end-to-end (real zip → bundle: 878/779/394/4285/425/1895/**1541 airspaces**). Still to do: per-feature `region`, **client-side Licence Ouverte attribution surfacing** (About page done; needs the SIA cycle date) |

**Licence (France).** The SIA AIXM 4.5 export is under the **Licence
Ouverte** (French open licence): free, worldwide, **redistribution and
commercial use explicitly permitted**. So — unlike OpenAIP — it needs **no
§12 exception**; the only obligations are **attribution** ("Service de
l'Information Aéronautique (SIA)" + the data's update date) and not
distorting the data (already covered by the "not for navigation"
disclaimer). This licence is France's alone — see the per-country caveat.

**The two standing costs:**

- **Fragmentation.** No single global feed. Each state's AIS publishes on
  its own portal/schedule/access method (open download, registration-
  gated, or only eAIP HTML/PDF with no machine-readable AIXM).
  EUROCONTROL's EAD aggregates Europe but is access-controlled. So this is
  genuinely *per-country ETL*, widened region by region — not one parser
  that lights up the world.
- **Licensing is per-country.** National AIP terms range from open
  licences (France's Licence Ouverte) to Crown-copyright / explicit reuse
  restrictions. Each state's terms are checked before its data enters a
  published bundle (§12); a state that forbids redistribution is accessed
  but not re-hosted, or dropped.

#### 3.1.1 Per-state status

Two independent gates decide whether a state is cheap, expensive, or
impossible, and they are not correlated:

1. **Licence** — may we *re-host* it? §12 forbids bundling anything that
   isn't redistributable, so a "no" here ends it regardless of format.
2. **AIXM version** — `ff-aixm` parses **4.5** only (flat snapshot). A
   5.1/5.1.1 state needs GML geometry, TimeSlice temporality and xlink
   resolution: a second parser, not a flag. France is the fortunate case
   where both gates open at once.

> **Finding (checked 2026-07-26): freely redistributable national AIS data
> is rare.** Surveying EUROCONTROL's AIXM data-source inventory, France is
> the only state found so far publishing a full AIP dataset under a licence
> that permits redistribution. Most of ECAC reaches the public only through
> **EAD**, which is access-controlled; the states that do publish directly
> tend to assert copyright. The non-US roadmap should therefore be planned
> as "France plus a slow, per-state legal effort", not "France first, then
> Europe falls out cheaply".

| State / provider | AIXM | Access | Redistribution | Status |
|---|---|---|---|---|
| **US — FAA** (NASR/CIFP, not AIXM) | n/a | open download | **public domain** | ✅ in use |
| **France — SIA/DSNA** | **4.5** (also a 5.1 dataset) | direct, cart-gated | **Licence Ouverte** — redistribution + commercial OK, attribution required | ✅ implemented |
| **Germany — DFS** | 5.1.1 (SNAPSHOT/BASELINE TimeSlices) | public download, `aip.dfs.de/datasets` | **prohibited** without prior written permission (AIS portal terms); automated access *is* carved out for AIXM downloads | ❌ blocked — **ask DFS** (below) |
| **Norway — Avinor** | 5.1 | AIS portal | AIP Norway asserted under the Copyright Act | ❌ likely blocked ⚠️ |
| **Finland — AIS Finland** | 5.1 | AIP SUP attachments only | — | ⚠️ not a full AIP dataset; partial/temporary airspace only |
| **Brazil — DECEA** | 5.1 | published dataset | unverified | ⚠️ worth checking |
| **Latvia** | 5.1 | EAD, registered users | access-controlled | ❌ |
| **Most of ECAC** (Austria, Belgium, Croatia, Cyprus, …) | via EAD | **EAD**, access-controlled | not redistributable by default | ❌ |
| **UK — NATS** | — | AIP portal | Crown copyright | ✅ served via openAIP (§3.1.2) |
| **Canada — NAV CANADA** | — | commercial licensing | not redistributable | ✅ served via openAIP (§3.1.2) — note thin navaid coverage |
| **Greenland / Denmark / Faroes — Naviair** | **none published** | eAIP documents only | not publicly stated | ✅ served via openAIP (§3.1.2) — no official alternative exists |

⚠️ = lead, not a conclusion. Only the France, Germany and Denmark rows
have been read against the provider's own material; the rest come from
EUROCONTROL's inventory and secondary statements. **Nothing here is a
licence review** — §12 still requires reading a state's actual terms
before its data enters a bundle.

**Naviair (Denmark, Greenland, Faroe Islands) — checked 2026-07-26.**
There is nothing to license, because there is nothing published:
EUROCONTROL's AIXM inventory entry for Denmark reads *"No information
available yet"*, and Naviair's own AIM portal offers eAIP documents with
no machine-readable dataset and **no stated terms of use** at all. So for
Greenland the licence question is moot — the official-AIXM tier has no
input to consume, and openAIP is not a fallback but the only route. If
Naviair later publishes a dataset, the terms will need asking for
directly (`aim@naviair.dk`), since they are not on the site.

#### 3.1.2 OpenAIP as a fallback tier (`ff-openaip` implemented)

An earlier draft adopted **OpenAIP** as *the* non-US source and reverted
it (see above). That decision stands for what it decided — OpenAIP is not
a substitute for official national AIXM. It does **not** decide the
different question §3.1.1 raises: what to do for a state whose official
data we may not re-host at all. There the comparison is not "OpenAIP vs
France's Licence Ouverte" but **"OpenAIP vs no coverage"**.

**Why it clears the gate that blocks Germany.** The §12 rule is about
*redistribution rights*, and OpenAIP grants them: re-hosting is permitted
with attribution, where DFS's terms forbid it outright. On the one axis
that blocks Germany, OpenAIP is the *more* permissive source.

**Licence position — corrected 2026-07-26.** This document previously
recorded OpenAIP as **CC BY-NC**, and the original revert cited that as a
reason. That is **not** the operative restriction: OpenAIP data **may be
included in paid or commercial software**, provided the underlying data
stays free to use and the product is not simply reselling the data
itself. freeflight — a planning tool that happens to bundle the data, not
a data vendor — sits inside that condition, and Phase 5's "paid data
partnerships" ambition is **not** foreclosed. The earlier NC reading was
the weaker of the revert's two reasons; the completeness reason below is
the one that actually survives.

**The price:**

- **Completeness.** The revert's decisive finding was that OpenAIP
  "doesn't carry enroute RNAV fixes/airways at official completeness".
  Nothing here disputes that. So **scope it to what it does well —
  airports, airspace, navaids — and do not import airways or enroute
  fixes from it.** A German route with no airways is honest; one with
  half an airway network is worse than none, because the gaps are
  invisible until a leg silently fails to expand.
- **Mixed provenance is a presentation problem, not just a data one.**
  France would be official AIS; Germany community-maintained. Rendered
  identically, a pilot cannot tell which is which. freeflight already has
  the pattern for this: §9.5.3's `verified_at`, where unverified aircraft
  figures are flagged wherever they feed a plan. Apply the same shape —
  a **per-feature source** on the bundle rows, surfaced in the UI
  ("community data — not an official AIS source") anywhere OpenAIP-derived
  features appear, and in the About page's attribution list.

**Attribution and the standing obligation.** Permission to bundle is
conditional, so the conditions are load-bearing: attribute OpenAIP
wherever its data appears (About page alongside the SIA attribution,
§3.1), and never present the extract as a product in itself. Worth
holding the exact wording of the terms in the repo rather than in
memory — the licence has already been mis-recorded here once.

**Status: `ff-openaip` implemented and wired into `ff-etl`**, validated
against the live API for four states. Configuration:

- `FF_OPENAIP_API_KEY` — required; unset skips the step entirely, leaving
  a US-only (or US+France) cycle unaffected.
- `FF_OPENAIP_STATES` — `CC:REGION` pairs, default `DE:ED,GB:EG,CA:CY,
  GL:BG`. Note `CC` is openAIP's country code and `REGION` the ICAO
  region, which differ (Canada is `CA` but `CY`).

**The one-tier-per-state rule is enforced twice.** Configuring a state
that the official-AIXM tier already serves (France) fails the run rather
than silently producing two versions of every airport; and the openAIP
step runs *after* the AIXM step, so `INSERT OR IGNORE` gives official
data priority on any ICAO both could supply. Individual state fetch
failures are logged and skipped — one unreachable country must not sink a
cycle — but a tier conflict is a config error and propagates.

Still to do: per-feature source provenance in the bundle + UI, and the
reporting-points import.

| State | Region | Airports | Navaids | Airspace | of which controlled |
|---|---|---|---|---|---|
| Germany | `ED` | 1364 / 1364 | 79 | 509 / 745 | 342 |
| United Kingdom | `EG` | 469 / 469 | 136 | 794 / 1185 | 388 |
| Canada | `CY` | 1452 / 1452 | 22 | 2264 / 2264 | 2010 |
| Greenland | `BG` | 77 / 77 | 19 | 26 / 27 | 10 |

**`ff-core` gained Class A and Class F** to make this correct. Neither
occurs in US airspace the FAA sources describe, so neither was modelled —
but **107 UK volumes are Class A** (the CTAs, including London's), and
Class A is precisely the airspace VFR traffic may not enter. Dropping it
would have removed the most important warning in a VFR tool for an entire
country. Canada contributes 85 Class F advisory volumes.

**Canada's 22 navaids is a coverage warning, not a bug.** Germany has 79
and the UK 136 for far smaller areas; 22 for Canada means openAIP's
navaid coverage there is thin. Airports (1452) and airspace (2264, all
converting) are healthy. Worth surfacing to the user rather than
presenting sparse data as complete.

**Known gap — airspace types `ff-core` cannot describe** are skipped
rather than mislabelled: 236 in Germany (86 parachute areas, 90 glider
sectors, 31 RMZ, ~29 FIS/LFA/ATZ) and 391 in the UK. An RMZ is airspace
you may not enter without a radio, so these are not junk — but forcing
them into `Alert` would put a wrong label in front of a pilot. Closing it
means extending `ff-core` with the European kinds. **This must not be
quietly skipped when these states ship.**

**Shape.** A `ff-openaip` crate parallel to `ff-aixm` (OpenAIP publishes
its own JSON/GeoJSON, not AIXM, so it is a separate reader either way),
feeding the same `ff-core` types and the same bundle tables, gated
per-state: a state is served by official AIXM *or* by OpenAIP, never
both, so features never silently conflict. `ff-etl` picks the tier from a
per-state config rather than a `FF_AIXM_FR_PATH`-style single env var
(see §3.1.1's note that the current wiring is France-shaped).

Data is split per country and object type — `apt` airports, `asp`
airspace, `nav` navaids, plus runways — as GeoJSON `FeatureCollection`s
(points for everything except airspace polygons; `[lon, lat]` decimal
degrees, elevations in **metres**, frequencies in MHz). Note the unit
difference from every other source in this project, which is feet.

**Access is not anonymous — checked 2026-07-26.** Both documented routes
now require credentials:

- The daily-export bucket
  (`storage.googleapis.com/29f98e10-…/{cc}_{type}.{fmt}`, e.g.
  `de_apt.geojson`) answers **`UserProjectMissing — Bucket is a requester
  pays bucket`**. It is downloadable only with a Google Cloud project to
  bill egress to; the widely-cited "just fetch this URL" recipe no longer
  works, including the documented `at_apt.xml` example.
- The REST API (`api.core.openaip.net`) returns **403
  `auth/forbidden`** — "No authenticated user found" — so it needs an
  account and API key.

So `ff-etl` needs a credential either way. That fits the existing
locally-provided-export pattern (SIA is cart-gated and already manual),
but it means OpenAIP is **not** a zero-setup source.

**Chosen route: the REST API with a key**, read from `FF_OPENAIP_API_KEY`
(consumed by `ff-etl`, where cycles are built — *not* by `ff-api`, which
never talks to OpenAIP). Auth is the header `x-openaip-api-key`; a query
param also works but keeps the secret in URLs and logs, so the header is
used. The key is a secret: it belongs in the ETL environment, never in
the repo or a bundle.

#### 3.1.3 OpenAIP data shape (verified against live DE data, 2026-07-26)

Responses are paged (`{limit, totalCount, totalPages, nextPage, page,
items}`); geometry is GeoJSON (`[lon, lat]` decimal degrees). German
coverage: **1364 airports, 79 navaids, 745 airspaces, 336 reporting
points**.

Five things that shape the importer:

- **Units are metric, and encoded as enums.** `elevation: {value, unit,
  referenceDatum}` with `unit: 0` = metres — confirmed against EDDF
  runway 18, whose 3999 m matches its real 4000 m length. Every other
  source in this project is feet. Convert at the crate boundary, with a
  test, or the mistake is invisible.
- **Everything is a numeric enum** — `type`, `icaoClass`, `activity`,
  `surface.composition`, frequency `type`, `unit`. There is **no schema
  or enum endpoint** (`/api/{docs,schema,enums,openapi.json}` all 404).
- **Many airports have no ICAO code.** Of 1364 German entries, small
  fields carry `name` only (no `icaoCode`/`iataCode`) — "FRANKFURT BGU"
  is one. Anything keyed on ICAO identity must tolerate its absence, or
  silently drop most of the country.
- **Runways are per-direction**, like AIXM's `Rdn`: EDDF returns 7 runway
  entries for 4 physical runways, each with `designator`, `trueHeading`
  and metric `dimension`. They need the same pairing `ff-aixm` does.
- **No waypoints and no airways** — confirming the revert's decisive
  objection first-hand, and why §3.1.2 scopes this to airports/airspace/
  navaids. There *are* **reporting points** (336 for Germany), which are
  the VFR entry/exit points German VFR flying is actually organized
  around — a genuine bonus the FAA sources have no equivalent of.

**`icaoClass` is safety-relevant and undocumented — derived, not
guessed.** It drives §9.3's Class B/C/D VFR warnings, so a wrong mapping
mislabels airspace in a planning tool. Derived by correlating against
real-world German airspace whose class is independently known:

| Value | Reading | Evidence |
|---|---|---|
| 2 | C | 87 entries, all `type: 7` (TMA); German TMAs are Class C |
| 3 | D | `CTR ANSBACH`/`AUGSBURG`/`BERLIN` — German CTRs are Class D |
| 4 | E | 110 entries; Class E is Germany's bulk controlled airspace |
| 6 | G | 6 entries |
| 8 | *unclassified* | 397 entries — `ED-R` restricted, `ED-D` danger, `RMZ`, glider sectors: things with no ICAO class |

Consistent with the obvious `0=A … 6=G` ordering, but **inference from
data, not documentation**. The importer must therefore encode this
mapping with its evidence and assert it in tests against named real
airspace (`CTR BERLIN` ⇒ Class D, `ED-R…` ⇒ unclassified), so a wrong or
changed mapping fails loudly instead of quietly mislabelling a Class C
TMA. Unknown values map to "unclassified", never to a guessed class.

**Action — approach the German authorities.** DFS is the most valuable
non-French target (large, dense, adjacent airspace, and it already
publishes a clean 5.1.1 dataset with automated download explicitly
permitted). The only blocker is re-hosting rights, which is a
conversation, not an engineering problem: contact `data@dfs.de` and ask
whether a "not for navigation" VFR planning tool may bundle their AIP
dataset into a client-side cycle, with attribution. Worth doing *before*
any AIXM 5.1 parser work — there is no point building the parser for data
we cannot ship. If permission is refused, the §3.1 fallback applies:
accessed live but not re-hosted, which conflicts with Android's
offline-first design (§8) and would likely mean dropping Germany.

Pipeline fit: the `ff-aixm` crate (parallel to `ff-cifp`/`ff-nasr`) streams
the AIXM 4.5 `<AIXM-Snapshot>` → `ff-core` types; `ff-etl` will fetch the
SIA export per AIRAC cycle and write it into the same bundle tables the FAA
sources use. AIXM carries no FAA-style region code, so navaid/waypoint
`region` is stamped from the dataset's ICAO region (`"LF"` for France).
The AIXM 4.5→`ff-core` feature mapping: `AirportHeliport (Ahp)`→`Airport`,
`Runway (Rwy)`+`RunwayDirection (Rdn)`→`Runway`, `Vor/Ndb/Dme/Tcn`→
`Navaid`, `DesignatedPoint (Dpn)`→`Waypoint`, `Route (Rte)`+`RouteSegment
(Rsg)`→`Airway`, `Airspace (Ase)`+`AirspaceBorder (Abd)`→airspace volumes.

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

### 4.1 ff-api HTTP surface

The contract both clients depend on. Response shapes for `/cycles/*`
are defined as Rust types in `ff-sync` and constructed by `ff-api`
directly, so server and client deserializer cannot drift apart silently
(this bit us once: `CycleManifest` and `/cycles/latest` had never
actually been run against each other and disagreed on several fields
until that was checked). Weather routes pass through `ff-weather`'s
validated structs.

Implemented today:

| Route | Consumer | Serves |
|---|---|---|
| `GET /health` | ops | liveness |
| `GET /weather/metar?ids=A,B` | both clients | `Vec<Metar>` (JSON) |
| `GET /weather/taf?ids=A,B` | both clients | `Vec<Taf>` |
| `GET /weather/gairmet` | both clients | `Vec<GAirmet>` (all current CONUS) |
| `GET /weather/sigmet` | both clients | `Vec<Sigmet>` |
| `GET /weather/isigmet` | both clients | `Vec<IntlSigmet>` |
| `GET /weather/cwa` | both clients | `Vec<Cwa>` (Center Weather Advisories, all current) |
| `GET /weather/pirep?bbox=` | both clients | `Vec<Pirep>` — unlike the other weather routes this one requires a bbox (aviationweather.gov's own API does too); its lat/lon order differs from this app's other bbox routes, converted server-side rather than leaking that inconsistency to clients |
| `GET /weather/windtemp?level=&fcst=&region=` | both clients | parsed `WindsAloftBulletin`, each station's ident additionally resolved to a `lat`/`lon` against the current cycle bundle (best-effort — the raw NWS product only carries idents; used for the route builder's nearest-station wind lookup, §9.3) |
| `GET /notams?location=ICAO` | both clients | raw NOTAM JSON (501 until credentials exist — risk [notam-api]) |
| `GET /dtpp/plate?url=` | web (`PlateViewer`) | proxies a single FAA d-TPP plate PDF byte-for-byte (`Content-Type: application/pdf`) — `url` must start with `https://aeronav.faa.gov/d-tpp/` (checked server-side; otherwise 400) so this can't be turned into an open proxy. Exists because `pdf.js` needs to fetch the PDF's bytes itself for highlighter annotation (§9.1), and aeronav.faa.gov sends no CORS headers, so a direct browser fetch is blocked |
| `GET /cycles/latest` | Android sync | `CycleManifest` (cycle id, bundle URL, sha256). `pmtiles_url`/`sha256` are always `None`: a nationwide cycle publishes one PMTiles file per sectional, which this single-chart shape can't represent (see `/data/charts` for the real list) — revisit once Android needs multi-chart offline sync |
| `GET /bundles/:id/cycle.sqlite` | Android sync | raw SQLite bytes (static file service, Range-capable) |
| `GET /bundles/:id/chart-<sectional>.pmtiles` | both clients | one sectional's chart tiles, one file per `chart_catalog` row — PMTiles is fetched via HTTP Range requests (web reads it directly through MapLibre's pmtiles protocol) |
| `GET /data/airports?bbox=` | web | airports (optionally filtered to a bounding box) |
| `GET /data/airports/:icao` | web | one airport + runways + frequencies |
| `GET /data/airports/:icao/procedures` | web | procedure list |
| `GET /data/procedures/:id` | web | transitions + legs + server-resolved fix coordinates, plus a matched d-TPP plate chart name/URL if `ff-etl` found one (§9.1) |
| `GET /data/charts?bbox=` | web | chart_catalog entries (optionally bbox-filtered) |
| `GET /data/airspace?bbox=` | web | Class B/C/D + Special Use Airspace boundary polygons (optionally bbox-filtered) |
| `GET /data/nearest_fix?lat=&lon=` | web | closest waypoint or navaid to a point — full scan over both tables (~49k waypoints + ~900 navaids nationwide), filtered/compared in Rust rather than SQL, same approach as the bbox routes above; backs the map's Waypoint tap tab (§9.1) |
| `GET /data/search?q=` | web | airport ident/name search (prefix on ICAO/FAA/IATA, substring on name, capped at 20) — pulled forward from Phase 2 once bundles went nationwide and a fixed airport list stopped making sense |
| `GET /data/airways/:ident` | web | one airway's seq-ordered legs + server-resolved fix coordinates (mirrors `/data/procedures/:id`'s `fixes` shape; ident lookup is case-insensitive) |
| `GET /data/search_idents?q=` | web | unified ident search across airports/waypoints/navaids/airways for the §9.3 route builder's single search box — exact matches first, then airports before fixes, capped at 20 |

Conventions: JSON only; no authentication in Phase 1 (see §11's abuse
note); errors are plain-text bodies with appropriate status codes (502
for upstream weather failures, 404 for unknown cycles/airports, 501 for
unconfigured features). Versioning: none yet — the web client and
`ff-api` deploy together in Phase 1, so breaking changes are
coordinated, not negotiated; revisit (URL prefix `/v1/` or media-type
versioning) before Android ships, since app-store clients can't be
force-updated in lockstep.

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
    ff-storage/         SQLite schema + migrations (rusqlite), used by
                        the ETL job, ff-api (server-side queries for the
                        web client), and (via uniffi) the Android client
    ff-sync/            cycle bundle download/verify/apply, delta logic
                        — Android-only; the web client has no local
                        cycle copy to sync (§8)
    ff-wasm/            wasm-bindgen bindings over ff-core/ff-planning
                        for the web client (ff-postflight bindings come
                        with Phase 3; no ff-sync binding — nothing local
                        to sync, see §8)
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

The authoritative schema is
`crates/ff-storage/src/migrations/0001_init.sql`; this section is a
readable summary and must track it, not the other way around. Fixes are
referenced by **ident** (`fix_ident`, matching ARINC 424's own
referencing style), not by foreign-key id — idents aren't globally
unique across ICAO regions, and the CIFP data itself doesn't
disambiguate, so pretending FK integrity there would be false
precision.

Cycle-bundle tables (populated by `ff-etl`, read-only in clients):

```
airport(icao PK, faa_id, iata, name, lat, lon, elevation_ft,
        airport_type, fuel_types)
runway(id, airport_icao FK, ident, length_ft, width_ft, surface,
       le_ident, le_lat, le_lon, le_heading_deg,
       he_ident, he_lat, he_lon, he_heading_deg)
frequency(id, airport_icao FK, kind, freq_mhz, remarks)
navaid(id, ident, navaid_type, lat, lon, elevation_ft, freq_khz, region)
waypoint(id, ident, lat, lon, region)
procedure(id PK, airport_icao FK, kind, ident, runway_ident)
procedure_transition(id PK, procedure_id FK, ident, kind)
procedure_leg(id, transition_id FK, seq, path_and_term, fix_ident,
              course_deg, altitude_constraint, speed_constraint,
              turn_direction)
airway(id, ident, kind)                          -- V/J/T/Q + Alaska "other"
airway_leg(airway_id FK, seq, fix_ident, min_altitude_ft, max_altitude_ft)
airspace(id, name, class, floor, ceiling, boundary_geojson,
         min_lat, min_lon, max_lat, max_lon)
chart_catalog(id PK, name, kind, cycle_id, min_lat, min_lon, max_lat,
              max_lon, tile_url)
```

Schema exists but **nothing populates it yet** (unstarted sources, §3):

```
airac_cycle(id, effective_date, source_version)  -- cycle id currently
                                                 -- lives in ff-etl's
                                                 -- latest.json instead
```

Client-local tables (never part of a published bundle; Android-only in
practice, since web has no local DB — §8):

```
aircraft_profile(id, name, cruise_tas_kt, fuel_burn_gph,
                 max_gross_weight_lb, forward_cg_limit_in,
                 aft_cg_limit_in)
route_plan(id, name, created_at, aircraft_profile_id FK)
route_leg(route_plan_id FK, seq, waypoint_ref, altitude_ft, notes)
flight_track(id, started_at, ended_at, aircraft_profile_id FK)
track_point(flight_track_id FK, seq, ts, lat, lon, alt_ft, gs_kt,
            track_deg)
flight_log_entry(id, flight_track_id FK, route_plan_id FK, departure,
                 arrival, total_time_seconds, taxi_time_seconds,
                 landings)

-- planned, not yet in the migration (backs §8's briefing snapshot —
-- sketched here so the concept has a concrete shape before Phase 1's
-- briefing UI needs it):
briefing_snapshot(id, taken_at, route_plan_id FK NULL)
briefing_item(briefing_snapshot_id FK, kind,     -- METAR|TAF|NOTAM|GAIRMET|SIGMET|WINDS
              station_or_location, raw_json)     -- the exact upstream
                                                 -- response frozen at
                                                 -- briefing time, so a
                                                 -- stale briefing shows
                                                 -- what the pilot saw,
                                                 -- not a re-decode
```

`procedure_leg.path_and_term` follows ARINC 424's leg-type coding (IF, TF,
CF, DF, CA, VA, etc.) directly, so the CIFP parser is a near-literal
translation rather than a lossy simplification — this matters if we ever
need to render procedures with the same fidelity as certified tools.

## 7. Data Pipeline (`ff-etl`)

- Runs on a schedule aligned to FAA's 28-day AIRAC and 56-day chart
  cycles (cron via CI, e.g. GitHub Actions scheduled workflow, or a small
  worker on the same host as `ff-api`).
- Target steps: fetch raw CIFP/NASR/DOF/shapefiles/GeoTIFFs → parse with
  `ff-cifp`/`ff-nasr`/`ff-charts` → validate (row counts vs. previous
  cycle, geometry sanity checks) → write a single versioned
  `cycle-YYYY-MM-DD.sqlite` and a matching `charts-YYYY-MM-DD.pmtiles` →
  upload to object storage behind a CDN → flip a `latest` pointer only
  after both artifacts pass validation.
- **Implemented so far** (see §13 Phase 0): CIFP + NASR +
  sectional-chart fetch/parse/tile/validate/publish runs end to end
  against live FAA data, nationwide — every airport/procedure in the
  CIFP file, and every FAA sectional currently published — writing
  `cycle.sqlite` plus one `chart-<sectional>.pmtiles` per sectional to a
  local directory (`FF_ETL_DATA_DIR`) with a `latest.json` pointer
  rather than object storage/CDN. The chart step discovers the current
  56-day chart cycle from the FAA VFR page (independent of the 28-day
  CIFP cycle), tiles each sectional at its own full native extent via
  `ff-charts` (no per-region cropping now that there's no single
  "region" left), and needs GDAL's CLI tools on `PATH`. No cron trigger
  yet; runs are manual. Run for real end to end against live FAA data
  (all 53 current sectionals, ~16GB of PMTiles output, ~5 hours):
  confirmed via `ff-api` — `/cycles/latest`'s `sqlite_sha256` matched an
  independent hash of the published bundle, all 53 `/data/charts`
  entries had correct bboxes, and a PMTiles file served a real 206
  Partial Content range response with a valid magic header. Nationwide
  chart output is disk-hungry enough that both the GDAL workdir
  (`tempfile::tempdir()`, i.e. `TMPDIR`) and `FF_ETL_DATA_DIR` need to
  point at a filesystem with tens of GB free — a run against a small
  `/tmp` or a small root disk will fail partway through (once with "no
  space" mid-GDAL, once more with "no space" on the final `latest.json`
  write after all 53 charts had already copied successfully).
  `chart_prep::crop_legend_and_collar` also trims FAA's baked-in legend/
  border margin before tiling: each sectional's raster shares one
  geotransform across its whole canvas (confirmed via FAA's own per-
  chart metadata — "only the main body of the chart is accurately
  georeferenced"), so left uncropped the legend/collar warps and tiles
  as if it were real chart imagery. The old region-cropped pipeline hid
  this by accident; going nationwide/full-extent exposed it. Detected
  per chart (a small preview scanned edge-in for the legend's white
  background vs. the chart body's near-total terrain-color coverage),
  not a fixed position, since placement/size varies chart to chart —
  validated against a real Wichita sectional (left legend column +
  bottom margin) and a real Western Aleutian Islands sectional (near
  none). Also fixed: `fetch_sectional_chart` used to keep only the
  first `.tif` in a sectional's zip, silently dropping the rest — a real
  bug for the handful of sectionals FAA ships as multiple separately-
  georeferenced files (Western Aleutian Islands' East/West split,
  Hawaiian Islands' Honolulu/Mariana/Samoan insets).
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
flight, not something relied on airborne with no signal (risk [web-gps]
already flagged browser limits as making web Android-secondary for
in-flight use; this generalizes that call to the whole client, not just
track recording — see risk [web-offline] for the decision record).
Consequences:

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
  from `ff-api`'s `/data/*` query endpoints (§4.1) per request. If
  `ff-api` is unreachable, the web app shows that plainly rather than
  silently failing or serving stale data — there's no cached/bundled
  fallback to reach for, by design, not by omission.
- Weather/NOTAM are fetched opportunistically whenever online on both
  platforms, with a "briefing snapshot" concept: the pilot explicitly
  takes a briefing before flight, which freezes the weather/NOTAM data
  used for planning and time-stamps it clearly in the UI ("Briefing
  taken 45 min ago"). Snapshots store the raw upstream responses, not
  re-decodable references — a stale briefing must show exactly what the
  pilot saw when they took it (`briefing_snapshot`/`briefing_item`, §6).
  On Android this persists locally so it survives going offline after
  departure. On web it only needs to survive the current session/tab —
  web assumes connectivity anyway, so there's no separate
  durable-persistence story required there.

## 9. Feature Design

### 9.1 Charts & procedures viewer

- MapLibre GL map with layer toggles: VFR sectional/TAC, IFR low/high
  enroute, and a vector overlay of airspace, airports, navaids. On
  Android, drawn from the local cycle SQLite (converted to GeoJSON on
  load). On web, drawn from GeoJSON `ff-api` returns for the current
  view (§8) — same overlay shape either way, different source.
  Implemented on web: VFR sectional and IFR Low/High Altitude Enroute
  chart layers — see §3/§7 for the ingestion side. TAC unstarted. Web's
  base layer, under all of this, is an OpenFreeMap vector basemap (free,
  no API key/rate limits) rather than a blank background, so the map
  stays usable whenever chart imagery is toggled off or hasn't loaded
  yet — opaque chart raster tiles cover it naturally once visible. Opens
  centered on KLGA at zoom 6 rather than a full-CONUS default view —
  unless this browser has a persisted view (below), which wins over the
  default.
- Two independent groups of map toggle buttons. Chart kind (Sectional/
  IFR Low/IFR High) is single-select — they're the same charts at
  different altitude scopes meant to replace each other, not stack —
  and materializes a kind's PMTiles sources lazily, the first time it's
  actually selected: adding a PMTiles source fetches that file's header
  immediately regardless of layer visibility, so eagerly adding every
  chart in the catalog on load once fired ~108 requests (57 sectionals +
  IFR Low/High) even though only one kind is ever shown at a time.
  Deselecting a kind removes its sources outright (not just hides its
  layers): a hidden raster source keeps its decoded tile cache alive,
  and together with MapLibre's default *dynamically-sized* per-source
  tile cache (57 sources for the Sectional kind alone) that blew past
  iOS Safari's per-tab memory ceiling within a minute of panning on an
  iPad — Safari kills and force-reloads the tab when that happens. The
  map now also sets a small fixed `maxTileCacheSize`, trading tile
  re-fetches on pan-back (cheap: HTTP range requests, CDN-cached) for
  bounded memory, and caps `pixelRatio` at 1.5 on iOS (render-buffer/
  texture memory scales with pixelRatio², so an iPad's dpr-2 canvas
  costs ~1.8× more than needed for a mild text softening; capping to 1
  would make chart fine print noticeably fuzzy — the wrong trade for a
  chart app). Desktop/Android keep native sharpness.
  Airspace/AIRMET-SIGMET/Airports/PIREPs/CWA are independent on/off
  switches (any combination can be showing at once), all on by default.
  MapLibre's own zoom/compass/attribution controls are restyled to
  match the app's dark theme instead of their white default.
- The map view persists itself per-browser (`localStorage`, separate
  blob from the flight plan's — see `persistence.ts`): camera on every
  moveend, base-chart kind and overlay toggles on change. Any reload —
  a crash (the iOS memory-kill above force-reloads the page), a manual
  refresh, tab eviction — restores the last view instead of resetting
  to defaults; a shared link's `?p=` view still wins over it (§9.1's
  Share button), since a link's recipient should see the sender's view.
  The fit-map-to-route effect skips its first run when a restored
  camera exists, so reloading with both a persisted flight plan and a
  persisted camera lands on the camera, not the route bounds (found
  via the same iPad report: the reload "returned me to my flight plan,
  but not the region of the map I was viewing").
- Tapping the map selects whichever airport in the current view is
  closest to the tap point, unconditionally (not gated on hitting the
  airport's own marker), and populates a tab bar below the map: Airport/
  Waypoint/Airspace/PIREPs/AIRMET/SIGMET/CWA. This replaced an earlier
  per-layer MapLibre Popup on each overlay (airspace/G-AIRMET/SIGMET/
  CWA/PIREP each opening their own) — a colored shape or point alone
  doesn't say what it means, and stacking one Popup per layer clicked
  didn't extend to "show me everything at this point across every
  category." The Airspace/PIREPs/AIRMET/SIGMET/CWA tabs list whatever
  features were actually under the tapped point (via
  `queryRenderedFeatures`, padded a few pixels for small/thin targets
  like PIREP circles and G-AIRMET's freezing-level lines); the Waypoint
  tab shows the closest waypoint/navaid (`/data/nearest_fix`, §4.1,
  since there's no client-side navaid/waypoint layer to query locally
  the way the others are) with a button to drop it into the route
  builder's middle fixes list.
- The Airport tab holds the airport detail view: runways, frequencies,
  remarks, and a procedure list (SIDs/STARs/approaches) pulled from
  `procedure`/`procedure_leg`, plus an action bar above it showing the
  selected airport (tap the label to search and change it) and buttons
  to set it as the route builder's departure/arrival or add it as a
  middle fix (§9.3) — the Set Departure/Arrival buttons highlight when
  the selected airport already matches that slot.
- Selecting a procedure draws it on the map (leg-by-leg from
  `procedure_leg`) and, when `ff-etl`'s d-TPP matching found one, shows
  the real FAA plate chart inline for visual cross-check — rendered on
  web by `PlateViewer` (`apps/web/src/PlateViewer.tsx`) via `pdf.js` onto
  a `<canvas>`, not the plain `<iframe>` this used before: a pilot can
  mark a plate up with translucent highlighter strokes (four colors, plus
  Erase/Undo/Clear — see below), and that needs pixels this page's JS
  controls, which a cross-origin iframe's rendered PDF content never
  gives access to. `pdf.js` needs the PDF's own bytes to rasterize, and
  aeronav.faa.gov sends no CORS headers, so `ff-api`'s `/dtpp/plate?url=`
  (§4.1) fetches server-side and re-serves them from our own origin —
  the same reasoning as this server's weather/NOTAM proxies, applied to a
  static file instead of a live API. The same `PlateViewer` backs
  `ProcedurePanel`'s plate, `AirportDiagramPanel`'s diagram, and
  `QuickChartLinks`' full-page overlay — one component, three call
  sites, matching the pre-existing "Full Page"/"Reduce" toggle's
  fixed-height (normal) / fullscreen (expanded) footprint. The page is
  fit "contain" (whole plate visible) by default; in the full-page
  overlay it can be zoomed in (buttons, wheel, or two-finger pinch on
  touch, up to 5×), which re-rasterizes the PDF at the higher resolution
  — a real re-render, not a blurry CSS upscale — and panned by scrolling
  or one-finger drag (when no highlighter tool is active; a second finger
  always pinches, even mid-stroke). The whole gesture layer runs on
  pointer events so mouse and touch share one path; the draw canvas keeps
  `touch-action: none` so a stroke doesn't scroll the page, which is why
  pan/pinch are handled explicitly rather than left to native scrolling.
  Inline (non-full-page) plates stay fit-only. Not a full PDF viewer (no
  rotate, text selection, etc.).
  Android's equivalent still needs its own approach (a `WebView` can't
  run this page's `pdf.js`/canvas code either — see `[dtpp-render]`, §12).
  - **Highlighter annotations**: freehand strokes drawn by dragging over
    the plate, stored as fractional (page-relative) point lists in
    `localStorage` keyed by the plate's own PDF URL
    (`apps/web/src/annotations.ts`, `freeflight.plateAnnotations.v1`) —
    same best-effort persistence contract as `persistence.ts`'s flight
    plan, and (like that plan) session-local/device-local only: no
    server sync, no cross-device story (Phase 4 territory if ever). A
    plate's URL embeds its FAA d-TPP cycle, so a cycle rollover simply
    starts that plate's markup over rather than trying to carry it onto
    a plate that may have changed. Rendered translucent (0.4 opacity) on
    a second, transparent canvas stacked over the PDF canvas — a real
    highlighter's own look, legible over whatever's underneath rather
    than obscuring it. The eraser hit-tests a pointer position against
    each stroke's polyline (point-to-segment distance) rather than
    requiring a pixel-exact hit.
  - A "+" next to the procedure's heading adds it as the route builder's
    SID/STAR directly from this view, without needing to browse for it
    again via §9.3's own picker — resolves immediately if the procedure
    has a single enroute transition, else shows the transition list to
    choose from (same "skip the picker when there's only one real
    choice" behavior as the route builder's own SID/STAR picker).

### 9.2 Weather & NOTAMs

- Route/airport briefing screen: METAR (raw + decoded), TAF, winds/temps
  aloft along the route (with an altitude selector over whatever levels
  the current bulletin actually has data for), and NOTAMs (filterable by
  relevance: runway/taxiway closures first). AIRMET/SIGMET/CWA polygons
  and PIREP points draw on the map (toggleable independently, §9.1) with
  their detail available via the tap tab bar rather than a fixed
  briefing-screen list.
- Fully driven by `ff-weather`/`ff-notam` through the `ff-api` proxy;
  decoded METAR/TAF share the same Rust decoder on both clients (compiled
  into `ff-wasm`/`ff-uniffi`) so "what does this METAR mean" logic is
  written once.

### 9.3 Simple flight planning

- Route builder: pick departure/destination airports, optionally a
  SID/STAR for each, and fixes/navaids/airways in between (autocomplete
  against the local cycle DB on Android — works offline; against
  `ff-api`'s `/data/search_idents` endpoint on web — requires
  connectivity, §4.1/§8). Implemented on web: departure/arrival airports
  are chosen first (dedicated fields), then "Set SID"/"Set STAR" browse
  that airport's real procedures — pick one, then a transition if it has
  more than one (see `apps/web/src/planning/procedureLookup.ts`, which
  also handles real data shapes without a clean literal `COMMON`
  transition by chaining onto whichever other transition picks up at
  the chosen one's endpoint fix). Fixes/navaids/airways in between still
  go through one unified search box, flight-plan-string style
  (`FIX1 V123 FIX2` — an airway expands to the fixes strictly between
  its neighbors, in that direction; see `expandRoute.ts`). The full
  route (departure → SID → fixes/airways → STAR → arrival) draws on the
  map in cyan with per-fix markers. Departure/arrival/middle fixes can
  also be set from the map's Airport/Waypoint tap tabs (§9.1) instead of
  this screen's own search fields — same route state either way.
- VFR/IFR toggle (defaults VFR). Under VFR, the route is checked against
  real Class B/C/D + Special Use Airspace boundaries (client-side
  point-in-polygon/segment-intersection over whatever `/data/airspace`
  bbox around the route returns, §9.1's data) and flags every volume a
  leg actually enters, lateral crossing only — altitude isn't compared
  against the volume's floor/ceiling (those are pre-formatted display
  strings, not structured numbers, per `AirspaceVolume`'s shape); the
  warning shows the floor/ceiling text so the pilot can judge relevance
  themselves. Suppressed under IFR, since an IFR flight is already on a
  clearance through controlled airspace and the same heads-up isn't the
  same kind of actionable under IFR that it is under VFR.
- Per-leg: great-circle/rhumb distance & course from `ff-planning`, ETE
  and fuel burn from a user-defined aircraft profile (cruise TAS, fuel
  burn GPH, optional simple winds-aloft correction — implemented on web:
  set a cruise altitude and each leg picks the nearest real station/level
  from the fetched winds-aloft bulletin, see `windsAloft.ts`).
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
- **Performance**: full CONUS CIFP+NASR cycle bundle should stay in the
  hundreds-of-MB range, not gigabytes — favor compact binary encodings
  over verbose JSON/text for anything embedded in the SQLite bundle.
  Chart tiles are downloaded per-region-of-interest, not the whole
  country at once. Real current bundle is ~145MB as of the 2026-07-09
  cycle (§13, Phase 0) — comfortably within this. (Revised from an
  earlier tens-of-MB target, set back when the bundle only held
  airports/procedures; nationwide waypoints/navaids/airways and the
  d-TPP chart-link table pushed it past that, and hundreds of MB was
  judged an acceptable cost for what those add.) Doesn't affect web
  either way (queries `ff-api` server-side, never downloads the whole
  file, §8) — this bears on Android's on-device copy once it syncs.
- **Testability**: `ff-cifp`/`ff-nasr` parsers get golden-file tests
  against real (public) FAA cycle excerpts; `ff-planning`/`ff-postflight`
  get unit tests with synthetic tracks/routes; `ff-api` gets integration
  tests with mocked upstream weather responses.
- **Legal/compliance**: prominent "not for navigation, VFR/IFR
  supplemental use only" disclaimer; FAA/NOAA data attribution per each
  source's terms of use; no redistribution of any non-public-domain data.
  The Phase 2 non-US source (national eAIP/AIXM, §3.1) has **per-country**
  terms — each state's AIP licence is reviewed before its data is included
  in a published bundle; a state that forbids redistribution is not
  re-hosted.
- **Abuse resistance** (currently unmet): `ff-api` is an unauthenticated
  public proxy with permissive CORS — as-is, anyone can use it as a free
  METAR relay, and once NOTAM credentials are configured, anonymous
  traffic spends *our* NMS quota and could get those credentials
  rate-limited or revoked. Before any non-local deployment: per-IP rate
  limiting on the weather/NOTAM proxy routes at minimum; restrict CORS
  to the real web origin(s); consider a lightweight app token for the
  clients. Fine to skip while everything runs on localhost — not fine to
  forget (risk [api-availability]).
- **Availability**: `ff-api` is a single point of failure by design in
  Phase 1 — web is fully down without it, and Android can't fetch new
  cycles or fresh weather (existing synced data keeps working). One
  instance, no HA story, acceptable for a hobby deployment; revisit
  before anyone depends on it (risk [api-availability]). The mitigations
  are cheap and already architecturally supported: cycle bundles and
  PMTiles are static files servable straight from a CDN with `ff-api`
  only minting manifests, and the weather proxy is stateless and
  trivially replicable.

## 12. Open Questions / Risks

Risks are named, not numbered, so cross-references elsewhere in this
document survive insertions/removals.

- **[web-gps] Browser background GPS limits**: web is a poor fit for
  in-flight track recording (tab throttling, no reliable background
  execution). Phase 1 web client may need to treat "record a flight" as
  an Android-only feature and let web users *import* a track (GPX)
  instead.
- **[web-offline] Web offline scope — decided**: after building a first
  pass at web-side offline sync (checksum-verified IndexedDB cache of
  the cycle bundle), decided to scope offline capability
  to Android only rather than maintain two parallel sync/storage stacks.
  Web becomes a thin client that queries `ff-api` per view and assumes
  connectivity — the same reasoning as [web-gps], generalized from
  "recording a track" to the whole client. Trades a simpler web
  architecture for web being unusable with zero connectivity, and gives
  `ff-api` a new responsibility: the `/data/*` JSON query endpoints
  (§4.1). Implemented: the web-side sync code built under the old design
  (`sync.ts`, its IndexedDB cache, the sql.js loader — and sql.js
  itself) has been removed, and the web client now queries `/data/*`
  per view, failing visibly when `ff-api` is unreachable.
- **[api-availability] ff-api as single point of failure**: see §11's
  availability and abuse-resistance bullets — one unauthenticated
  instance currently carries the whole product. Open questions: where
  does it deploy, who notices when it's down, and what's the trigger for
  adding rate limiting/CORS restrictions (proposal: before any non-local
  deployment, not after the first incident).
- **[dtpp-render] d-TPP plate rendering — web resolved (twice over),
  Android open**: web originally displayed the real FAA plate inline via
  a plain `<iframe>` — simpler than the `pdf.js`/native-renderer approach
  first envisioned here, since browsers render PDFs natively. That
  changed once highlighter annotation (§9.1) needed pixels this page's
  JS controls, which a cross-origin iframe's rendered content can't
  give — web now renders plates itself via `pdf.js` onto a `<canvas>`
  (`PlateViewer`), fetching bytes through `ff-api`'s `/dtpp/plate` proxy
  (§4.1) since aeronav.faa.gov sends no CORS headers for a direct
  browser fetch. Net effect: the "no rendering library needed" win was
  temporary, but it bought time before the actual requirement (markup,
  not just viewing) showed up. Android still needs its own spike: no
  equivalent "just point an iframe at it" trick, so it's a choice
  between an in-app PDF rendering library (perf/quality on low-end
  devices unverified) and "open externally" (simpler, worse UX) — and
  if Android ever wants highlighter parity with web, that decision
  needs to support drawing on the rendered output too, not just
  displaying it.
- **[leg-types] ARINC 424 leg coding completeness**: implementing the
  full leg-type state machine (RF legs, vectors-to-final, holding
  patterns) is nontrivial; Phase 1 should scope down to the common leg
  types (IF, TF, CF, DF, CA/CD/VA/VI) and explicitly flag procedures
  using unsupported leg types rather than silently mis-rendering them.
- **[notam-api] NOTAM API instability**: the original FAA NOTAM Search
  API was retired outright in 2026 and its replacement (the NMS API,
  §3) issues credentials by email request only. Current state: `ff-notam`
  targets the new API and the endpoints are confirmed live, but the
  actual NOTAM record shape is unvalidated pending credentials. Standing
  assumption: this
  remains the flakiest upstream dependency; `ff-api`'s proxy/cache layer
  should be built to degrade gracefully when it changes again.
- **[chart-hosting] Chart hosting cost/rights**: re-hosting converted
  FAA raster charts as PMTiles is public-domain data, but bandwidth cost
  for chart tiles at scale should be estimated before wide release (this
  is why the ETL step produces a single static-hostable file format
  rather than standing up a tile server).

## 13. Roadmap

- **Phase 0 — Foundation**: done. Workspace scaffolding, `ff-core` domain
  types, `ff-storage` schema/migrations, `ff-cifp`/`ff-nasr` parsers with
  fixture tests (and validated against real cycle files), and `ff-etl`
  producing a first cycle bundle end to end: fetches the current
  CIFP/NASR cycle live from FAA, builds an `ff-storage`-schema bundle,
  fetches/tiles every current FAA sectional and IFR Enroute Low/High
  chart, fetches Class B/C/D + Special Use Airspace boundaries and
  matches FAA d-TPP plate charts against procedures (§9.1), validates
  against the previously published cycle, and publishes all artifacts
  for `ff-api` to serve (§4.1). Real current bundle (2026-07-09 cycle):
  13,321 airports / 14,316 procedures / 48,731 waypoints / 890 navaids /
  1,466 airways / 2,819 airspace volumes / 108 chart_catalog rows /
  12,388 matched d-TPP chart links, ~145MB — within §11's
  hundreds-of-MB target. `publish_bundle`'s object storage is still a
  local directory, not a bucket — the remaining Phase 0 follow-up.
- **Phase 1 — MVP (read-only)**: web + Android chart/procedure/airport
  viewer, weather/NOTAM briefing. Offline cycle sync is Android-only
  (§8) — the milestone this phase is really chasing is "can I look
  things up offline on the device that's actually in the cockpit"; web
  is a connected-only companion for the same data. **Web is essentially
  done**: sectional + IFR Low/High chart layers (mutually-exclusive
  toggle, lazily loaded per kind), independent airspace/AIRMET-SIGMET/
  airports/PIREPs/CWA overlay toggles, a tap-driven Airport/Waypoint/
  Airspace/PIREPs/AIRMET/SIGMET/CWA tab bar replacing per-layer popups
  (§9.1), full METAR/TAF/AIRMET/SIGMET/CWA/PIREP/winds-aloft weather
  (§9.2), and inline FAA plate charts (SID/STAR/Approach/Airport Diagram)
  rendered via `pdf.js` with translucent highlighter annotation (§9.1).
  NOTAM proxy exists but its record shape is still unvalidated pending
  credentials (`[notam-api]`, §12). **Android hasn't been started at
  all** — every item above is web-only; Android is the actual gap this
  phase is named for (offline-in-the-cockpit), not a parallel-track
  detail.
- **Phase 2 — Flight planning**: route builder, nav log, basic W&B,
  aircraft profiles. Web slice implemented: `ff-planning`'s math now
  runs client-side via `ff-wasm` (previously built but not wired into
  the web app — see `apps/web/README.md`), behind a route
  builder/nav-log/W&B UI. Session-only (no persistence — the web client
  still has no local database, §8). Winds-aloft correction is wired in:
  setting a cruise altitude picks the nearest reporting station/level to
  each leg from the same NOAA bulletin the map already fetches (`ff-api`
  now resolves each station's ident to a coordinate against the current
  cycle bundle — the raw NWS product only carries idents — so the web
  client can do a nearest-station lookup at all; see
  `apps/web/src/planning/windsAloft.ts`). Departure/arrival/middle fixes
  can also be set straight from the map's Airport/Waypoint tap tabs
  (§9.1/§9.3), and a VFR-only warning flags real Class B/C/D/Special Use
  Airspace the route actually crosses (§9.3). Android's equivalent
  (`ff-uniffi` bindings, persisted via the
  `aircraft_profile`/`route_plan`/`route_leg` tables) is still unstarted.
- **Phase 3 — Post-flight analysis**: GPS track recording (Android),
  GPX import (web), phase-of-flight detection, flight log export.
- **Phase 4 — Accounts & sync** (optional): let a pilot's route plans,
  aircraft profiles, and flight logs follow them between web and Android
  — still no server-side flight-plan filing.
- **Phase 5 — Expand beyond Phase 1 scope**: broader leg-type/procedure
  coverage, non-US airports/navaids/waypoints/airways/airspace via national
  AIS AIXM (France/SIA 4.5 first; see §3.1 for the source, version, and
  per-country licensing decision, and the `ff-aixm` crate — brought online
  a region at a time), evaluate paid data partnerships if the product
  justifies it.
