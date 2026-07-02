//! Sanity-checks a freshly built cycle bundle before publishing it,
//! catching a silent upstream regression (a parsing bug, a changed FAA
//! field layout) rather than letting a broken bundle overwrite a good
//! one — DESIGN.md §7's "validate against the previous cycle" step.
use crate::bundle::BundleStats;
use rusqlite::Connection;
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ValidateError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("bundle failed validation: {0}")]
    Failed(String),
}

/// `bundle_path` is the freshly built (not yet published) bundle;
/// `previous_bundle_path` is the currently-published one, if any (`None`
/// on the very first run, when there's nothing to compare against).
pub fn validate_bundle(
    bundle_path: &Path,
    stats: &BundleStats,
    previous_bundle_path: Option<&Path>,
) -> Result<(), ValidateError> {
    if stats.airports == 0 {
        return Err(ValidateError::Failed("bundle has zero airports".to_string()));
    }

    let conn = Connection::open(bundle_path)?;
    {
        let mut stmt = conn.prepare("SELECT icao, lat, lon FROM airport")?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, f64>(1)?,
                row.get::<_, f64>(2)?,
            ))
        })?;
        for row in rows {
            let (icao, lat, lon) = row?;
            if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) || (lat == 0.0 && lon == 0.0) {
                return Err(ValidateError::Failed(format!(
                    "{icao} has implausible coordinates ({lat}, {lon})"
                )));
            }
        }
    }

    let Some(previous_path) = previous_bundle_path else {
        tracing::info!("no previously published cycle to compare against — treating this as the baseline");
        return Ok(());
    };
    if !previous_path.exists() {
        tracing::warn!(
            path = %previous_path.display(),
            "latest.json points at a missing bundle — skipping comparison"
        );
        return Ok(());
    }

    let prev_conn = Connection::open(previous_path)?;
    let prev_airports: usize = prev_conn.query_row("SELECT COUNT(*) FROM airport", [], |row| row.get(0))?;
    // A cycle-to-cycle drop this sharp for the same fixed region means
    // something broke upstream, not real-world airport closures.
    if prev_airports > 0 && stats.airports * 2 < prev_airports {
        return Err(ValidateError::Failed(format!(
            "airport count dropped from {prev_airports} to {} vs. the previous cycle",
            stats.airports
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bundle_with_airports(path: &Path, airports: &[(&str, f64, f64)]) {
        let conn = ff_storage::open(path.to_str().unwrap()).unwrap();
        for (icao, lat, lon) in airports {
            conn.execute(
                "INSERT INTO airport (icao, faa_id, iata, name, lat, lon, elevation_ft, airport_type, fuel_types)
                 VALUES (?1, NULL, NULL, 'Test', ?2, ?3, 0, 'Airport', '')",
                rusqlite::params![icao, lat, lon],
            )
            .unwrap();
        }
    }

    fn stats_for(count: usize) -> BundleStats {
        BundleStats {
            airports: count,
            ..Default::default()
        }
    }

    #[test]
    fn passes_with_no_previous_bundle() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cycle.sqlite");
        bundle_with_airports(&path, &[("KSFO", 37.6, -122.4)]);
        validate_bundle(&path, &stats_for(1), None).unwrap();
    }

    #[test]
    fn rejects_implausible_coordinates() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cycle.sqlite");
        bundle_with_airports(&path, &[("KSFO", 0.0, 0.0)]);
        assert!(validate_bundle(&path, &stats_for(1), None).is_err());
    }

    #[test]
    fn rejects_a_sharp_airport_count_drop_from_the_previous_cycle() {
        let dir = tempfile::tempdir().unwrap();
        let prev_path = dir.path().join("prev.sqlite");
        bundle_with_airports(
            &prev_path,
            &[
                ("KSFO", 37.6, -122.4),
                ("KOAK", 37.7, -122.2),
                ("KSJC", 37.3, -121.9),
                ("KPAO", 37.5, -122.1),
                ("KHWD", 37.7, -122.1),
            ],
        );
        let new_path = dir.path().join("new.sqlite");
        bundle_with_airports(&new_path, &[("KSFO", 37.6, -122.4)]);

        let result = validate_bundle(&new_path, &stats_for(1), Some(&prev_path));
        assert!(result.is_err());
    }

    #[test]
    fn passes_when_airport_count_is_stable_across_cycles() {
        let dir = tempfile::tempdir().unwrap();
        let prev_path = dir.path().join("prev.sqlite");
        bundle_with_airports(&prev_path, &[("KSFO", 37.6, -122.4), ("KOAK", 37.7, -122.2)]);
        let new_path = dir.path().join("new.sqlite");
        bundle_with_airports(&new_path, &[("KSFO", 37.6, -122.4), ("KOAK", 37.7, -122.2)]);

        validate_bundle(&new_path, &stats_for(2), Some(&prev_path)).unwrap();
    }
}
