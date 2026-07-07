//! SQLite schema and migrations (DESIGN.md §6, §8), shared by `ff-etl`
//! (building cycle bundles), `ff-api` (server-side queries for the web
//! client, which has no local database of its own — see §8), and native
//! clients (Android via `ff-uniffi`) that open the schema directly with
//! `rusqlite`.
use rusqlite::Connection;
use thiserror::Error;

/// Ordered list of migrations to apply, each `(version, sql)`. Applied in
/// order inside one transaction per `open()` call; already-applied
/// versions (tracked in `schema_migrations`) are skipped.
const MIGRATIONS: &[(i64, &str)] = &[
    (1, include_str!("migrations/0001_init.sql")),
    (2, include_str!("migrations/0002_airspace_bbox.sql")),
    (3, include_str!("migrations/0003_airway_ident_index.sql")),
    (4, include_str!("migrations/0004_dtpp_chart.sql")),
];

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

/// Open (creating if needed) a freeflight SQLite database at `path` and
/// bring it up to the latest schema version.
pub fn open(path: &str) -> Result<Connection, StorageError> {
    let conn = Connection::open(path)?;
    migrate(&conn)?;
    Ok(conn)
}

/// Open an in-memory database, for tests and short-lived tooling.
pub fn open_in_memory() -> Result<Connection, StorageError> {
    let conn = Connection::open_in_memory()?;
    migrate(&conn)?;
    Ok(conn)
}

fn migrate(conn: &Connection) -> Result<(), StorageError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL);",
    )?;
    for (version, sql) in MIGRATIONS {
        let already_applied: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version = ?1)",
            [version],
            |row| row.get(0),
        )?;
        if already_applied {
            continue;
        }
        conn.execute_batch(sql)?;
        conn.execute(
            "INSERT INTO schema_migrations (version, applied_at) VALUES (?1, datetime('now'))",
            [version],
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_and_creates_expected_tables() {
        let conn = open_in_memory().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'airport'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn migrations_are_idempotent() {
        let conn = open_in_memory().unwrap();
        // Re-running migrate() against an already-migrated connection
        // must not error (schema_migrations should skip re-applying).
        migrate(&conn).unwrap();
    }

    #[test]
    fn can_insert_and_read_back_an_airport() {
        let conn = open_in_memory().unwrap();
        conn.execute(
            "INSERT INTO airport (icao, name, lat, lon, elevation_ft, airport_type) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params!["KSFO", "San Francisco Intl", 37.6188, -122.375, 13, "Airport"],
        )
        .unwrap();
        let name: String = conn
            .query_row("SELECT name FROM airport WHERE icao = 'KSFO'", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(name, "San Francisco Intl");
    }
}
