# freeflight Android client

Kotlin + Jetpack Compose + MapLibre Android SDK over the shared Rust core
(`crates/ff-uniffi`), per `DESIGN.md` §5/§10.

This is the **offline-capable client** (§8). It owns a local copy of the
cycle bundle and the chart archives, and every screen except live weather
works with the radio off: charts, airport data, runways, frequencies,
procedures and airspace are all read from the device.

## What it does

Three tabs.

- **Map** — an FAA sectional (or IFR enroute chart) drawn from a PMTiles
  archive on the device, with Class B/C/D and Special Use airspace and
  airports overlaid from the local bundle, a search box over the whole
  nationwide cycle (airports, navaids, waypoints), and a status line that
  always shows the AIRAC cycle in use and the age of the last weather
  fetch (§11). Tapping an airport opens its sheet; tapping a procedure
  draws it and frames it on the map.
- **Data** — the installed cycle (effective date, counts, size), checking
  for and downloading a newer one, and per-chart download/removal. Nothing
  here transfers anything without being asked: a cycle is ~145MB and a
  sectional is a couple hundred more.
- **Settings** — the `ff-api` base URL, the not-for-navigation
  disclaimer, and the data-source attributions shipped inside the bundle.

The airport sheet separates what came from the installed cycle from what
came off the network a moment ago, and says so when the latter fails —
"no weather shown" and "weather couldn't be fetched" mean different
things to a pilot.

## How the pieces fit

The split with the Rust core is the one `ff-sync::apply` describes: **Kotlin
moves bytes, Rust verifies and installs them, and all reads go through
Rust.**

- `ff-uniffi` opens the local bundle and answers the same questions
  `ff-api`'s `/data/*` routes answer for web — from a file on the device
  instead of a file on a server. It also verifies and atomically swaps in
  a downloaded cycle, and reads raster tiles out of PMTiles archives.
- `data/ApiClient.kt` is the only thing here that speaks HTTP. Downloads
  stream to disk and resume with a `Range` request, so a 145MB bundle
  survives being interrupted.
- `map/TileServer.kt` is a loopback HTTP server that hands MapLibre tiles
  out of the core. The Android SDK has no `pmtiles://` protocol hook the
  way MapLibre GL JS does, so the app puts a server where the SDK expects
  one. It serves one URL shape and 404s everything else.
- The map style carries no `glyphs` or `sprite` URL and the overlays are
  deliberately label-free: the sectional underneath already has the
  labels, drawn by the FAA, and a text layer would make every label a
  network dependency.

## Building

Needs, beyond the Android SDK:

```sh
rustup target add aarch64-linux-android x86_64-linux-android
cargo install cargo-ndk
```

plus an NDK (`sdkmanager "ndk;28.2.13676358"`, matching `ndkVersion` in
`app/build.gradle.kts`). Gradle drives the rest: `:app:cargoBuild`
cross-compiles `ff-uniffi` and `:app:generateUniffiBindings` generates the
Kotlin bindings from the library it just built, so the bindings can never
describe a different core than the `.so` shipped beside them.

```sh
./gradlew assembleDebug
```

Both ABIs together take minutes, mostly `rusqlite` compiling SQLite per
target. The default (`ff.abis` in `gradle.properties`) covers real devices
and the emulator; add `armeabi-v7a` for a release that needs 32-bit
hardware.

### 16 KB page sizes

Android devices are moving to 16 KB memory pages, and a native library laid
out for 4 KB pages will not load on one. Nothing about a normal build tells
you — it installs and runs everywhere else — so the build checks it, and
fails on a library whose LOAD segments are aligned below 16 KB.

This bit us once already: JNA 5.14 shipped a `libjnidispatch.so` that a
Pixel reported as "RELRO segment not aligned", which is why the version
catalog pins JNA far above the version uniffi actually requires. The Rust
core needs two linker flags, not one — `max-page-size` aligns the segments
and `common-page-size` pads the RELRO region — both set in `cargoBuild`.

To inspect a library by hand:

```sh
$ANDROID_HOME/ndk/<ver>/toolchains/llvm/prebuilt/*/bin/llvm-readelf -l <lib>.so
```

Every `LOAD` line should show an alignment of `0x4000` or more.

## Running

The app is useless until it has a cycle, and it gets one from `ff-api`:

```sh
cargo run -p ff-etl    # build and publish a cycle (see the web README)
cargo run -p ff-api    # serve it (default :8080)
```

Then **Data → Download cycle**, and a chart covering where you are flying.
The default server address is `http://10.0.2.2:8080` — the host machine as
seen from the emulator — and is changeable in Settings. Release builds
allow plain HTTP only to that address; anywhere else must be HTTPS.

Everything works in airplane mode once a cycle and a chart are installed.

## Not here yet

Own-ship position (the location permission is declared but no GPS is read
yet), flight planning against `ff-planning` (its bindings are exposed and
tested, but no UI consumes them — `DESIGN.md` §13 Phase 2), track
recording (Phase 3), and NOTAMs (blocked on credentials, risk
`[notam-api]`).
