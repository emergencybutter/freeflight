//! Live check of the GFS (Open-Meteo) winds-aloft path end to end. Not
//! run by default:
//!
//! ```sh
//! cargo test -p ff-weather --test real_winds_aloft -- --ignored --nocapture
//! ```
use ff_weather::{bulletin_at, GfsWindsForecast, WeatherClient, Wind, FD_STATIONS};

#[tokio::test]
#[ignore]
async fn fetches_live_gfs_winds_aloft() {
    let pick = |id: &str| *FD_STATIONS.iter().find(|s| s.0 == id).expect(id);
    // Sea-level (SFO), high-elevation (DEN), and mid-continent (ABI).
    let stations = [pick("SFO"), pick("DEN"), pick("ABI")];
    let (times, profiles) = WeatherClient::new()
        .fetch_gfs_profiles(&stations, 3)
        .await
        .expect("live Open-Meteo GFS fetch");
    assert_eq!(times.len(), 3);
    let forecast = GfsWindsForecast {
        fetched_at: times[0],
        times,
        stations: profiles,
    };
    let bulletin = bulletin_at(&forecast, forecast.times[0]).expect("bulletin");
    println!("{bulletin:#?}");

    let sfo = &bulletin.stations[0];
    assert_eq!(sfo.levels.len(), 9, "sea-level station gets every level");
    assert_eq!(sfo.levels[0].temp_c, None, "never a temp at 3,000 ft");
    let den = &bulletin.stations[1];
    assert_eq!(den.levels[0].altitude_ft, 9000, "DEN (~5,400 ft) starts at 9,000");
    for level in bulletin.stations.iter().flat_map(|s| &s.levels) {
        if let Wind::Directional { direction_deg, speed_kt } = level.wind {
            assert!((10..=360).contains(&direction_deg) && direction_deg % 10 == 0);
            assert!((5..250).contains(&speed_kt));
        }
        if let Some(t) = level.temp_c {
            assert!((-80..=40).contains(&t));
        }
    }
}
