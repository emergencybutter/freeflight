# freeflight

A ForeFlight-inspired EFB app (charts, procedures, weather, simple flight
planning, post-flight analysis) built on a shared Rust core, with clients
for web and Android. US-only, free data sources, Phase 1. See
[`DESIGN.md`](./DESIGN.md) for the full design.

## Layout

- `crates/` — shared Rust core: domain types, ARINC 424/NASR parsers,
  weather/NOTAM clients, planning/post-flight analysis, storage, sync,
  and the `wasm`/`uniffi` bindings consumed by the clients.
- `services/ff-api` — axum server: weather/NOTAM proxy, cycle bundle
  hosting.
- `services/ff-etl` — batch job that builds versioned data-cycle bundles
  from FAA/NOAA sources.
- `apps/web`, `apps/android` — client applications (not part of the Rust
  workspace; scaffolded separately).

## Building

```sh
cargo check --workspace
cargo test --workspace
```

The `ff-wasm` crate additionally builds with `wasm-pack build
crates/ff-wasm --target web`. The `ff-uniffi` crate is consumed from
Android via its generated Kotlin bindings (`uniffi-bindgen`).
