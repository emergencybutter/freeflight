//! Decoders for individual ARINC 424 field encodings, used by
//! `crate::extract`. Column ranges and encodings were cross-checked
//! against the open-source `arinc424` parser
//! (github.com/jack-laverty/arinc424) rather than guessed at from memory
//! — see that project's `src/arinc424/definitions/*.py` for the field
//! tables this was verified against.
use ff_core::{PathAndTerm, TurnDirection};

/// ARINC 424 latitude: 9 characters, `N`/`S` + 2-digit degrees + 2-digit
/// minutes + 2-digit seconds + 2-digit hundredths of a second.
pub fn latitude(raw: &str) -> Option<f64> {
    let raw = raw.trim();
    if raw.len() != 9 {
        return None;
    }
    let sign = match raw.as_bytes()[0] {
        b'N' => 1.0,
        b'S' => -1.0,
        _ => return None,
    };
    dms(sign, &raw[1..3], &raw[3..5], &raw[5..7], &raw[7..9])
}

/// ARINC 424 longitude: 10 characters, `E`/`W` + 3-digit degrees +
/// 2-digit minutes + 2-digit seconds + 2-digit hundredths of a second.
pub fn longitude(raw: &str) -> Option<f64> {
    let raw = raw.trim();
    if raw.len() != 10 {
        return None;
    }
    let sign = match raw.as_bytes()[0] {
        b'E' => 1.0,
        b'W' => -1.0,
        _ => return None,
    };
    dms(sign, &raw[1..4], &raw[4..6], &raw[6..8], &raw[8..10])
}

fn dms(sign: f64, deg: &str, min: &str, sec: &str, hundredths: &str) -> Option<f64> {
    let deg: f64 = deg.parse().ok()?;
    let min: f64 = min.parse().ok()?;
    let sec: f64 = sec.parse().ok()?;
    let hundredths: f64 = hundredths.parse().ok()?;
    Some(sign * (deg + min / 60.0 + (sec + hundredths / 100.0) / 3600.0))
}

/// Course/bearing fields are stored as tenths of a degree (e.g. `"1055"`
/// -> `105.5`), with an optional trailing `T` marking a true (rather than
/// magnetic) value; this scaffold decodes the number but doesn't yet
/// track the true/magnetic distinction separately.
pub fn tenths_of_degree(raw: &str) -> Option<f64> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let raw = raw.strip_suffix('T').unwrap_or(raw);
    let value: f64 = raw.parse().ok()?;
    Some(value / 10.0)
}

pub fn uint(raw: &str) -> Option<u32> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    raw.parse().ok()
}

pub fn signed_int(raw: &str) -> Option<i32> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    raw.parse().ok()
}

pub fn non_empty(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Path and Termination (spec §5.21): a 2-letter ARINC 424 leg type code.
/// Codes not represented in [`PathAndTerm`] (e.g. `FA`, `FC`, `PI`, `AF`)
/// map to `Unsupported` rather than being invented — see DESIGN.md §12.
pub fn path_and_term(raw: &str) -> PathAndTerm {
    match raw.trim() {
        "IF" => PathAndTerm::IF,
        "TF" => PathAndTerm::TF,
        "CF" => PathAndTerm::CF,
        "DF" => PathAndTerm::DF,
        "CA" => PathAndTerm::CA,
        "CD" => PathAndTerm::CD,
        "VA" => PathAndTerm::VA,
        "VI" => PathAndTerm::VI,
        "VD" => PathAndTerm::VD,
        "VM" => PathAndTerm::VM,
        "RF" => PathAndTerm::RF,
        "CI" => PathAndTerm::CI,
        "HA" | "HF" | "HM" => PathAndTerm::HoldingPattern,
        _ => PathAndTerm::Unsupported,
    }
}

/// Turn Direction (spec §5.20): `L`/`R`/`E`(ither); blank means the field
/// doesn't apply to this leg, decoded as `None` rather than a guess.
pub fn turn_direction(raw: &str) -> Option<TurnDirection> {
    match raw.trim() {
        "L" => Some(TurnDirection::Left),
        "R" => Some(TurnDirection::Right),
        "E" => Some(TurnDirection::Either),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_a_northern_eastern_hemisphere_coordinate() {
        // 39 deg 14 min 29.17 sec
        let expected = 39.0 + 14.0 / 60.0 + (29.0 + 17.0 / 100.0) / 3600.0;
        assert!((latitude("N39142917").unwrap() - expected).abs() < 1e-9);

        let expected_lon = -(94.0 + 14.0 / 60.0 + (29.0 + 17.0 / 100.0) / 3600.0);
        assert!((longitude("W094142917").unwrap() - expected_lon).abs() < 1e-9);
    }

    #[test]
    fn rejects_malformed_coordinates() {
        assert_eq!(latitude("N391429"), None); // too short
        assert_eq!(latitude("X39142917"), None); // bad hemisphere letter
        assert_eq!(longitude(""), None);
    }

    #[test]
    fn decodes_tenths_of_a_degree_course() {
        assert_eq!(tenths_of_degree("1055"), Some(105.5));
        assert_eq!(tenths_of_degree("0900T"), Some(90.0));
        assert_eq!(tenths_of_degree("    "), None);
    }

    #[test]
    fn maps_known_leg_types_and_falls_back_to_unsupported() {
        assert_eq!(path_and_term("TF"), PathAndTerm::TF);
        assert_eq!(path_and_term("RF"), PathAndTerm::RF);
        assert_eq!(path_and_term("HM"), PathAndTerm::HoldingPattern);
        assert_eq!(path_and_term("PI"), PathAndTerm::Unsupported);
        assert_eq!(path_and_term("  "), PathAndTerm::Unsupported);
    }
}
