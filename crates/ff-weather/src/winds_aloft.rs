//! Winds and temperatures aloft, derived from the GFS model.
//!
//! This used to parse the NWS/FAA "FD" bulletin from aviationweather.gov's
//! `/api/data/windtemp`, but that product was discontinued in Nov 2026
//! with the retirement of the NAM model (NWS SCN26-47; the endpoint now
//! answers 400). Its replacement here is GFS pressure-level data from
//! Open-Meteo (<https://open-meteo.com/en/docs/gfs-api>), interpolated
//! to the same altitudes at the same station idents and reshaped into the
//! same [`WindsAloftBulletin`], so API consumers didn't have to change.
//!
//! FD conventions reproduced on top of the raw model output:
//! - Altitudes are the "low" product's nine MSL levels, 3,000–39,000 ft.
//! - No forecast at a level within 1,500 ft of the station's elevation
//!   (no [`WindsAloftLevel`] at all for it).
//! - No temperature at 3,000 ft, nor at a level within 2,500 ft of the
//!   station's elevation.
//! - Winds under 5 kt are reported as [`Wind::LightAndVariable`];
//!   directions are rounded to the nearest 10° (true), speeds to whole kt.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Wind {
    LightAndVariable,
    Directional { direction_deg: u32, speed_kt: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindsAloftLevel {
    pub altitude_ft: u32,
    pub wind: Wind,
    /// `None` at 3,000 ft and close to the station's own elevation
    /// (see module docs).
    pub temp_c: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StationWindsAloft {
    pub station_id: String,
    /// Only the altitudes this station actually has a forecast for.
    pub levels: Vec<WindsAloftLevel>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindsAloftBulletin {
    /// When the model data was fetched, FD-style `DDHHMMZ`.
    pub data_based_on: String,
    /// The forecast hour these values are for, FD-style `DDHHMMZ`.
    pub valid_time: String,
    /// Free-text provenance note (the FD field of the same name held the
    /// usage window; nothing parses it).
    pub for_use: String,
    pub stations: Vec<StationWindsAloft>,
}

/// The FD "low" product's altitude columns (ft MSL).
pub const FD_LOW_LEVELS_FT: [u32; 9] = [3000, 6000, 9000, 12000, 18000, 24000, 30000, 34000, 39000];

/// GFS isobaric levels requested from Open-Meteo. 950 hPa keeps 3,000 ft
/// bracketed under strong surface highs; 150 hPa keeps 39,000 ft
/// bracketed in a cold, low tropopause (200 hPa can sit near 37,000 ft).
pub const GFS_PRESSURE_LEVELS_HPA: [u32; 12] = [950, 925, 900, 850, 800, 700, 600, 500, 400, 300, 250, 200];
const GFS_TOP_LEVEL_HPA: u32 = 150;

fn all_levels() -> impl Iterator<Item = u32> {
    GFS_PRESSURE_LEVELS_HPA.into_iter().chain([GFS_TOP_LEVEL_HPA])
}

/// The `hourly=` variable list for an Open-Meteo GFS request.
pub fn gfs_hourly_variables() -> String {
    all_levels()
        .flat_map(|p| {
            [
                format!("wind_speed_{p}hPa"),
                format!("wind_direction_{p}hPa"),
                format!("temperature_{p}hPa"),
                format!("geopotential_height_{p}hPa"),
            ]
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// One model level at one hour.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LevelSample {
    pub height_m: f64,
    pub speed_kt: f64,
    pub direction_deg: f64,
    pub temp_c: f64,
}

/// One station's hourly vertical profiles.
#[derive(Debug, Clone, PartialEq)]
pub struct StationProfile {
    pub station_id: String,
    pub lat: f64,
    pub lon: f64,
    pub elevation_m: f64,
    /// Parallel to [`GfsWindsForecast::times`]; each entry is that hour's
    /// levels sorted by ascending height. Levels with any missing value
    /// are dropped.
    pub hourly: Vec<Vec<LevelSample>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GfsWindsForecast {
    /// Unix seconds when this was fetched.
    pub fetched_at: i64,
    /// Unix seconds, one per forecast hour.
    pub times: Vec<i64>,
    pub stations: Vec<StationProfile>,
}

/// One location's entry in an Open-Meteo multi-location response (an
/// array, in request order). Only what we read is modeled; `hourly`
/// holds `time` plus one array per requested variable.
#[derive(Debug, Deserialize)]
pub struct OpenMeteoLocation {
    pub elevation: f64,
    pub hourly: std::collections::HashMap<String, Vec<Option<f64>>>,
}

/// Turns one Open-Meteo location into a [`StationProfile`] plus its time
/// axis. `None` if the response lacks the `time` array.
pub fn profile_from_open_meteo(
    station: (&str, f64, f64),
    loc: &OpenMeteoLocation,
) -> Option<(Vec<i64>, StationProfile)> {
    let times: Vec<i64> = loc.hourly.get("time")?.iter().map(|t| t.unwrap_or_default() as i64).collect();
    let get = |name: String, i: usize| loc.hourly.get(&name).and_then(|v| v.get(i).copied().flatten());
    let hourly = (0..times.len())
        .map(|i| {
            let mut levels: Vec<LevelSample> = all_levels()
                .filter_map(|p| {
                    Some(LevelSample {
                        height_m: get(format!("geopotential_height_{p}hPa"), i)?,
                        speed_kt: get(format!("wind_speed_{p}hPa"), i)?,
                        direction_deg: get(format!("wind_direction_{p}hPa"), i)?,
                        temp_c: get(format!("temperature_{p}hPa"), i)?,
                    })
                })
                .collect();
            levels.sort_by(|a, b| a.height_m.total_cmp(&b.height_m));
            levels
        })
        .collect();
    Some((
        times,
        StationProfile {
            station_id: station.0.to_string(),
            lat: station.1,
            lon: station.2,
            elevation_m: loc.elevation,
            hourly,
        },
    ))
}

const FT_PER_M: f64 = 3.280_84;

/// Wind (as u/v, so interpolation doesn't wrap through 0°/360° wrongly)
/// and temperature at `height_m`, linearly interpolated in height
/// between the bracketing levels; clamped to the end levels outside
/// them. `None` for an empty profile.
fn interpolate(levels: &[LevelSample], height_m: f64) -> Option<(f64, f64, f64)> {
    let uvt = |s: &LevelSample| {
        // Meteorological direction is where the wind blows FROM.
        let rad = s.direction_deg.to_radians();
        (-s.speed_kt * rad.sin(), -s.speed_kt * rad.cos(), s.temp_c)
    };
    let first = levels.first()?;
    let last = levels.last()?;
    if height_m <= first.height_m {
        return Some(uvt(first));
    }
    if height_m >= last.height_m {
        return Some(uvt(last));
    }
    let upper = levels.iter().position(|s| s.height_m >= height_m)?;
    let (lo, hi) = (&levels[upper - 1], &levels[upper]);
    let f = (height_m - lo.height_m) / (hi.height_m - lo.height_m);
    let (u0, v0, t0) = uvt(lo);
    let (u1, v1, t1) = uvt(hi);
    Some((u0 + f * (u1 - u0), v0 + f * (v1 - v0), t0 + f * (t1 - t0)))
}

fn to_wind(u: f64, v: f64) -> Wind {
    let speed = (u * u + v * v).sqrt().round();
    if speed < 5.0 {
        return Wind::LightAndVariable;
    }
    let from_deg = (-u).atan2(-v).to_degrees().rem_euclid(360.0);
    let mut direction = ((from_deg / 10.0).round() as u32) * 10;
    if direction == 0 {
        direction = 360;
    }
    Wind::Directional {
        direction_deg: direction,
        speed_kt: speed as u32,
    }
}

/// FD levels for one station's profile at one hour.
pub fn station_levels(levels: &[LevelSample], elevation_m: f64) -> Vec<WindsAloftLevel> {
    let elevation_ft = elevation_m * FT_PER_M;
    FD_LOW_LEVELS_FT
        .into_iter()
        .filter(|&alt| f64::from(alt) >= elevation_ft + 1500.0)
        .filter_map(|alt| {
            let (u, v, t) = interpolate(levels, f64::from(alt) / FT_PER_M)?;
            let temp_c = (alt != 3000 && f64::from(alt) >= elevation_ft + 2500.0).then(|| t.round() as i32);
            Some(WindsAloftLevel {
                altitude_ft: alt,
                wind: to_wind(u, v),
                temp_c,
            })
        })
        .collect()
}

/// FD-style `DDHHMMZ`.
fn ddhhmm(unix: i64) -> String {
    let secs_of_day = unix.rem_euclid(86_400);
    // Day-of-month from days since epoch (civil-from-days, H. Hinnant).
    let z = unix.div_euclid(86_400) + 719_468;
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    format!("{:02}{:02}{:02}Z", day, secs_of_day / 3600, (secs_of_day % 3600) / 60)
}

/// The bulletin for the forecast hour nearest `valid_at` (unix seconds).
/// `None` if the forecast has no hours.
pub fn bulletin_at(forecast: &GfsWindsForecast, valid_at: i64) -> Option<WindsAloftBulletin> {
    let (idx, &time) = forecast
        .times
        .iter()
        .enumerate()
        .min_by_key(|(_, &t)| (t - valid_at).abs())?;
    Some(WindsAloftBulletin {
        data_based_on: ddhhmm(forecast.fetched_at),
        valid_time: ddhhmm(time),
        for_use: "GFS model via Open-Meteo (replaces the retired NWS FD product)".to_string(),
        stations: forecast
            .stations
            .iter()
            .map(|s| StationWindsAloft {
                station_id: s.station_id.clone(),
                levels: s
                    .hourly
                    .get(idx)
                    .map(|levels| station_levels(levels, s.elevation_m))
                    .unwrap_or_default(),
            })
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn level(height_m: f64, speed_kt: f64, direction_deg: f64, temp_c: f64) -> LevelSample {
        LevelSample {
            height_m,
            speed_kt,
            direction_deg,
            temp_c,
        }
    }

    #[test]
    fn interpolates_direction_across_north_without_wrapping_through_south() {
        // 350° and 010° at equal speed, halfway → 360°, not 180°.
        let levels = [level(1000.0, 20.0, 350.0, 0.0), level(2000.0, 20.0, 10.0, -10.0)];
        let (u, v, t) = interpolate(&levels, 1500.0).unwrap();
        assert_eq!(to_wind(u, v), Wind::Directional { direction_deg: 360, speed_kt: 20 });
        assert!((t + 5.0).abs() < 1e-9);
    }

    #[test]
    fn applies_fd_ground_and_temperature_omission_rules() {
        let levels = [level(0.0, 30.0, 270.0, 10.0), level(13_000.0, 30.0, 270.0, -60.0)];
        // 5,400 ft station: 3,000 and 6,000 are below elevation + 1,500
        // → omitted; 9,000 is ≥ elevation + 2,500, so it has a temp too.
        let out = station_levels(&levels, 5400.0 / FT_PER_M);
        assert_eq!(out.first().unwrap().altitude_ft, 9000);
        assert!(out[0].temp_c.is_some());
        // Sea level: 3,000 ft has a wind but never a temperature.
        let out = station_levels(&levels, 0.0);
        assert_eq!(out[0].altitude_ft, 3000);
        assert_eq!(out[0].temp_c, None);
        assert!(out[1].temp_c.is_some());
        assert_eq!(out.len(), 9);
        // 7,000 ft station: 9,000 is ≥1,500 above (wind) but <2,500 (no temp).
        let out = station_levels(&levels, 7000.0 / FT_PER_M);
        assert_eq!(out[0].altitude_ft, 9000);
        assert_eq!(out[0].temp_c, None);
    }

    #[test]
    fn light_winds_are_light_and_variable() {
        let levels = [level(0.0, 3.0, 90.0, 0.0), level(13_000.0, 4.0, 90.0, 0.0)];
        assert!(station_levels(&levels, 0.0).iter().all(|l| l.wind == Wind::LightAndVariable));
    }

    #[test]
    fn formats_fd_timestamps() {
        // 2026-11-21T19:05:00Z
        assert_eq!(ddhhmm(1_795_287_900), "211905Z");
    }

    #[test]
    fn requests_every_level_including_the_top_bracket() {
        let vars = gfs_hourly_variables();
        assert!(vars.contains("geopotential_height_150hPa"));
        assert_eq!(vars.split(',').count(), 13 * 4);
    }
}
