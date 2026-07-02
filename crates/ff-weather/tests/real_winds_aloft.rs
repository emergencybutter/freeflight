//! Validates `parse_windtemp_bulletin` against real captured
//! aviationweather.gov "FD" bulletins — no network needed for these
//! tests. See `winds_aloft` module docs for the format details these
//! confirm.
//!
//! `fetches_live_winds_aloft` additionally hits the real API through
//! the actual `WeatherClient` method. Not run by default:
//!
//! ```sh
//! cargo test -p ff-weather --test real_winds_aloft -- --ignored --nocapture
//! ```
use ff_weather::{parse_windtemp_bulletin, Wind, WeatherClient};

#[test]
fn parses_real_low_bulletin_exhaustively() {
    let text = include_str!("fixtures_real_windtemp_low.txt");
    let bulletin = parse_windtemp_bulletin(text).expect("parse real low bulletin");

    assert_eq!(bulletin.data_based_on, "021800Z");
    assert_eq!(bulletin.valid_time, "030000Z");
    assert_eq!(bulletin.stations.len(), 176);

    let abi = bulletin
        .stations
        .iter()
        .find(|s| s.station_id == "ABI")
        .expect("ABI present");
    // "ABI      1616+21 1714+13 2407+07 9900-05 2109-16 222230 212741 222853"
    // — 3000ft column blank (omitted), confirming empty fields don't
    // produce a level at all.
    assert!(!abi.levels.iter().any(|l| l.altitude_ft == 3000));
    let l6000 = abi.levels.iter().find(|l| l.altitude_ft == 6000).unwrap();
    assert_eq!(l6000.wind, Wind::Directional { direction_deg: 160, speed_kt: 16 });
    assert_eq!(l6000.temp_c, Some(21));
    // 18000ft: "9900-05" — light and variable wind, with a temp.
    let l18000 = abi.levels.iter().find(|l| l.altitude_ft == 18000).unwrap();
    assert_eq!(l18000.wind, Wind::LightAndVariable);
    assert_eq!(l18000.temp_c, Some(-5));
    // 30000ft: "222230" — implied-negative temp, no explicit sign.
    let l30000 = abi.levels.iter().find(|l| l.altitude_ft == 30000).unwrap();
    assert_eq!(l30000.wind, Wind::Directional { direction_deg: 220, speed_kt: 22 });
    assert_eq!(l30000.temp_c, Some(-30));

    let abr = bulletin
        .stations
        .iter()
        .find(|s| s.station_id == "ABR")
        .expect("ABR present");
    // "ABR 1411 ..." — 3000ft: wind only, no temperature at all.
    let abr_3000 = abr.levels.iter().find(|l| l.altitude_ft == 3000).unwrap();
    assert_eq!(abr_3000.wind, Wind::Directional { direction_deg: 140, speed_kt: 11 });
    assert_eq!(abr_3000.temp_c, None);

    let atl = bulletin
        .stations
        .iter()
        .find(|s| s.station_id == "ATL")
        .expect("ATL present");
    // "ATL 9900 ..." — bare light-and-variable, no temperature.
    let atl_3000 = atl.levels.iter().find(|l| l.altitude_ft == 3000).unwrap();
    assert_eq!(atl_3000.wind, Wind::LightAndVariable);
    assert_eq!(atl_3000.temp_c, None);
}

#[test]
fn parses_real_high_bulletin_with_different_column_set() {
    let text = include_str!("fixtures_real_windtemp_high.txt");
    let bulletin = parse_windtemp_bulletin(text).expect("parse real high bulletin");
    assert!(!bulletin.stations.is_empty());
    for station in &bulletin.stations {
        for level in &station.levels {
            assert!(level.altitude_ft == 45000 || level.altitude_ft == 53000);
        }
    }
}

#[tokio::test]
#[ignore]
async fn fetches_live_winds_aloft() {
    let client = WeatherClient::new();
    let bulletin = client
        .fetch_winds_aloft("low", "06", "all")
        .await
        .expect("live winds-aloft fetch");
    assert!(!bulletin.stations.is_empty());
}
