//! Client for the free aviationweather.gov Data API: METAR, TAF,
//! Graphical AIRMET, (domestic + international) SIGMET, and winds/temps
//! aloft (DESIGN.md §3, §9.2).

pub mod client;
pub mod hazards;
pub mod records;
pub mod winds_aloft;

pub use client::{WeatherClient, WeatherError, DEFAULT_BASE_URL};
pub use hazards::{GAirmet, GAirmetCoord, IntlSigmet, Sigmet, SigmetCoord};
pub use records::{CloudLayer, Metar, Taf, TafForecastPeriod};
pub use winds_aloft::{
    parse_windtemp_bulletin, StationWindsAloft, Wind, WindsAloftBulletin, WindsAloftError,
    WindsAloftLevel,
};
