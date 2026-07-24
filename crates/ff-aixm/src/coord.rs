//! AIXM 4.5 geographical-coordinate parsing.
//!
//! VERIFY: the SIA export encodes `geoLat`/`geoLong` as **DMS with a
//! hemisphere suffix** — latitude `DDMMSS.ss{N|S}` (e.g. `490042.00N`),
//! longitude `DDDMMSS.ss{E|W}` (e.g. `0023259.00E`). This is the classic
//! EUROCONTROL AIXM 4.5 geo encoding, but confirm the exact form (decimal
//! places, whether any rows use signed decimal degrees instead) against a
//! real export. A signed/plain decimal-degree fallback is handled too, so
//! an unexpected decimal row degrades to a best-effort parse rather than
//! being dropped.

/// Parses an AIXM `geoLat` string to signed decimal degrees (S negative).
pub fn parse_lat(s: &str) -> Option<f64> {
    parse_coord(s, 2)
}

/// Parses an AIXM `geoLong` string to signed decimal degrees (W negative).
pub fn parse_lon(s: &str) -> Option<f64> {
    parse_coord(s, 3)
}

/// `deg_digits` is 2 for latitude (`DDMMSS`) or 3 for longitude
/// (`DDDMMSS`) — the width of the degrees field in the DMS form.
fn parse_coord(raw: &str, deg_digits: usize) -> Option<f64> {
    let s = raw.trim();
    if s.is_empty() {
        return None;
    }

    // Peel a hemisphere letter off the end, if present.
    let (body, negative) = match s.chars().last() {
        Some(c @ ('N' | 'S' | 'E' | 'W' | 'n' | 's' | 'e' | 'w')) => {
            (&s[..s.len() - c.len_utf8()], matches!(c, 'S' | 'W' | 's' | 'w'))
        }
        _ => (s, false),
    };
    let body = body.trim();

    let (int_part, frac_part) = match body.split_once('.') {
        Some((i, f)) => (i, f),
        None => (body, ""),
    };

    // DMS form: the integer part is exactly DEG+MM+SS digits, all numeric.
    let dms_len = deg_digits + 4;
    let value = if int_part.len() == dms_len && int_part.bytes().all(|b| b.is_ascii_digit()) {
        let deg: f64 = int_part[..deg_digits].parse().ok()?;
        let min: f64 = int_part[deg_digits..deg_digits + 2].parse().ok()?;
        let sec: f64 = if frac_part.is_empty() {
            int_part[deg_digits + 2..].parse().ok()?
        } else {
            format!("{}.{}", &int_part[deg_digits + 2..], frac_part)
                .parse()
                .ok()?
        };
        deg + min / 60.0 + sec / 3600.0
    } else {
        // Fallback: a plain (possibly signed) decimal-degree value. Keep
        // its own sign — a hemisphere suffix and a leading `-` don't
        // co-occur in practice, and `negative` is false here when the
        // value already carries its sign.
        body.parse::<f64>().ok()?
    };

    Some(if negative { -value } else { value })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn parses_dms_latitude_and_longitude() {
        // 49°00'42.00"N -> 49.011667 ; 002°32'59.00"E -> 2.549722
        assert!(close(parse_lat("490042.00N").unwrap(), 49.0 + 42.0 / 3600.0));
        assert!(close(parse_lon("0023259.00E").unwrap(), 2.0 + 32.0 / 60.0 + 59.0 / 3600.0));
    }

    #[test]
    fn southern_and_western_hemispheres_are_negative() {
        assert!(parse_lat("233000.00S").unwrap() < 0.0);
        assert!(parse_lon("0450000.00W").unwrap() < 0.0);
        assert!(close(parse_lat("233000.00S").unwrap(), -(23.0 + 30.0 / 60.0)));
    }

    #[test]
    fn tolerates_decimal_degree_fallback() {
        assert!(close(parse_lat("49.0128N").unwrap(), 49.0128));
        assert!(close(parse_lon("-2.55").unwrap(), -2.55));
    }

    #[test]
    fn empty_is_none() {
        assert!(parse_lat("").is_none());
        assert!(parse_lat("   ").is_none());
    }
}
