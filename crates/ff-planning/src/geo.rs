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

/// The point `fraction` of the way along the great circle from `from` to
/// `to` (0.0 = `from`, 1.0 = `to`), by spherical interpolation. Used to
/// place a computed distance-along-route — a top of climb/descent, say —
/// back onto the map (see [`crate::vertical`]).
pub fn intermediate_point(from: (f64, f64), to: (f64, f64), fraction: f64) -> (f64, f64) {
    let (lat1, lon1) = (to_radians(from.0), to_radians(from.1));
    let (lat2, lon2) = (to_radians(to.0), to_radians(to.1));
    // Angular separation; below ~1e-9 rad (~0.006 nm) the sin() ratios
    // below go singular and the two points are the same point anyway.
    let d = distance_nm(from, to) / EARTH_RADIUS_NM;
    if d < 1e-9 {
        return from;
    }
    let a = ((1.0 - fraction) * d).sin() / d.sin();
    let b = (fraction * d).sin() / d.sin();
    let x = a * lat1.cos() * lon1.cos() + b * lat2.cos() * lon2.cos();
    let y = a * lat1.cos() * lon1.sin() + b * lat2.cos() * lon2.sin();
    let z = a * lat1.sin() + b * lat2.sin();
    (
        z.atan2((x * x + y * y).sqrt()).to_degrees(),
        y.atan2(x).to_degrees(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intermediate_endpoints_are_the_endpoints() {
        let ksfo = (37.6188, -122.375);
        let kmry = (36.5844, -121.8429);
        let start = intermediate_point(ksfo, kmry, 0.0);
        let end = intermediate_point(ksfo, kmry, 1.0);
        assert!((start.0 - ksfo.0).abs() < 1e-9 && (start.1 - ksfo.1).abs() < 1e-9);
        assert!((end.0 - kmry.0).abs() < 1e-9 && (end.1 - kmry.1).abs() < 1e-9);
    }

    #[test]
    fn intermediate_midpoint_is_equidistant() {
        let ksfo = (37.6188, -122.375);
        let kmry = (36.5844, -121.8429);
        let mid = intermediate_point(ksfo, kmry, 0.5);
        let to_start = distance_nm(ksfo, mid);
        let to_end = distance_nm(mid, kmry);
        assert!((to_start - to_end).abs() < 1e-6, "{to_start} vs {to_end}");
        // And it lies on the route: the two halves sum to the whole.
        assert!((to_start + to_end - distance_nm(ksfo, kmry)).abs() < 1e-6);
    }

    #[test]
    fn intermediate_point_of_a_degenerate_leg_is_that_point() {
        let p = (37.6188, -122.375);
        assert_eq!(intermediate_point(p, p, 0.5), p);
    }

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
