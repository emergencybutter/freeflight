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
        return Err(ValidateError::Failed(
            "bundle has zero airports".to_string(),
        ));
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
            if !(-90.0..=90.0).contains(&lat)
                || !(-180.0..=180.0).contains(&lon)
                || (lat == 0.0 && lon == 0.0)
            {
                return Err(ValidateError::Failed(format!(
                    "{icao} has implausible coordinates ({lat}, {lon})"
                )));
            }
        }
    }

    // A bundle may mix AIRAC cycles — the FAA side is fetched
    // automatically while a national AIS export is a manual download, so
    // one source trailing the other by a cycle is routine and no longer
    // blocks the build. What must never happen is a source that can't say
    // how old it is: the clients flag a mixed cycle by comparing each
    // source's date against the bundle's, and a NULL there would read as
    // "same cycle" and quietly claim currency it doesn't have (§11).
    {
        let mut stmt = conn.prepare(
            "SELECT name FROM data_source WHERE effective_date IS NULL OR effective_date = ''",
        )?;
        let undated: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<_, _>>()?;
        if !undated.is_empty() {
            return Err(ValidateError::Failed(format!(
                "these data sources carry no effective date, so clients could not tell a pilot \
                 how current they are: {}",
                undated.join(", ")
            )));
        }
    }

    let cycle_date: Option<String> = conn
        .query_row(
            "SELECT effective_date FROM airac_cycle ORDER BY id DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .ok();
    if let Some(cycle_date) = cycle_date.as_deref() {
        let mut stmt = conn
            .prepare("SELECT name, effective_date FROM data_source WHERE effective_date != ?1")?;
        let mixed: Vec<(String, String)> = stmt
            .query_map([cycle_date], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<_, _>>()?;
        for (name, effective) in &mixed {
            tracing::warn!(
                source = %name,
                source_effective = %effective,
                cycle = %cycle_date,
                "mixed-cycle bundle: this source is from a different AIRAC cycle than the bundle"
            );
        }
    }

    let Some(previous_path) = previous_bundle_path else {
        tracing::info!(
            "no previously published cycle to compare against — treating this as the baseline"
        );
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
    let prev_airports: usize =
        prev_conn.query_row("SELECT COUNT(*) FROM airport", [], |row| row.get(0))?;
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

    /// Records a cycle plus its data sources, the way the pipeline does.
    fn with_sources(path: &Path, cycle: &str, sources: &[(&str, Option<&str>)]) {
        let conn = ff_storage::open(path.to_str().unwrap()).unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO airac_cycle (id, effective_date, source_version)
             VALUES (?1, ?1, 'test')",
            [cycle],
        )
        .unwrap();
        for (name, effective) in sources {
            conn.execute(
                "INSERT OR REPLACE INTO data_source (name, effective_date, licence, url, attribution)
                 VALUES (?1, ?2, 'test', 'https://example.test', ?1)",
                rusqlite::params![name, effective],
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
        bundle_with_airports(
            &prev_path,
            &[("KSFO", 37.6, -122.4), ("KOAK", 37.7, -122.2)],
        );
        let new_path = dir.path().join("new.sqlite");
        bundle_with_airports(&new_path, &[("KSFO", 37.6, -122.4), ("KOAK", 37.7, -122.2)]);

        validate_bundle(&new_path, &stats_for(2), Some(&prev_path)).unwrap();
    }

    #[test]
    fn a_source_from_another_airac_cycle_is_allowed_and_only_warned_about() {
        // The FAA side is fetched automatically, a national AIS export is
        // downloaded by hand — one trailing the other by a cycle is normal
        // and must not cost the whole build.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cycle.sqlite");
        bundle_with_airports(&path, &[("LFPG", 49.0, 2.5)]);
        with_sources(
            &path,
            "2026-10-01",
            &[
                ("FAA", Some("2026-10-01")),
                ("France (SIA)", Some("2026-09-03")),
            ],
        );

        validate_bundle(&path, &stats_for(1), None).unwrap();
    }

    #[test]
    fn a_source_with_no_effective_date_is_refused() {
        // A NULL date reads as "same cycle" to a client comparing against
        // the bundle's, so it would claim a currency it cannot support.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cycle.sqlite");
        bundle_with_airports(&path, &[("LFPG", 49.0, 2.5)]);
        with_sources(
            &path,
            "2026-10-01",
            &[("FAA", Some("2026-10-01")), ("France (SIA)", None)],
        );

        let err = validate_bundle(&path, &stats_for(1), None).unwrap_err();
        assert!(
            format!("{err}").contains("France (SIA)"),
            "the message must name the offending source, got: {err}"
        );
    }

    #[test]
    fn an_empty_effective_date_counts_as_undated_too() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cycle.sqlite");
        bundle_with_airports(&path, &[("LFPG", 49.0, 2.5)]);
        with_sources(&path, "2026-10-01", &[("openAIP", Some(""))]);

        assert!(validate_bundle(&path, &stats_for(1), None).is_err());
    }

}
