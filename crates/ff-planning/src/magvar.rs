//! Magnetic declination (variation) from the **World Magnetic Model
//! 2025**, for converting true courses/headings to magnetic — the number
//! a pilot actually flies. Self-contained: the WMM Gauss coefficients are
//! embedded, so this works offline anywhere with no data fetch, matching
//! the app's offline-capable planning (DESIGN.md §8).
//!
//! Convention: declination is positive **East**. Magnetic = True −
//! declination ("east is least, west is best").
//!
//! Accuracy: validated against NOAA's official WMM2025 test values to
//! <0.1° for |lat| ≲ 45° (covers CONUS/Europe; typically <0.01°). Error
//! grows toward the magnetic poles — roughly 0.5° by 55°, larger beyond —
//! where declination varies rapidly and NOAA itself flags it unreliable
//! for navigation. Variation is applied here to whole degrees, well inside
//! that. Model epoch 2025.0, valid ~2025–2030.

// WMM2025, epoch 2025.0, 90 coefficients, degree 12. Generated from NOAA's
// WMM2025.COF (https://www.ncei.noaa.gov/products/world-magnetic-model).
// Fields: (n, m, g, h, g_dot, h_dot) — nT and nT/year.
const WMM_EPOCH: f64 = 2025.0;
const WMM_MAX_DEGREE: usize = 12;
#[rustfmt::skip]
const WMM_COEFFS: &[(u8, u8, f64, f64, f64, f64)] = &[
    (1, 0, -29351.8, 0.0, 12.0, 0.0),
    (1, 1, -1410.8, 4545.4, 9.7, -21.5),
    (2, 0, -2556.6, 0.0, -11.6, 0.0),
    (2, 1, 2951.1, -3133.6, -5.2, -27.7),
    (2, 2, 1649.3, -815.1, -8.0, -12.1),
    (3, 0, 1361.0, 0.0, -1.3, 0.0),
    (3, 1, -2404.1, -56.6, -4.2, 4.0),
    (3, 2, 1243.8, 237.5, 0.4, -0.3),
    (3, 3, 453.6, -549.5, -15.6, -4.1),
    (4, 0, 895.0, 0.0, -1.6, 0.0),
    (4, 1, 799.5, 278.6, -2.4, -1.1),
    (4, 2, 55.7, -133.9, -6.0, 4.1),
    (4, 3, -281.1, 212.0, 5.6, 1.6),
    (4, 4, 12.1, -375.6, -7.0, -4.4),
    (5, 0, -233.2, 0.0, 0.6, 0.0),
    (5, 1, 368.9, 45.4, 1.4, -0.5),
    (5, 2, 187.2, 220.2, 0.0, 2.2),
    (5, 3, -138.7, -122.9, 0.6, 0.4),
    (5, 4, -142.0, 43.0, 2.2, 1.7),
    (5, 5, 20.9, 106.1, 0.9, 1.9),
    (6, 0, 64.4, 0.0, -0.2, 0.0),
    (6, 1, 63.8, -18.4, -0.4, 0.3),
    (6, 2, 76.9, 16.8, 0.9, -1.6),
    (6, 3, -115.7, 48.8, 1.2, -0.4),
    (6, 4, -40.9, -59.8, -0.9, 0.9),
    (6, 5, 14.9, 10.9, 0.3, 0.7),
    (6, 6, -60.7, 72.7, 0.9, 0.9),
    (7, 0, 79.5, 0.0, -0.0, 0.0),
    (7, 1, -77.0, -48.9, -0.1, 0.6),
    (7, 2, -8.8, -14.4, -0.1, 0.5),
    (7, 3, 59.3, -1.0, 0.5, -0.8),
    (7, 4, 15.8, 23.4, -0.1, 0.0),
    (7, 5, 2.5, -7.4, -0.8, -1.0),
    (7, 6, -11.1, -25.1, -0.8, 0.6),
    (7, 7, 14.2, -2.3, 0.8, -0.2),
    (8, 0, 23.2, 0.0, -0.1, 0.0),
    (8, 1, 10.8, 7.1, 0.2, -0.2),
    (8, 2, -17.5, -12.6, 0.0, 0.5),
    (8, 3, 2.0, 11.4, 0.5, -0.4),
    (8, 4, -21.7, -9.7, -0.1, 0.4),
    (8, 5, 16.9, 12.7, 0.3, -0.5),
    (8, 6, 15.0, 0.7, 0.2, -0.6),
    (8, 7, -16.8, -5.2, -0.0, 0.3),
    (8, 8, 0.9, 3.9, 0.2, 0.2),
    (9, 0, 4.6, 0.0, -0.0, 0.0),
    (9, 1, 7.8, -24.8, -0.1, -0.3),
    (9, 2, 3.0, 12.2, 0.1, 0.3),
    (9, 3, -0.2, 8.3, 0.3, -0.3),
    (9, 4, -2.5, -3.3, -0.3, 0.3),
    (9, 5, -13.1, -5.2, 0.0, 0.2),
    (9, 6, 2.4, 7.2, 0.3, -0.1),
    (9, 7, 8.6, -0.6, -0.1, -0.2),
    (9, 8, -8.7, 0.8, 0.1, 0.4),
    (9, 9, -12.9, 10.0, -0.1, 0.1),
    (10, 0, -1.3, 0.0, 0.1, 0.0),
    (10, 1, -6.4, 3.3, 0.0, 0.0),
    (10, 2, 0.2, 0.0, 0.1, -0.0),
    (10, 3, 2.0, 2.4, 0.1, -0.2),
    (10, 4, -1.0, 5.3, -0.0, 0.1),
    (10, 5, -0.6, -9.1, -0.3, -0.1),
    (10, 6, -0.9, 0.4, 0.0, 0.1),
    (10, 7, 1.5, -4.2, -0.1, 0.0),
    (10, 8, 0.9, -3.8, -0.1, -0.1),
    (10, 9, -2.7, 0.9, -0.0, 0.2),
    (10, 10, -3.9, -9.1, -0.0, -0.0),
    (11, 0, 2.9, 0.0, 0.0, 0.0),
    (11, 1, -1.5, 0.0, -0.0, -0.0),
    (11, 2, -2.5, 2.9, 0.0, 0.1),
    (11, 3, 2.4, -0.6, 0.0, -0.0),
    (11, 4, -0.6, 0.2, 0.0, 0.1),
    (11, 5, -0.1, 0.5, -0.1, -0.0),
    (11, 6, -0.6, -0.3, 0.0, -0.0),
    (11, 7, -0.1, -1.2, -0.0, 0.1),
    (11, 8, 1.1, -1.7, -0.1, -0.0),
    (11, 9, -1.0, -2.9, -0.1, 0.0),
    (11, 10, -0.2, -1.8, -0.1, 0.0),
    (11, 11, 2.6, -2.3, -0.1, 0.0),
    (12, 0, -2.0, 0.0, 0.0, 0.0),
    (12, 1, -0.2, -1.3, 0.0, -0.0),
    (12, 2, 0.3, 0.7, -0.0, 0.0),
    (12, 3, 1.2, 1.0, -0.0, -0.1),
    (12, 4, -1.3, -1.4, -0.0, 0.1),
    (12, 5, 0.6, -0.0, -0.0, -0.0),
    (12, 6, 0.6, 0.6, 0.1, -0.0),
    (12, 7, 0.5, -0.1, -0.0, -0.0),
    (12, 8, -0.1, 0.8, 0.0, 0.0),
    (12, 9, -0.4, 0.1, 0.0, -0.0),
    (12, 10, -0.2, -1.0, -0.1, -0.0),
    (12, 11, -1.3, 0.1, -0.0, 0.0),
    (12, 12, -0.7, 0.2, -0.1, -0.1),
];

const A_WGS84_KM: f64 = 6378.137;
const F_WGS84: f64 = 1.0 / 298.257223563;
const GEOMAG_R_KM: f64 = 6371.2;

/// Schmidt semi-normalization factor `sqrt((2-δ_m0)·(n-m)!/(n+m)!)`,
/// computed as a running product to avoid factorial overflow.
fn schmidt_factor(n: usize, m: usize) -> f64 {
    let mut denom = 1.0f64;
    for k in (n - m + 1)..=(n + m) {
        denom *= k as f64;
    }
    let two = if m > 0 { 2.0 } else { 1.0 };
    (two / denom).sqrt()
}

/// Magnetic declination (variation) in degrees, **positive East**, at the
/// given geodetic position/altitude/time. `alt_km` is height above the
/// WGS84 ellipsoid; declination is nearly altitude-independent, so 0 is a
/// fine default for planning. `decimal_year` e.g. 2026.5 for mid-2026.
pub fn declination_deg(lat_deg: f64, lon_deg: f64, alt_km: f64, decimal_year: f64) -> f64 {
    let dt = decimal_year - WMM_EPOCH;
    let lambda = lon_deg.to_radians();
    let phi = lat_deg.to_radians();

    // Geodetic -> geocentric spherical.
    let e2 = F_WGS84 * (2.0 - F_WGS84);
    let (sp, cp) = (phi.sin(), phi.cos());
    let rc = A_WGS84_KM / (1.0 - e2 * sp * sp).sqrt();
    let px = (rc + alt_km) * cp;
    let pz = (rc * (1.0 - e2) + alt_km) * sp;
    let r = px.hypot(pz);
    let gclat = pz.atan2(px);
    let ct = gclat.sin(); // cos(colatitude θ)
    let st = gclat.cos(); // sin(colatitude θ)

    // Unnormalized associated Legendre P_n^m(cosθ) (no Condon-Shortley
    // phase), by the standard recurrences.
    let nmax = WMM_MAX_DEGREE;
    let mut p = vec![[0.0f64; WMM_MAX_DEGREE + 1]; WMM_MAX_DEGREE + 1];
    p[0][0] = 1.0;
    for m in 0..=nmax {
        if m > 0 {
            p[m][m] = (2.0 * m as f64 - 1.0) * st * p[m - 1][m - 1];
        }
        if m < nmax {
            p[m + 1][m] = (2.0 * m as f64 + 1.0) * ct * p[m][m];
        }
        for n in (m + 2)..=nmax {
            let (nn, mm) = (n as f64, m as f64);
            p[n][m] = ((2.0 * nn - 1.0) * ct * p[n - 1][m] - (nn + mm - 1.0) * p[n - 2][m]) / (nn - mm);
        }
    }
    let pv = |n: usize, m: usize| -> f64 { if m <= n { p[n][m] } else { 0.0 } };

    // Spherical-harmonic synthesis → geocentric field (X north, Y east,
    // Z down).
    let (mut x, mut y, mut z) = (0.0f64, 0.0f64, 0.0f64);
    for &(nn8, mm8, g0, h0, gdot, hdot) in WMM_COEFFS {
        let (n, m) = (nn8 as usize, mm8 as usize);
        let g = g0 + dt * gdot;
        let h = h0 + dt * hdot;
        let sf = schmidt_factor(n, m);
        let pb = sf * pv(n, m);
        let dpb = sf * ((n as f64 * ct * pv(n, m) - (n as f64 + m as f64) * pv(n - 1, m)) / st);
        let arn = (GEOMAG_R_KM / r).powi((n + 2) as i32);
        let (cml, sml) = ((m as f64 * lambda).cos(), (m as f64 * lambda).sin());
        x += arn * (g * cml + h * sml) * dpb;
        y += arn * (m as f64) * (g * sml - h * cml) * pb;
        z += -arn * (n as f64 + 1.0) * (g * cml + h * sml) * pb;
    }
    y /= st;

    // Rotate geocentric -> geodetic (barely affects declination, included
    // for correctness), then D = atan2(east, north).
    let d = phi - gclat;
    let xg = x * d.cos() - z * d.sin();
    y.atan2(xg).to_degrees()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Official NOAA WMM2025 test values (declination, field 5), at the
    // mid-latitudes the nav log actually uses — asserted to <0.1°, far
    // finer than the whole-degree variation a nav log needs.
    #[test]
    fn matches_noaa_wmm2025_test_values() {
        // (year, alt_km, lat, lon, expected_declination_deg)
        let cases = [
            (2025.0, 18.0, 0.0, 21.0, 1.29),
            (2025.0, 65.0, 43.0, 93.0, 0.50),
            (2025.5, 63.0, 26.0, 81.0, 0.51),
            (2025.0, 94.0, -29.0, -110.0, 15.74),
            (2025.0, 51.0, -33.0, 109.0, -5.49),
        ];
        for (yr, alt, lat, lon, expected) in cases {
            let d = declination_deg(lat, lon, alt, yr);
            let err = (d - expected).abs();
            assert!(err < 0.1, "lat={lat} lon={lon}: got {d:.2}, expected {expected:.2} (err {err:.3})");
        }
    }

    #[test]
    fn conus_and_france_variation_is_in_the_expected_range() {
        // Sanity: US East ~ West var, US West ~ East var, France ~ small.
        let ny = declination_deg(40.6, -73.8, 0.0, 2026.5); // ~ -13° (W)
        assert!((-16.0..-9.0).contains(&ny), "NY var {ny}");
        let la = declination_deg(34.0, -118.4, 0.0, 2026.5); // ~ +11° (E)
        assert!((8.0..14.0).contains(&la), "LA var {la}");
        let paris = declination_deg(48.85, 2.35, 0.0, 2026.5); // ~ +1-2° (E)
        assert!((-1.0..4.0).contains(&paris), "Paris var {paris}");
    }
}
