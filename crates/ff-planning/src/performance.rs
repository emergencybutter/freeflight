//! Aircraft performance tables and lookup (DESIGN.md §9.5.6).
//!
//! A pilot's POH gives true airspeed and fuel burn as a function of
//! altitude, not as the single numbers §9.3's `AircraftProfile` has
//! carried so far. A 172 does not true 110 kt and burn 7.9 gph at both
//! 2,000 ft and 10,000 ft, and planning as though it does quietly
//! mis-states every leg.
//!
//! ## Interpolate between, clamp outside — never extrapolate
//!
//! Lookup is linear between the two bracketing rows. Above the highest
//! row or below the lowest, it returns that endpoint's values unchanged
//! rather than continuing the trend. Extrapolating engine performance
//! past the numbers the pilot actually entered would produce a
//! confident-looking answer with nothing behind it — a 172's cruise TAS
//! does not keep rising linearly to 18,000 ft, and a fuel figure invented
//! that way is exactly the kind of wrong a planning tool must not be.

use serde::{Deserialize, Serialize};

/// One row of a performance table, at a given pressure altitude.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PerformancePoint {
    pub pressure_altitude_ft: f64,
    pub tas_kt: f64,
    pub fuel_gph: f64,
    /// Rate of climb or descent, a positive magnitude — the phase
    /// supplies the sign. `None` for cruise rows.
    #[serde(default)]
    pub vertical_speed_fpm: Option<f64>,
    /// Which power setting this row was recorded at (`"65%"`,
    /// `"2400 RPM"`). Empty for climb/descent, which have no power
    /// dimension. Free text because POHs disagree about what they key
    /// cruise tables on.
    #[serde(default)]
    pub power_setting: String,
}

/// The three tables belonging to one aircraft. Any of them may be empty,
/// in which case the profile's scalar fields are used instead.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AircraftPerformance {
    #[serde(default)]
    pub climb: Vec<PerformancePoint>,
    #[serde(default)]
    pub cruise: Vec<PerformancePoint>,
    #[serde(default)]
    pub descent: Vec<PerformancePoint>,
}

/// Linear interpolation of `rows` at `altitude_ft`, clamped to the
/// table's own range. `None` if there are no rows at all.
///
/// Rows need not arrive sorted — they are ordered here, since the
/// interpolation is meaningless otherwise and the caller has no reason to
/// care.
fn interpolate(rows: &[&PerformancePoint], altitude_ft: f64) -> Option<PerformancePoint> {
    if rows.is_empty() {
        return None;
    }
    let mut sorted: Vec<&PerformancePoint> = rows.to_vec();
    sorted.sort_by(|a, b| {
        a.pressure_altitude_ft
            .partial_cmp(&b.pressure_altitude_ft)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Below the table, or a single row: hold the endpoint.
    let first = sorted[0];
    if altitude_ft <= first.pressure_altitude_ft || sorted.len() == 1 {
        return Some((*first).clone());
    }
    let last = sorted[sorted.len() - 1];
    if altitude_ft >= last.pressure_altitude_ft {
        return Some((*last).clone());
    }

    let upper_index = sorted
        .iter()
        .position(|r| r.pressure_altitude_ft >= altitude_ft)
        .unwrap_or(sorted.len() - 1);
    let upper = sorted[upper_index];
    let lower = sorted[upper_index.saturating_sub(1)];

    let span = upper.pressure_altitude_ft - lower.pressure_altitude_ft;
    // Duplicate altitudes would divide by zero; take the lower row.
    if span <= 0.0 {
        return Some((*lower).clone());
    }
    let t = (altitude_ft - lower.pressure_altitude_ft) / span;
    let blend = |a: f64, b: f64| a + (b - a) * t;
    Some(PerformancePoint {
        pressure_altitude_ft: altitude_ft,
        tas_kt: blend(lower.tas_kt, upper.tas_kt),
        fuel_gph: blend(lower.fuel_gph, upper.fuel_gph),
        vertical_speed_fpm: match (lower.vertical_speed_fpm, upper.vertical_speed_fpm) {
            (Some(a), Some(b)) => Some(blend(a, b)),
            // A partially-filled column is not something to average
            // against a guess.
            _ => None,
        },
        power_setting: lower.power_setting.clone(),
    })
}

impl AircraftPerformance {
    /// Climb performance at `altitude_ft`.
    pub fn climb_at(&self, altitude_ft: f64) -> Option<PerformancePoint> {
        interpolate(&self.climb.iter().collect::<Vec<_>>(), altitude_ft)
    }

    /// Descent performance at `altitude_ft`.
    pub fn descent_at(&self, altitude_ft: f64) -> Option<PerformancePoint> {
        interpolate(&self.descent.iter().collect::<Vec<_>>(), altitude_ft)
    }

    /// Cruise performance at `altitude_ft` for a given power setting.
    ///
    /// A cruise table may hold several power settings, and mixing them
    /// would interpolate between unrelated curves. So: use
    /// `power_setting` if given; if not, and the table records exactly
    /// one setting, use that; otherwise return `None` rather than
    /// silently picking one. An ambiguous table falls back to the
    /// profile's scalar cruise figures, which is at least a number the
    /// pilot typed on purpose.
    pub fn cruise_at(
        &self,
        altitude_ft: f64,
        power_setting: Option<&str>,
    ) -> Option<PerformancePoint> {
        if self.cruise.is_empty() {
            return None;
        }
        let chosen: Option<&str> = match power_setting {
            Some(wanted) => Some(wanted),
            None => {
                let mut settings: Vec<&str> = self
                    .cruise
                    .iter()
                    .map(|r| r.power_setting.as_str())
                    .collect();
                settings.sort_unstable();
                settings.dedup();
                match settings.as_slice() {
                    [only] => Some(*only),
                    _ => None,
                }
            }
        };
        let chosen = chosen?;
        let rows: Vec<&PerformancePoint> = self
            .cruise
            .iter()
            .filter(|r| r.power_setting == chosen)
            .collect();
        interpolate(&rows, altitude_ft)
    }

    /// The distinct power settings the cruise table records, sorted — for
    /// a UI that has to offer the choice.
    pub fn cruise_power_settings(&self) -> Vec<String> {
        let mut settings: Vec<String> = self
            .cruise
            .iter()
            .map(|r| r.power_setting.clone())
            .collect();
        settings.sort();
        settings.dedup();
        settings
    }

    pub fn is_empty(&self) -> bool {
        self.climb.is_empty() && self.cruise.is_empty() && self.descent.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cruise_row(alt: f64, tas: f64, gph: f64, power: &str) -> PerformancePoint {
        PerformancePoint {
            pressure_altitude_ft: alt,
            tas_kt: tas,
            fuel_gph: gph,
            vertical_speed_fpm: None,
            power_setting: power.to_string(),
        }
    }

    fn c172_cruise() -> AircraftPerformance {
        AircraftPerformance {
            cruise: vec![
                cruise_row(2000.0, 105.0, 8.4, "65%"),
                cruise_row(4000.0, 108.0, 8.2, "65%"),
                cruise_row(6000.0, 110.0, 7.9, "65%"),
            ],
            ..Default::default()
        }
    }

    #[test]
    fn interpolates_between_bracketing_rows() {
        let perf = c172_cruise();
        // Halfway between the 4,000 and 6,000 ft rows.
        let at5 = perf.cruise_at(5000.0, None).expect("in range");
        assert!((at5.tas_kt - 109.0).abs() < 1e-9, "TAS {}", at5.tas_kt);
        assert!((at5.fuel_gph - 8.05).abs() < 1e-9, "GPH {}", at5.fuel_gph);

        // Exactly on a row returns that row's numbers.
        let at4 = perf.cruise_at(4000.0, None).expect("on a row");
        assert!((at4.tas_kt - 108.0).abs() < 1e-9);
    }

    #[test]
    fn clamps_outside_the_table_rather_than_extrapolating() {
        let perf = c172_cruise();
        // Well above the highest row: hold 6,000 ft's figures. Continuing
        // the trend would claim ~118 kt at 12,000 ft, which is invented.
        let high = perf.cruise_at(12000.0, None).expect("above range");
        assert!((high.tas_kt - 110.0).abs() < 1e-9, "TAS {}", high.tas_kt);
        assert!((high.fuel_gph - 7.9).abs() < 1e-9);

        // Below the lowest row, likewise.
        let low = perf.cruise_at(0.0, None).expect("below range");
        assert!((low.tas_kt - 105.0).abs() < 1e-9);
    }

    #[test]
    fn unsorted_rows_still_interpolate_correctly() {
        let perf = AircraftPerformance {
            cruise: vec![
                cruise_row(6000.0, 110.0, 7.9, "65%"),
                cruise_row(2000.0, 105.0, 8.4, "65%"),
                cruise_row(4000.0, 108.0, 8.2, "65%"),
            ],
            ..Default::default()
        };
        let at5 = perf.cruise_at(5000.0, None).expect("in range");
        assert!((at5.tas_kt - 109.0).abs() < 1e-9, "TAS {}", at5.tas_kt);
    }

    #[test]
    fn a_single_row_table_is_that_row_everywhere() {
        let perf = AircraftPerformance {
            cruise: vec![cruise_row(6000.0, 110.0, 7.9, "65%")],
            ..Default::default()
        };
        for altitude in [0.0, 6000.0, 17500.0] {
            let point = perf.cruise_at(altitude, None).expect("single row");
            assert!((point.tas_kt - 110.0).abs() < 1e-9);
        }
    }

    #[test]
    fn power_settings_are_never_mixed() {
        let perf = AircraftPerformance {
            cruise: vec![
                cruise_row(4000.0, 108.0, 8.2, "65%"),
                cruise_row(8000.0, 112.0, 7.6, "65%"),
                cruise_row(4000.0, 118.0, 10.5, "75%"),
                cruise_row(8000.0, 122.0, 9.8, "75%"),
            ],
            ..Default::default()
        };

        // Asked for a setting: only that curve is used.
        let economy = perf.cruise_at(6000.0, Some("65%")).expect("65% rows");
        assert!(
            (economy.tas_kt - 110.0).abs() < 1e-9,
            "TAS {}",
            economy.tas_kt
        );
        let fast = perf.cruise_at(6000.0, Some("75%")).expect("75% rows");
        assert!((fast.tas_kt - 120.0).abs() < 1e-9, "TAS {}", fast.tas_kt);

        // Not asked, and the table is ambiguous: refuse rather than
        // average two unrelated curves into a number that is neither.
        assert!(
            perf.cruise_at(6000.0, None).is_none(),
            "an ambiguous cruise table silently picked a power setting"
        );
        assert_eq!(perf.cruise_power_settings(), vec!["65%", "75%"]);

        // An unknown setting matches nothing.
        assert!(perf.cruise_at(6000.0, Some("55%")).is_none());
    }

    #[test]
    fn climb_rows_interpolate_their_vertical_speed() {
        let perf = AircraftPerformance {
            climb: vec![
                PerformancePoint {
                    pressure_altitude_ft: 0.0,
                    tas_kt: 76.0,
                    fuel_gph: 11.0,
                    vertical_speed_fpm: Some(730.0),
                    power_setting: String::new(),
                },
                PerformancePoint {
                    pressure_altitude_ft: 4000.0,
                    tas_kt: 75.0,
                    fuel_gph: 10.5,
                    vertical_speed_fpm: Some(620.0),
                    power_setting: String::new(),
                },
            ],
            ..Default::default()
        };
        let at2 = perf.climb_at(2000.0).expect("in range");
        assert!((at2.vertical_speed_fpm.unwrap() - 675.0).abs() < 1e-9);
        assert!((at2.tas_kt - 75.5).abs() < 1e-9);
    }

    #[test]
    fn an_empty_table_yields_nothing() {
        let perf = AircraftPerformance::default();
        assert!(perf.is_empty());
        assert!(perf.cruise_at(6000.0, None).is_none());
        assert!(perf.climb_at(2000.0).is_none());
        assert!(perf.descent_at(2000.0).is_none());
    }
}
