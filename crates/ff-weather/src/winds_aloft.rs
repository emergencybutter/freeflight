//! Parses the NWS/FAA winds-and-temperatures-aloft forecast ("FD"
//! bulletin) served as raw fixed-width text at aviationweather.gov's
//! `/api/data/windtemp` — the one product in this crate's coverage with
//! no JSON API.
//!
//! Format confirmed against real captured bulletins for both the "low"
//! (3,000–39,000 ft) and "high" (45,000/53,000 ft) products. The column
//! layout (which altitudes, how wide each field is) is read from each
//! bulletin's own `FT ...` header line rather than hardcoded, so both of
//! those — and any other column set FAA might issue — parse with the
//! same code.
//!
//! Per-station, per-altitude field encoding (confirmed by checking every
//! field across all 176 stations in a real "low" bulletin, not just a
//! few samples):
//! - Empty: no forecast at this altitude for this station (too close to
//!   the ground or otherwise out of range) — no `WindsAloftLevel` at all,
//!   not one with `None` fields.
//! - 4 chars `DDFF`: wind only, no temperature. This is *not* limited to
//!   the lowest column — confirmed live at 6,000 and 9,000 ft too, for
//!   stations near that elevation.
//! - 6 chars `DDFFTT`: wind plus temperature with an *implied* negative
//!   sign (never shown) — used for the bulletin's higher altitude
//!   columns, per its own header note ("TEMPS NEG ABV 24000").
//! - 7 chars `DDFF+TT`/`DDFF-TT`: wind plus an explicitly-signed
//!   temperature.
//! - `DD`/`FF` of `99`/`00` (`"9900..."`) is the sentinel for "light and
//!   variable" wind (under 5kt) — not literally 0° at 0kt.
//! - If `DD > 50`: encodes wind speeds of 100–199kt — true direction is
//!   `(DD - 50) * 10`, true speed is `FF + 100`. Documented in the NWS FD
//!   format spec but not present in any bulletin captured this session
//!   (no jet-stream-strength forecast in that snapshot) — covered here by
//!   a synthetic test instead of a live fixture.
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum WindsAloftError {
    #[error("no 'FT' column header line found in bulletin text")]
    NoHeaderLine,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Wind {
    LightAndVariable,
    Directional { direction_deg: u32, speed_kt: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindsAloftLevel {
    pub altitude_ft: u32,
    pub wind: Wind,
    /// `None` when this altitude is close enough to the station's own
    /// elevation that the product doesn't forecast a temperature there.
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
    /// Raw text after "DATA BASED ON", e.g. `"021800Z"`.
    pub data_based_on: String,
    /// Raw text after "VALID", e.g. `"030000Z"`.
    pub valid_time: String,
    /// Raw text after "FOR USE", e.g. `"2000-0300Z. TEMPS NEG ABV 24000"`
    /// — kept as-is rather than further parsed: it's day/time only (no
    /// month/year), so turning it into a real timestamp needs external
    /// context this parser doesn't have.
    pub for_use: String,
    pub stations: Vec<StationWindsAloft>,
}

struct Column {
    altitude_ft: u32,
    /// 0-indexed, inclusive character position of the header label's
    /// last digit — data fields for this column end at the same
    /// position in every station row.
    end: usize,
}

pub fn parse_windtemp_bulletin(text: &str) -> Result<WindsAloftBulletin, WindsAloftError> {
    let lines: Vec<&str> = text.lines().collect();
    let header_idx = lines
        .iter()
        .position(|l| l.trim_start().starts_with("FT "))
        .ok_or(WindsAloftError::NoHeaderLine)?;

    let data_based_on = lines
        .iter()
        .find(|l| l.starts_with("DATA BASED ON"))
        .map(|l| l.trim_start_matches("DATA BASED ON").trim().to_string())
        .unwrap_or_default();

    let (valid_time, for_use) = lines
        .iter()
        .find(|l| l.starts_with("VALID"))
        .map(|l| parse_valid_line(l))
        .unwrap_or_default();

    let columns = parse_header_columns(lines[header_idx]);

    let stations = lines[header_idx + 1..]
        .iter()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| parse_station_line(l, &columns))
        .collect();

    Ok(WindsAloftBulletin {
        data_based_on,
        valid_time,
        for_use,
        stations,
    })
}

fn parse_valid_line(line: &str) -> (String, String) {
    // "VALID 030000Z   FOR USE 2000-0300Z. TEMPS NEG ABV 24000"
    let valid_time = line
        .trim_start_matches("VALID")
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_string();
    let for_use = line
        .find("FOR USE")
        .map(|i| line[i + "FOR USE".len()..].trim().to_string())
        .unwrap_or_default();
    (valid_time, for_use)
}

fn parse_header_columns(header: &str) -> Vec<Column> {
    let bytes = header.as_bytes();
    let mut columns = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            if let Ok(altitude_ft) = header[start..i].parse() {
                columns.push(Column { altitude_ft, end: i - 1 });
            }
        } else {
            i += 1;
        }
    }
    columns
}

/// The station id occupies the first 3 characters of every row —
/// confirmed live for idents that aren't classic 3-letter codes too
/// (`"T01"`, `"4J3"`, `"2XG"`).
const STATION_ID_END: usize = 2;

fn parse_station_line(line: &str, columns: &[Column]) -> Option<StationWindsAloft> {
    if line.len() <= STATION_ID_END {
        return None;
    }
    let station_id = line[0..=STATION_ID_END].trim().to_string();
    if station_id.is_empty() {
        return None;
    }

    let mut levels = Vec::new();
    let mut prev_end = STATION_ID_END;
    for col in columns {
        let start = prev_end + 1;
        prev_end = col.end;
        if start >= line.len() {
            continue;
        }
        let end = (col.end + 1).min(line.len());
        let field = line[start..end].trim();
        if field.is_empty() {
            continue;
        }
        if let Some((wind, temp_c)) = decode_field(field) {
            levels.push(WindsAloftLevel {
                altitude_ft: col.altitude_ft,
                wind,
                temp_c,
            });
        }
    }

    Some(StationWindsAloft { station_id, levels })
}

fn decode_field(field: &str) -> Option<(Wind, Option<i32>)> {
    let (dir_speed, temp_c) = match field.len() {
        4 => (field, None),
        6 => {
            let (ds, t) = field.split_at(4);
            let magnitude: i32 = t.parse().ok()?;
            (ds, Some(-magnitude))
        }
        7 => {
            let (ds, signed) = field.split_at(4);
            let magnitude: i32 = signed[1..].parse().ok()?;
            let t = if &signed[0..1] == "-" { -magnitude } else { magnitude };
            (ds, Some(t))
        }
        _ => return None,
    };

    if dir_speed == "9900" {
        return Some((Wind::LightAndVariable, temp_c));
    }

    let dd: u32 = dir_speed[0..2].parse().ok()?;
    let ff: u32 = dir_speed[2..4].parse().ok()?;
    let (direction_deg, speed_kt) = if dd > 50 {
        ((dd - 50) * 10, ff + 100)
    } else {
        (dd * 10, ff)
    };

    Some((Wind::Directional { direction_deg, speed_kt }, temp_c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_high_speed_direction_encoding() {
        // Synthetic — this encoding (DD > 50 means +100kt, direction
        // shifted by -50*10) is documented in the NWS FD spec but wasn't
        // present in any bulletin captured this session.
        let (wind, temp) = decode_field("731960").unwrap();
        assert_eq!(wind, Wind::Directional { direction_deg: 230, speed_kt: 119 });
        assert_eq!(temp, Some(-60));
    }

    #[test]
    fn rejects_text_with_no_header_line() {
        assert!(matches!(
            parse_windtemp_bulletin("just some unrelated text"),
            Err(WindsAloftError::NoHeaderLine)
        ));
    }
}
