//! Client for the free aviationweather.gov Data API: METAR, TAF,
//! Graphical AIRMET, and (domestic + international) SIGMET (DESIGN.md
//! §3, §9.2). Winds/temps aloft has no JSON API — aviationweather.gov
//! only serves it as a fixed-width text bulletin — and is left for a
//! follow-up that writes a real parser for that format, the same way
//! `ff-cifp`/`ff-nasr` do for their fixed-width sources.

pub mod client;
pub mod hazards;
pub mod records;

pub use client::{WeatherClient, WeatherError, DEFAULT_BASE_URL};
pub use hazards::{GAirmet, GAirmetCoord, IntlSigmet, Sigmet, SigmetCoord};
pub use records::{CloudLayer, Metar, Taf, TafForecastPeriod};
