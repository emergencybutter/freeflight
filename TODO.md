# TODO

## Get real chart imagery flowing

`ff-charts`' `geotiff_to_pmtiles` pipeline (gdalwarp → gdal_translate →
gdaladdo → pure-Rust MBTiles→PMTiles repack) is implemented and tested,
but only against a synthetic GeoTIFF — FAA's chart-imagery domains
(aeronav.faa.gov) are blocked from this environment's egress, the same
way FAA's CIFP/NASR domains were before those got fixed with
user-provided files.

To close this gap:

- Fetch or have the user upload a real FAA VFR raster chart GeoTIFF
  (sectional or TAC) from https://aeronav.faa.gov's raster chart
  products, or a mirror.
- Run it through `geotiff_to_pmtiles` for real, confirm the output
  PMTiles renders correctly (visually, in a MapLibre viewer once #2
  below lands).
- Wire a real chart into the demo bundle / catalog (`ChartCatalogEntry`)
  and into `apps/web`, following the same pattern used for CIFP/NASR
  data in `build_demo_bundle.rs`.
- Check chart licensing/attribution requirements for redistribution
  (FAA charts are public domain, but confirm before bundling one into
  the repo — likely want a small cropped extract rather than a full
  chart to keep repo size down).

## Validate ff-cifp's VHF/NDB navaid and waypoint extraction against real data

`extract_vhf_navaid`/`extract_ndb_navaid`/`extract_waypoint` (added to
close the "no procedure legs on the map" gap) are covered by unit tests
using hand-built ARINC 424 lines and a synthetic end-to-end CIFP file
run through `build_demo_bundle`, but — unlike the rest of `ff-cifp` —
haven't been run against a real FAA CIFP cycle file the way
`extract_airport`/`extract_runway_end`/`extract_procedure_leg_row` were
(see the "Validate ff-cifp against a real, current FAA CIFP cycle file"
commit). This environment's egress blocks FAA/aeronav domains, and no
mirror with the full `FAACIFP18` file turned up in a search this time;
ask the user to upload one (as happened for the NASR CSV subscription)
to run `crates/ff-cifp/tests/real_cifp.rs`-style validation.

One specific known gap worth checking against real data: CIFP's NAVAID
Class field (spec §5.35), which would distinguish VOR-only / VOR-DME /
VORTAC, is left unparsed (its exact column layout isn't reliably
documented in the open-source parser used to cross-check the other
field offsets — see the comment on `extract_vhf_navaid` in
`crates/ff-cifp/src/extract.rs`). `NavaidType` is currently inferred
from whether a DME sub-field is populated instead, collapsing VOR/DME
and VORTAC together under `NavaidType::VorDme`.

Once validated, regenerate `apps/web/public/demo-cycle.sqlite` — the
navaid/waypoint tables are still empty in the currently-committed
bundle, so the web client's procedure-leg map overlay (`MapView.tsx`)
has nothing to draw yet, even though the code path is wired up and
tested.
