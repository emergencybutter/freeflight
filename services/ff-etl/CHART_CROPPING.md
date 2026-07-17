# Chart cropping status

FAA raster charts ship as a full printed **sheet**: the georeferenced map
body plus a **collar** (legend columns, title block, CONUS index map,
barcode, Class B/airspace text tables, marginalia). For the map we want only
the body — the collar is georeferenced by the sheet's single geotransform,
so if left in it warps onto the map plane and covers adjacent geography.

Cropping happens in `ff-etl` before tiling to PMTiles (`src/chart_prep.rs`).
There are two mechanisms and one manual override.

## Per-family status

| Chart family | Cropper | Status |
|---|---|---|
| **Sectionals** | `crop_legend_and_collar` (white-band scan) | ✅ cropped |
| **IFR Enroute Low/High** | `crop_to_neatline` (black-frame detect) | ✅ cropped |
| **TAC** (Terminal Area) | `crop_to_neatline` → hardcoded fractions | ⚠️ **12 of ~34 cropped**; rest uncropped |
| **VFR Flyway** | `crop_to_neatline` | ❌ uncropped (detect fails; no hardcoded entries) |
| **Helicopter Route** | `crop_to_neatline` → hardcoded fractions | ⚠️ only `New_York` + `Downtown_Manhattan` cropped |

`crop_to_neatline` tries, in order: a **hardcoded fraction** for
`(kind, label)`; then **`detect_neatline`** (finds the black map frame in a
4000px `-r average` preview); else returns `None` and the chart tiles
**uncropped**.

## Why TACs need hardcoded crops

`detect_neatline` cannot read a TAC's frame:
- the neatline is a **thin line broken by lat/lon graticule tick labels**, so
  it never forms a continuous ≥40%-width dark run (higher preview resolution
  doesn't help — the line is genuinely thin + interrupted);
- content-density heuristics fail too — the body is **heterogeneous**
  (low-saturation rural terrain *and* near-white ocean), so no single
  saturation/whiteness threshold bounds it across all charts.

So each TAC's map-body box is **hand-measured** from the preview and added to
`HARDCODED_NEATLINE_FRACTIONS` in `src/chart_prep.rs`, keyed by
`(ChartKind, label)`. (Keyed by kind because a city's TAC and Helicopter
chart share a label — both `New York TAC.tif` and `New York HEL.tif` reduce
to `New_York` — and need different boxes.)

## TACs currently cropped (12, major Class B metros)

Atlanta, Boston, Chicago, Dallas-Ft Worth, Denver, Los Angeles, Miami,
New York, Philadelphia, Phoenix, San Francisco, Seattle.

**Still uncropped** (tile with the full collar): Anchorage, Fairbanks,
Baltimore-Washington, Charlotte, Cincinnati, Cleveland, Colorado Springs,
Detroit, Houston, Kansas City, Las Vegas, Memphis, Minneapolis-St Paul,
New Orleans, Orlando, Pittsburgh, Puerto Rico-VI, Salt Lake City, San Diego,
St Louis, Tampa, Portland. Each needs the same hand-measurement to be cropped.

## Production state

Shipped **2026-07-17** to the live cycle `2026-07-09`: the 12 cropped
`chart-*-tac.pmtiles` replaced the uncropped ones and their `chart_catalog`
bboxes were updated in place; `ff-api` restarted.
- **Rollback backup**: `/containers/freeflight/tac-backup-20260717/` on vya2
  (the 12 original uncropped pmtiles + the pre-edit `cycle.sqlite`).
- Cloudflare serves the bundles as `DYNAMIC` (uncached) — no purge needed.
- A future cycle regeneration applies the 12 crops automatically (they're in
  the committed hardcoded table); the uncropped TACs keep falling back until
  measured.

## How to crop another TAC

1. Fetch + preview the source (matches what `crop_to_neatline` sees):
   ```sh
   curl -sLO "https://aeronav.faa.gov/visual/<MM-DD-YYYY>/tac-files/<Name>_TAC.zip"
   unzip -o <Name>_TAC.zip
   gdal_translate -expand rgb "<City> TAC.tif" rgb.tif      # if palette-indexed
   gdal_translate -outsize 4000 0 -r average -of PNG rgb.tif preview.png
   ```
   Note: some zips hold two maps (e.g. Denver→Denver+Colorado Springs,
   Seattle→Seattle+Portland) — pick the `<City> TAC.tif` you want; the label
   is that basename minus `" TAC"`, spaces → underscores.
2. Read the map-body box off the preview as fractions
   `(left, top, right, bottom)` of the full image (the 4000px preview and the
   full-res source share fractions). Keep in-map text boxes (Class B/C
   altitude examples drawn over water) — they're chart content, not collar.
3. Add `(ChartKind::TerminalAreaChart, "<Label>", (l, t, r, b))` to
   `HARDCODED_NEATLINE_FRACTIONS`.
4. Re-tile just that chart and **render-verify** the crop before shipping
   (verification catches a box that clips the map or leaves collar):
   ```sh
   FF_CYCLE_SQLITE=... FF_CYCLE_DIR=... FF_CYCLE_DATE=2026-07-09 \
   FF_CHART_FILTER=<Name> \
   cargo run -p ff-etl --bin tile_terminal_charts   # needs GDAL on PATH
   ```
   `FF_CHART_FILTER` is comma-separated (e.g. `Atlanta,Boston`); the tiler
   clears catalog rows per-id, so a filtered run never drops others.

Deploying re-tiled charts to prod: ship the new `chart-*-tac.pmtiles` to
`/containers/freeflight/data/cycles/<id>/`, update the matching
`chart_catalog` bboxes in that cycle's `cycle.sqlite`, and restart `ff-api`.
