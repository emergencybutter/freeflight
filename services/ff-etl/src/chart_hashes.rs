//! Backfilling `chart_catalog.sha256` and `.bytes` into an already-published
//! cycle bundle, in place.
//!
//! `add_chart` records both when it tiles a chart (migrations 0007/0008), so
//! anything built by the current pipeline already has them. This exists for
//! cycles published before that — including the one in production — where
//! re-running the whole ETL to recover two columns would mean re-fetching and
//! re-tiling about 20GB of raster imagery through GDAL for no change in the
//! imagery itself. The PMTiles files are already sitting next to the bundle;
//! the hash and the size can simply be read off them.
//!
//! What the client gains, all of it dormant until these columns are filled
//! (DESIGN.md §8):
//!
//!  - chart downloads get verified instead of being trusted,
//!  - a set can say what it will cost before it starts,
//!  - and archives become reusable across cycles, so the next AIRAC update
//!    stops re-downloading sectionals that did not change.
//!
//! Pure metadata — no raster processing, so unlike `tile_terminal_charts`
//! this needs no GDAL.

use ff_sync::sha256_file_hex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ChartHashError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("opening the cycle bundle failed: {0}")]
    Storage(#[from] ff_storage::StorageError),
    #[error("hashing {path} failed: {source}")]
    Hash {
        path: String,
        #[source]
        source: ff_sync::ChecksumError,
    },
}

#[derive(Debug, Default, PartialEq)]
pub struct BackfillStats {
    /// Rows that gained a hash and a size.
    pub filled: usize,
    /// Rows already carrying both, left alone unless forced.
    pub skipped: usize,
    /// Rows whose archive isn't on this disk — catalogued but not published
    /// here, which is normal for a partial mirror.
    pub missing: usize,
}

/// Fill in the hash and size of every chart in `bundle_path` whose PMTiles
/// archive can be found in `charts_dir`.
///
/// Idempotent: rows that already have both are skipped, so re-running costs
/// a catalogue scan rather than re-hashing 20GB. `force` recomputes
/// regardless, which is what to use if the published files were replaced
/// without the catalogue being updated.
///
/// The bundle is opened through `ff-storage` rather than rusqlite directly,
/// so migrations 0007/0008 are both applied *and recorded*. Adding the
/// columns by hand instead leaves `schema_migrations` behind, and the next
/// client to open the bundle re-runs `ALTER TABLE ... ADD COLUMN`, fails on
/// the duplicate, and reports the whole cycle as unreadable.
pub fn backfill_chart_hashes(
    bundle_path: &Path,
    charts_dir: &Path,
    force: bool,
) -> Result<BackfillStats, ChartHashError> {
    let conn = ff_storage::open(&bundle_path.display().to_string())?;

    let rows: Vec<(String, String, Option<String>, Option<i64>)> = conn
        .prepare("SELECT id, tile_url, sha256, bytes FROM chart_catalog ORDER BY id")?
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })?
        .collect::<Result<_, _>>()?;

    let mut stats = BackfillStats::default();
    // One hash per distinct file, not per row: nothing in the schema stops
    // two catalogue entries pointing at the same archive, and these are
    // hundreds of megabytes each.
    let mut hashed: HashMap<PathBuf, (String, u64)> = HashMap::new();

    for (id, tile_url, sha256, bytes) in rows {
        if !force && sha256.is_some() && bytes.is_some() {
            stats.skipped += 1;
            continue;
        }
        let path = charts_dir.join(archive_file_name(&tile_url));
        let Ok(meta) = std::fs::metadata(&path) else {
            tracing::warn!(chart = %id, path = %path.display(), "no archive on disk; skipping");
            stats.missing += 1;
            continue;
        };

        let (digest, size) = match hashed.get(&path) {
            Some(known) => known.clone(),
            None => {
                tracing::info!(chart = %id, bytes = meta.len(), "hashing");
                let digest = sha256_file_hex(&path).map_err(|source| ChartHashError::Hash {
                    path: path.display().to_string(),
                    source,
                })?;
                let entry = (digest, meta.len());
                hashed.insert(path.clone(), entry.clone());
                entry
            }
        };

        conn.execute(
            "UPDATE chart_catalog SET sha256 = ?1, bytes = ?2 WHERE id = ?3",
            rusqlite::params![digest, size as i64, id],
        )?;
        stats.filled += 1;
    }
    Ok(stats)
}

/// `tile_url` is a serving path (`/bundles/2026-08-06/chart-seattle.pmtiles`);
/// the published files sit together in one directory, so only the last
/// segment identifies the archive.
fn archive_file_name(tile_url: &str) -> &str {
    tile_url.rsplit('/').next().unwrap_or(tile_url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    struct Fixture {
        _dir: tempfile::TempDir,
        bundle: PathBuf,
        charts: PathBuf,
    }

    impl Fixture {
        /// A bundle catalogueing `charts`, each written to disk with the
        /// given contents unless `on_disk` says otherwise.
        fn new(charts: &[(&str, Option<&[u8]>)]) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let bundle = dir.path().join("cycle.sqlite");
            let charts_dir = dir.path().join("published");
            std::fs::create_dir_all(&charts_dir).unwrap();

            let conn = ff_storage::open(&bundle.display().to_string()).unwrap();
            for (slug, contents) in charts {
                let file = format!("chart-{slug}.pmtiles");
                conn.execute(
                    "INSERT INTO chart_catalog
                        (id, name, kind, cycle_id, min_lat, min_lon, max_lat, max_lon, tile_url)
                     VALUES (?1, ?2, 'Sectional', '2026-08-06', 0, 0, 1, 1, ?3)",
                    params![
                        format!("2026-08-06-{slug}"),
                        format!("{slug} Sectional"),
                        format!("/bundles/2026-08-06/{file}")
                    ],
                )
                .unwrap();
                if let Some(bytes) = contents {
                    std::fs::write(charts_dir.join(&file), bytes).unwrap();
                }
            }
            drop(conn);
            Self {
                _dir: dir,
                bundle,
                charts: charts_dir,
            }
        }

        fn run(&self, force: bool) -> BackfillStats {
            backfill_chart_hashes(&self.bundle, &self.charts, force).unwrap()
        }

        fn row(&self, slug: &str) -> (Option<String>, Option<i64>) {
            let conn = rusqlite::Connection::open(&self.bundle).unwrap();
            conn.query_row(
                "SELECT sha256, bytes FROM chart_catalog WHERE id = ?1",
                [format!("2026-08-06-{slug}")],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap()
        }
    }

    #[test]
    fn fills_in_the_hash_and_size_of_a_published_chart() {
        let fixture = Fixture::new(&[("seattle", Some(b"pmtiles bytes"))]);

        let stats = fixture.run(false);

        assert_eq!(stats.filled, 1);
        let (sha, bytes) = fixture.row("seattle");
        assert_eq!(sha.as_deref(), Some(ff_sync::sha256_hex(b"pmtiles bytes").as_str()));
        assert_eq!(bytes, Some(13));
    }

    /// Re-running must not re-hash what is already done — on a real cycle
    /// that would be ~20GB of reading for no change.
    #[test]
    fn a_second_run_skips_rows_that_are_already_filled() {
        let fixture = Fixture::new(&[("seattle", Some(b"pmtiles bytes"))]);
        fixture.run(false);

        let stats = fixture.run(false);

        assert_eq!(stats, BackfillStats { filled: 0, skipped: 1, missing: 0 });
    }

    #[test]
    fn force_recomputes_a_row_whose_archive_was_replaced() {
        let fixture = Fixture::new(&[("seattle", Some(b"original"))]);
        fixture.run(false);
        std::fs::write(
            fixture.charts.join("chart-seattle.pmtiles"),
            b"republished, different bytes",
        )
        .unwrap();

        let stats = fixture.run(true);

        assert_eq!(stats.filled, 1);
        let (sha, _) = fixture.row("seattle");
        assert_eq!(
            sha.as_deref(),
            Some(ff_sync::sha256_hex(b"republished, different bytes").as_str())
        );
    }

    /// A catalogued chart whose archive isn't on this disk is counted and
    /// left null, not treated as an error — a mirror may hold a subset.
    #[test]
    fn a_chart_with_no_archive_on_disk_is_left_alone() {
        let fixture = Fixture::new(&[("seattle", Some(b"here")), ("juneau", None)]);

        let stats = fixture.run(false);

        assert_eq!(stats, BackfillStats { filled: 1, skipped: 0, missing: 1 });
        assert_eq!(fixture.row("juneau"), (None, None));
    }

    /// The columns arrive via `ff-storage`, so the bundle must come out with
    /// the migrations *recorded* — otherwise the next client to open it
    /// re-runs `ADD COLUMN`, fails, and reports the cycle as unreadable.
    #[test]
    fn the_backfilled_bundle_records_its_migrations() {
        let fixture = Fixture::new(&[("seattle", Some(b"here"))]);
        fixture.run(false);

        let conn = rusqlite::Connection::open(&fixture.bundle).unwrap();
        let applied: Vec<i64> = conn
            .prepare("SELECT version FROM schema_migrations ORDER BY version")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();

        assert!(applied.contains(&7), "0007 recorded, got {applied:?}");
        assert!(applied.contains(&8), "0008 recorded, got {applied:?}");
    }
}
