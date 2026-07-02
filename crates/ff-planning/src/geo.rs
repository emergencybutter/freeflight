//! Great-circle distance and bearing. Coordinates are (lat, lon) in
//! decimal degrees; bearings and courses are true (not magnetic) degrees,
//! 0-360 measured clockwise from north.

/// Mean earth radius in nautical miles (the value the nautical mile is
/// itself defined against, so 1 degree of arc is ~60 nm).
pub const EARTH_RADIUS_NM: f64 = 3440.065;

fn to_radians(deg: f64) -> f64 {
    deg.to_radians()
}

/// Great-circle distance between two points, in nautical miles.
pub fn distance_nm(from: (f64, f64), to: (f64, f64)) -> f64 {
    let (lat1, lon1) = (to_radians(from.0), to_radians(from.1));
    let (lat2, lon2) = (to_radians(to.0), to_radians(to.1));
    let dlat = lat2 - lat1;
    let dlon = lon2 - lon1;
    let a = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().asin();
    EARTH_RADIUS_NM * c
}

/// Initial true course from `from` to `to`, in degrees [0, 360).
pub fn initial_bearing_deg(from: (f64, f64), to: (f64, f64)) -> f64 {
    let (lat1, lon1) = (to_radians(from.0), to_radians(from.1));
    let (lat2, lon2) = (to_radians(to.0), to_radians(to.1));
    let dlon = lon2 - lon1;
    let y = dlon.sin() * lat2.cos();
    let x = lat1.cos() * lat2.sin() - lat1.sin() * lat2.cos() * dlon.cos();
    let bearing = y.atan2(x).to_degrees();
    (bearing + 360.0) % 360.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quarter_circumference_along_the_equator() {
        let d = distance_nm((0.0, 0.0), (0.0, 90.0));
        // 90 degrees of great-circle arc == 90 * 60 = 5400 nm by
        // definition of the nautical mile; allow for the mean-radius
        // approximation.
        assert!((d - 5400.0).abs() < 60.0, "distance was {d} nm");
    }

    #[test]
    fn bearing_due_east_along_the_equator() {
        let b = initial_bearing_deg((0.0, 0.0), (0.0, 90.0));
        assert!((b - 90.0).abs() < 0.5, "bearing was {b} deg");
    }

    #[test]
    fn bearing_due_north_to_the_pole() {
        let b = initial_bearing_deg((0.0, 0.0), (90.0, 0.0));
        assert!((b - 0.0).abs() < 0.5, "bearing was {b} deg");
    }
}
