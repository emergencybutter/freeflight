//! Client for the free aviationweather.gov Data API: METAR and TAF
//! (DESIGN.md §3, §9.2). AIRMET/SIGMET/G-AIRMET/winds-aloft follow the
//! same client pattern and are left for a follow-up once the polygon
//! GeoJSON response shape has been mapped out.

pub mod client;
pub mod records;

pub use client::{WeatherClient, WeatherError, DEFAULT_BASE_URL};
pub use records::{CloudLayer, Metar, Taf, TafForecastPeriod};
