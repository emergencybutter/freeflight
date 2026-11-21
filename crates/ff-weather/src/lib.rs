//! Client for the free aviationweather.gov Data API: METAR, TAF,
//! Graphical AIRMET, (domestic + international) SIGMET, Center Weather
//! Advisories, PIREPs — plus winds/temps aloft, which since the NWS FD
//! product's retirement come from the GFS model via Open-Meteo
//! (DESIGN.md §3, §9.2).

pub mod cache;
pub mod client;
pub mod fd_stations;
pub mod hazards;
pub mod records;
pub mod winds_aloft;

pub use cache::{parse_metar_flight_categories, CACHE_BASE_URL, METAR_CACHE_FILE};
pub use client::{WeatherClient, WeatherError, DATIS_BASE_URL, DEFAULT_BASE_URL, OPEN_METEO_GFS_URL};
pub use fd_stations::FD_STATIONS;
pub use hazards::{Cwa, GAirmet, GAirmetCoord, IntlSigmet, Pirep, PirepCloud, Sigmet, SigmetCoord};
pub use records::{CloudLayer, Datis, Metar, Taf, TafForecastPeriod};
pub use winds_aloft::{
    bulletin_at, GfsWindsForecast, StationProfile, StationWindsAloft, Wind, WindsAloftBulletin,
    WindsAloftLevel, FD_LOW_LEVELS_FT,
};
