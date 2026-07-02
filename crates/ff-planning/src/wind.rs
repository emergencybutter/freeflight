use serde::{Deserialize, Serialize};

/// Wind given in the meteorological convention: `direction_true_deg` is
/// the direction the wind is blowing *from*.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Wind {
    pub direction_true_deg: f64,
    pub speed_kt: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WindTriangleResult {
    /// Wind correction angle in degrees; add to true course to get true
    /// heading.
    pub wca_deg: f64,
    pub true_heading_deg: f64,
    pub ground_speed_kt: f64,
}

/// Solve the wind triangle for the heading/groundspeed needed to make good
/// a given true course, standard E6B formulas:
/// `WCA = asin((Vw/Va) * sin(WD - CRS))`, `GS = Va*cos(WCA) - Vw*cos(WD - CRS)`.
pub fn solve(true_course_deg: f64, true_airspeed_kt: f64, wind: Wind) -> WindTriangleResult {
    if true_airspeed_kt <= 0.0 {
        return WindTriangleResult {
            wca_deg: 0.0,
            true_heading_deg: true_course_deg,
            ground_speed_kt: 0.0,
        };
    }
    let beta = (wind.direction_true_deg - true_course_deg).to_radians();
    let ratio = (wind.speed_kt / true_airspeed_kt) * beta.sin();
    // Wind faster than TAS can make the triangle unsolvable (no heading
    // makes good the course); clamp rather than propagate a NaN.
    let wca_rad = ratio.clamp(-1.0, 1.0).asin();
    let wca_deg = wca_rad.to_degrees();
    let ground_speed_kt = true_airspeed_kt * wca_rad.cos() - wind.speed_kt * beta.cos();
    WindTriangleResult {
        wca_deg,
        true_heading_deg: (true_course_deg + wca_deg + 360.0) % 360.0,
        ground_speed_kt,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_headwind_only_reduces_groundspeed() {
        let result = solve(
            360.0,
            100.0,
            Wind {
                direction_true_deg: 360.0,
                speed_kt: 20.0,
            },
        );
        assert!((result.wca_deg).abs() < 0.01);
        assert!((result.ground_speed_kt - 80.0).abs() < 0.01);
    }

    #[test]
    fn direct_tailwind_only_increases_groundspeed() {
        let result = solve(
            360.0,
            100.0,
            Wind {
                direction_true_deg: 180.0,
                speed_kt: 20.0,
            },
        );
        assert!((result.wca_deg).abs() < 0.01);
        assert!((result.ground_speed_kt - 120.0).abs() < 0.01);
    }

    #[test]
    fn crosswind_from_the_right_requires_a_positive_correction() {
        let result = solve(
            360.0,
            100.0,
            Wind {
                direction_true_deg: 90.0,
                speed_kt: 20.0,
            },
        );
        assert!(result.wca_deg > 0.0, "wca was {}", result.wca_deg);
        assert!(result.ground_speed_kt < 100.0);
    }
}
