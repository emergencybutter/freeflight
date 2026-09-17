// GENERATED — do not edit.
//
// Source of truth: crates/ff-charts/src/vocabulary.rs
// Regenerate:     cargo run -p ff-core --bin gen-web-vocabulary
//
// Chart naming and airport display thresholds live in Rust so the web
// and Android clients cannot drift apart on them. Generated rather than
// called through wasm because the map path loads no wasm today, and a
// label is not worth an async init on that screen.

export const CHART_KIND_LABELS: Record<string, string> = {
  Sectional: "Sectional",
  TerminalAreaChart: "TAC",
  VfrFlyway: "Flyway",
  IfrEnrouteLow: "IFR Low",
  IfrEnrouteHigh: "IFR High",
  HelicopterRoute: "Heli",
  WorldAeronauticalChart: "WAC",
};

export const CHART_KIND_ORDER: Record<string, number> = {
  Sectional: 0,
  TerminalAreaChart: 1,
  VfrFlyway: 2,
  IfrEnrouteLow: 3,
  IfrEnrouteHigh: 4,
  HelicopterRoute: 5,
  WorldAeronauticalChart: 6,
};

export const DEFAULT_CHART_KIND = "Sectional";
