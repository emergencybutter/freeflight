//! aviationweather.gov's pre-compiled *cache* bulk files (a different
//! host path than the per-query Data API in `client`): one gzipped file
//! carrying every current report, updated ~once a minute. We use the
//! METAR cache purely to color airport markers by flight category
//! (VFR/MVFR/IFR/LIFR) — pulling ~5,000 stations in a single periodic
//! download is far kinder to upstream than a per-station-list query on
//! every map pan (DESIGN.md §4's "be a good citizen" note).

use crate::WeatherError;
use std::collections::HashMap;
use std::io::Read;

/// Base path for the cache bulk files — note this is *not* the same base
/// as `client::DEFAULT_BASE_URL` (`.../api/data`); the cache lives under
/// `.../data/cache`.
pub const CACHE_BASE_URL: &str = "https://aviationweather.gov/data/cache";

/// The METAR cache file: gzipped CSV, all current worldwide stations.
pub const METAR_CACHE_FILE: &str = "metars.cache.csv.gz";

/// Gunzips and parses `metars.cache.csv.gz` bytes into a
/// `station_id -> flight_category` map. Columns are selected *by header
/// name* (`station_id`, `flight_category`) rather than by index: the
/// file's header repeats `sky_cover`/`cloud_base_ft_agl` for its four
/// cloud layers, so positional parsing would be fragile. Rows whose
/// flight category is empty or the literal text `null` (both used
/// upstream for reports missing ceiling/visibility) are skipped rather
/// than mapped through.
pub fn parse_metar_flight_categories(
    gzipped: &[u8],
) -> Result<HashMap<String, String>, WeatherError> {
    let mut csv_text = String::new();
    flate2::read::GzDecoder::new(gzipped)
        .read_to_string(&mut csv_text)
        .map_err(|e| WeatherError::Cache(format!("gunzip failed: {e}")))?;

    let mut reader = csv::ReaderBuilder::new()
        // The header has duplicate column names (the four cloud layers),
        // which trips csv's default strict header handling.
        .flexible(true)
        .from_reader(csv_text.as_bytes());

    let headers = reader
        .headers()
        .map_err(|e| WeatherError::Cache(format!("bad header: {e}")))?;
    let station_idx = headers
        .iter()
        .position(|h| h == "station_id")
        .ok_or_else(|| WeatherError::Cache("no station_id column".into()))?;
    let category_idx = headers
        .iter()
        .position(|h| h == "flight_category")
        .ok_or_else(|| WeatherError::Cache("no flight_category column".into()))?;

    let mut categories = HashMap::new();
    for record in reader.records() {
        let record = record.map_err(|e| WeatherError::Cache(format!("bad row: {e}")))?;
        let (Some(station), Some(category)) = (record.get(station_idx), record.get(category_idx))
        else {
            continue;
        };
        // The cache writes an empty field for some stations and the
        // literal string "null" for others (both mean "no category") —
        // skip both so callers only ever see the four real categories.
        if station.is_empty() || category.is_empty() || category.eq_ignore_ascii_case("null") {
            continue;
        }
        // A station can appear more than once (e.g. a METAR plus a later
        // SPECI); the file is newest-first, so keep the first seen.
        categories
            .entry(station.to_string())
            .or_insert_with(|| category.to_string());
    }
    Ok(categories)
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::io::Write;

    fn gzip(text: &str) -> Vec<u8> {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(text.as_bytes()).unwrap();
        encoder.finish().unwrap()
    }

    #[test]
    fn parses_station_to_flight_category() {
        // Trimmed to the columns that matter, but keeps the duplicated
        // cloud-layer headers that force `flexible(true)` and by-name
        // column selection.
        let csv = "\
raw_text,station_id,observation_time,sky_cover,cloud_base_ft_agl,sky_cover,cloud_base_ft_agl,flight_category,metar_type
\"KSFO ...\",KSFO,2026-07-12T19:00:00Z,FEW,2000,,,VFR,METAR
\"KJFK ...\",KJFK,2026-07-12T19:00:00Z,OVC,400,,,LIFR,METAR
\"NOCAT ...\",NOCAT,2026-07-12T19:00:00Z,,,,,,METAR
\"NULLCAT ...\",NULLCAT,2026-07-12T19:00:00Z,,,,,null,METAR";
        let map = parse_metar_flight_categories(&gzip(csv)).unwrap();
        assert_eq!(map.get("KSFO").map(String::as_str), Some("VFR"));
        assert_eq!(map.get("KJFK").map(String::as_str), Some("LIFR"));
        // Empty and literal-"null" flight_category rows are skipped.
        assert!(!map.contains_key("NOCAT"));
        assert!(!map.contains_key("NULLCAT"));
    }

    #[test]
    fn keeps_first_seen_when_a_station_repeats() {
        let csv = "\
raw_text,station_id,flight_category
\"speci\",KSFO,IFR
\"metar\",KSFO,VFR";
        let map = parse_metar_flight_categories(&gzip(csv)).unwrap();
        assert_eq!(map.get("KSFO").map(String::as_str), Some("IFR"));
    }
}
