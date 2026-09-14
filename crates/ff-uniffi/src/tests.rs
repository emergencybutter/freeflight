//! Tests for the binding's own logic, run on the host.
//!
//! Every one of these goes through the public `Freeflight` surface — the
//! same calls Kotlin makes — against a fixture bundle applied through the
//! real sync path, so what is covered is what the app actually depends on
//! and not a parallel arrangement of the same SQL.

use super::*;
use rusqlite::params;
use std::fs::File;

struct Fixture {
    _dir: tempfile::TempDir,
    core: Arc<Freeflight>,
    data_dir: PathBuf,
}

impl Fixture {
    /// A core with nothing downloaded — a first run.
    fn empty() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().join("files");
        let core = Freeflight::new(data_dir.display().to_string());
        Self {
            _dir: dir,
            core,
            data_dir,
        }
    }

    /// A core with `cycle_id` installed, carrying a small but real bundle:
    /// two airports, a runway, a frequency, an approach whose legs end at a
    /// runway threshold, an airspace volume, and a chart catalogue row.
    fn with_cycle(cycle_id: &str) -> Self {
        let fixture = Self::empty();
        let staged = fixture.stage_bundle(cycle_id);
        let sha = ff_sync::sha256_file_hex(&staged).unwrap();
        fixture
            .core
            .apply_cycle(cycle_id.to_string(), staged.display().to_string(), sha)
            .unwrap();
        fixture
    }

    fn stage_bundle(&self, cycle_id: &str) -> PathBuf {
        let staging = self.data_dir.join("staging");
        fs::create_dir_all(&staging).unwrap();
        let path = staging.join("cycle.sqlite");
        let conn = ff_storage::open(&path.display().to_string()).unwrap();
        seed(&conn, cycle_id);
        drop(conn);
        path
    }

    /// Writes a one-tile PMTiles archive where a download would have landed
    /// and installs it, as the chart downloader does.
    fn install_chart(&self, chart_id: &str, tile_png: &[u8]) {
        use pmtiles2::{util::tile_id, Compression, PMTiles, TileType};
        let staged = self.data_dir.join("staged-chart.pmtiles");
        fs::create_dir_all(staged.parent().unwrap()).unwrap();
        let mut archive = PMTiles::new(TileType::Png, Compression::None);
        archive.min_zoom = 8;
        archive.max_zoom = 8;
        archive.add_tile(tile_id(8, 41, 89), tile_png).unwrap();
        archive
            .to_writer(&mut File::create(&staged).unwrap())
            .unwrap();
        self.core
            .install_chart(chart_id.to_string(), staged.display().to_string())
            .unwrap();
    }
}

fn seed(conn: &Connection, cycle_id: &str) {
    conn.execute(
        "INSERT INTO airac_cycle (id, effective_date, source_version) VALUES (?1, ?1, 'test')",
        [cycle_id],
    )
    .unwrap();

    // KSEA: towered, has an approach. S43: a small field with none — the
    // pair the ordering and `has_procedures` assertions below rest on.
    conn.execute(
        "INSERT INTO airport (icao, faa_id, iata, name, lat, lon, elevation_ft, airport_type)
         VALUES ('KSEA', 'SEA', 'SEA', 'Seattle-Tacoma Intl', 47.4502, -122.3088, 433, 'Airport')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO airport (icao, faa_id, iata, name, lat, lon, elevation_ft, airport_type)
         VALUES ('S43', 'S43', NULL, 'Harvey Field', 47.9063, -122.1057, 22, 'Airport')",
        [],
    )
    .unwrap();
    // Far west of the antimeridian, to exercise the wrapped-viewport path.
    conn.execute(
        "INSERT INTO airport (icao, faa_id, iata, name, lat, lon, elevation_ft, airport_type)
         VALUES ('PASY', 'SYA', 'SYA', 'Eareckson AS', 52.7123, 174.1136, 98, 'Airport')",
        [],
    )
    .unwrap();

    conn.execute(
        "INSERT INTO runway (airport_icao, ident, length_ft, width_ft, surface,
                             le_ident, le_lat, le_lon, le_heading_deg,
                             he_ident, he_lat, he_lon, he_heading_deg)
         VALUES ('KSEA', '16L/34R', 11901, 150, 'Concrete',
                 '16L', 47.4636, -122.3088, 160.0,
                 '34R', 47.4309, -122.3088, 340.0)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO frequency (airport_icao, kind, freq_mhz, remarks)
         VALUES ('KSEA', 'TWR', 119.9, NULL)",
        [],
    )
    .unwrap();
    // The shape a real bundle has at a Class B field: one frequency
    // recorded many times over, once per procedure that uses the sector,
    // each of those tagged 'OTHER'. KSEA's real cycle has 99 such rows
    // across 44 distinct frequencies.
    for (kind, remarks) in [
        ("APP", "APCH/P DEP/P"),
        ("APP", "APCH/P DEP/P"),
        ("OTHER", "CLASS B"),
        ("OTHER", "SEATTLE DP"),
        ("OTHER", "SUMMA DP"),
    ] {
        conn.execute(
            "INSERT INTO frequency (airport_icao, kind, freq_mhz, remarks)
             VALUES ('KSEA', ?1, 119.2, ?2)",
            params![kind, remarks],
        )
        .unwrap();
    }

    conn.execute(
        "INSERT INTO navaid (ident, navaid_type, lat, lon, elevation_ft, freq_khz, region)
         VALUES ('SEA', 'VORTAC', 47.4353, -122.3097, 400, 11690, 'K1')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO waypoint (ident, lat, lon, region) VALUES ('HELNS', 47.6, -122.5, 'K1')",
        [],
    )
    .unwrap();

    conn.execute(
        "INSERT INTO procedure (id, airport_icao, kind, ident, runway_ident)
         VALUES ('KSEA-I16L', 'KSEA', 'APPROACH', 'ILS 16L', '16L')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO procedure_transition (id, procedure_id, ident, kind)
         VALUES ('KSEA-I16L-A', 'KSEA-I16L', 'HELNS', 'APPROACH')",
        [],
    )
    .unwrap();
    for (seq, path_and_term, fix) in [
        (1i64, "IF", "HELNS"),
        (2, "TF", "SEA"),
        // ARINC 424's runway-threshold pseudo-fix: in neither the waypoint
        // nor the navaid table, only in `runway`.
        (3, "TF", "RW16L"),
    ] {
        conn.execute(
            "INSERT INTO procedure_leg (transition_id, seq, path_and_term, fix_ident)
             VALUES ('KSEA-I16L-A', ?1, ?2, ?3)",
            params![seq, path_and_term, fix],
        )
        .unwrap();
    }
    conn.execute(
        "INSERT INTO dtpp_chart (airport_icao, procedure_ident, chart_name, pdf_url, cycle)
         VALUES ('KSEA', 'ILS 16L', 'ILS OR LOC RWY 16L', 'https://example.test/i16l.pdf', '2601')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO dtpp_chart (airport_icao, procedure_ident, chart_name, pdf_url, cycle)
         VALUES ('KSEA', 'AIRPORT DIAGRAM', 'AIRPORT DIAGRAM',
                 'https://example.test/apd.pdf', '2601')",
        [],
    )
    .unwrap();

    conn.execute(
        "INSERT INTO airspace (id, name, class, floor, ceiling, boundary_geojson,
                               min_lat, min_lon, max_lat, max_lon)
         VALUES ('KSEA-B', 'SEATTLE CLASS B', 'B', 'SFC', 'MSL:10000',
                 '{\"type\":\"Polygon\",\"coordinates\":[]}', 47.0, -123.0, 48.0, -121.0)",
        [],
    )
    .unwrap();

    conn.execute(
        "INSERT INTO chart_catalog (id, name, kind, cycle_id, min_lat, min_lon, max_lat, max_lon,
                                    tile_url)
         VALUES (?1, 'Seattle Sectional', 'Sectional', ?2, 45.0, -125.0, 49.0, -117.0, ?3)",
        params![
            format!("{cycle_id}-seattle"),
            cycle_id,
            format!("/bundles/{cycle_id}/chart-seattle.pmtiles")
        ],
    )
    .unwrap();

    conn.execute(
        "INSERT INTO data_source (name, effective_date, licence, url, attribution)
         VALUES ('FAA', '2026-07-09', 'Public domain', 'https://faa.gov', 'Source: FAA')",
        [],
    )
    .unwrap();
}

/// A viewport over Sea-Tac only: Harvey Field (47.91N) is deliberately
/// north of its top edge, so "what's in the box" has something to exclude.
fn seattle_bbox() -> BoundingBox {
    BoundingBox {
        min_lat: 47.2,
        min_lon: -123.0,
        max_lat: 47.6,
        max_lon: -122.0,
    }
}

// ---- first run ----------------------------------------------------------

#[test]
fn a_first_run_reports_no_cycle_rather_than_empty_results() {
    let fixture = Fixture::empty();

    assert!(fixture.core.current_cycle().is_none());
    // §11: an unsynced app must be distinguishable from an empty map.
    let err = fixture
        .core
        .airports_in_bbox(seattle_bbox(), 100)
        .unwrap_err();
    assert!(matches!(err, CoreError::NoCycle), "got {err:?}");
    assert!(matches!(
        fixture.core.charts().unwrap_err(),
        CoreError::NoCycle
    ));
}

#[test]
fn any_cycle_is_an_update_when_none_is_installed() {
    let fixture = Fixture::empty();
    assert!(fixture.core.is_update_available("2026-07-09".to_string()));
}

// ---- cycle lifecycle ----------------------------------------------------

#[test]
fn applying_a_bundle_makes_its_contents_queryable() {
    let fixture = Fixture::with_cycle("2026-07-09");

    let cycle = fixture.core.current_cycle().unwrap();
    assert_eq!(cycle.cycle_id, "2026-07-09");
    assert_eq!(cycle.effective_date.as_deref(), Some("2026-07-09"));
    assert_eq!(cycle.airport_count, 3);
    assert_eq!(cycle.procedure_count, 1);
    assert!(cycle.bundle_bytes > 0);
    assert!(cycle.installed_chart_ids.is_empty());
}

#[test]
fn only_a_newer_cycle_counts_as_an_update() {
    let fixture = Fixture::with_cycle("2026-07-09");

    assert!(fixture.core.is_update_available("2026-08-06".to_string()));
    assert!(!fixture.core.is_update_available("2026-07-09".to_string()));
    assert!(!fixture.core.is_update_available("2026-06-11".to_string()));
}

#[test]
fn a_manifest_parses_through_ff_syncs_own_wire_type() {
    let fixture = Fixture::empty();
    let manifest = fixture
        .core
        .parse_manifest(
            r#"{"cycle_id":"2026-08-06","sqlite_url":"https://x.test/cycle.sqlite",
                "sqlite_sha256":"abc123","pmtiles_url":null,"pmtiles_sha256":null}"#
                .to_string(),
        )
        .unwrap();

    assert_eq!(manifest.cycle_id, "2026-08-06");
    assert_eq!(manifest.sqlite_sha256, "abc123");
}

#[test]
fn a_malformed_manifest_is_an_error_not_a_default() {
    let fixture = Fixture::empty();
    let err = fixture
        .core
        .parse_manifest("{\"cycle_id\":\"2026-08-06\"}".to_string())
        .unwrap_err();
    assert!(matches!(err, CoreError::InvalidManifest(_)), "got {err:?}");
}

#[test]
fn a_bundle_whose_checksum_does_not_match_never_becomes_current() {
    let fixture = Fixture::with_cycle("2026-07-09");
    let staged = fixture.stage_bundle("2026-08-06");

    let err = fixture
        .core
        .apply_cycle(
            "2026-08-06".to_string(),
            staged.display().to_string(),
            "0".repeat(64),
        )
        .unwrap_err();

    assert!(matches!(err, CoreError::Sync(_)), "got {err:?}");
    // §8: the previous cycle is still there and still usable.
    assert_eq!(fixture.core.current_cycle().unwrap().cycle_id, "2026-07-09");
    assert_eq!(
        fixture
            .core
            .airport("KSEA".to_string())
            .unwrap()
            .airport
            .icao,
        "KSEA"
    );
}

#[test]
fn a_cycle_swapped_in_under_a_running_app_is_picked_up_without_a_restart() {
    let fixture = Fixture::with_cycle("2026-07-09");
    // Open the bundle, so there is a cached connection to go stale.
    assert_eq!(fixture.core.current_cycle().unwrap().airport_count, 3);

    let staged = fixture.stage_bundle("2026-08-06");
    let conn = ff_storage::open(&staged.display().to_string()).unwrap();
    conn.execute(
        "INSERT INTO airport (icao, faa_id, iata, name, lat, lon, elevation_ft, airport_type)
         VALUES ('KBFI', 'BFI', 'BFI', 'Boeing Field', 47.53, -122.301, 21, 'Airport')",
        [],
    )
    .unwrap();
    drop(conn);
    let sha = ff_sync::sha256_file_hex(&staged).unwrap();
    fixture
        .core
        .apply_cycle("2026-08-06".to_string(), staged.display().to_string(), sha)
        .unwrap();

    let cycle = fixture.core.current_cycle().unwrap();
    assert_eq!(cycle.cycle_id, "2026-08-06");
    assert_eq!(cycle.airport_count, 4);
    assert_eq!(
        fixture
            .core
            .airport("KBFI".to_string())
            .unwrap()
            .airport
            .name,
        "Boeing Field"
    );
}

// ---- queries ------------------------------------------------------------

#[test]
fn a_viewport_returns_the_airports_inside_it_and_nothing_else() {
    let fixture = Fixture::with_cycle("2026-07-09");

    let airports = fixture.core.airports_in_bbox(seattle_bbox(), 100).unwrap();

    let idents: Vec<_> = airports.iter().map(|a| a.icao.as_str()).collect();
    assert_eq!(idents, vec!["KSEA"], "S43 and PASY are outside the box");
}

#[test]
fn a_cropped_viewport_keeps_the_airports_with_procedures() {
    let fixture = Fixture::with_cycle("2026-07-09");
    let wide = BoundingBox {
        min_lat: 47.0,
        min_lon: -123.0,
        max_lat: 48.5,
        max_lon: -121.0,
    };

    let airports = fixture.core.airports_in_bbox(wide, 1).unwrap();

    // Both fields are in the box; the limit must drop the one without
    // procedures, not whichever sorts first by ident (which would be S43).
    assert_eq!(airports.len(), 1);
    assert_eq!(airports[0].icao, "KSEA");
    assert!(airports[0].has_procedures);
}

#[test]
fn a_viewport_across_the_antimeridian_still_finds_airports() {
    let fixture = Fixture::with_cycle("2026-07-09");
    // Crosses 180°, so the western edge is numerically greater than the
    // eastern one — `BETWEEN` would match nothing.
    let aleutians = BoundingBox {
        min_lat: 51.0,
        min_lon: 172.0,
        max_lat: 54.0,
        max_lon: -178.0,
    };

    let airports = fixture.core.airports_in_bbox(aleutians, 100).unwrap();

    assert_eq!(
        airports.iter().map(|a| a.icao.as_str()).collect::<Vec<_>>(),
        vec!["PASY"]
    );
}

#[test]
fn search_matches_idents_and_names_and_ranks_exact_idents_first() {
    let fixture = Fixture::with_cycle("2026-07-09");

    let by_name = fixture.core.search("harvey".to_string(), 10).unwrap();
    assert_eq!(by_name[0].ident, "S43");

    // "SEA" is exactly a navaid ident and a prefix of the airport KSEA;
    // the exact match has to win.
    let by_ident = fixture.core.search("sea".to_string(), 10).unwrap();
    assert_eq!(by_ident[0].ident, "SEA");
    assert_eq!(by_ident[0].kind, "navaid");
    assert!(by_ident.iter().any(|hit| hit.ident == "KSEA"));
}

#[test]
fn an_empty_search_returns_nothing_rather_than_everything() {
    let fixture = Fixture::with_cycle("2026-07-09");
    assert!(fixture
        .core
        .search("   ".to_string(), 10)
        .unwrap()
        .is_empty());
}

#[test]
fn an_airport_carries_its_runways_frequencies_and_diagram() {
    let fixture = Fixture::with_cycle("2026-07-09");

    let detail = fixture.core.airport("ksea".to_string()).unwrap();

    assert_eq!(detail.airport.name, "Seattle-Tacoma Intl");
    assert!(detail.airport.has_procedures);
    assert_eq!(detail.runways.len(), 1);
    assert_eq!(detail.runways[0].length_ft, 11901);
    assert_eq!(detail.frequencies[0].freq_mhz, 119.9);
    assert_eq!(detail.frequencies[0].kind, "TWR");
    assert_eq!(
        detail.airport_diagram_url.as_deref(),
        Some("https://example.test/apd.pdf")
    );
}

#[test]
fn repeated_records_of_one_frequency_collapse_to_a_single_entry() {
    let fixture = Fixture::with_cycle("2026-07-09");

    let frequencies = fixture
        .core
        .airport("KSEA".to_string())
        .unwrap()
        .frequencies;

    // Six rows went in on two distinct frequencies; two come out.
    assert_eq!(frequencies.len(), 2, "got {frequencies:?}");
    let approach = frequencies.iter().find(|f| f.freq_mhz == 119.2).unwrap();
    // 'APP' outranks the 'OTHER' rows for the same number: what a pilot
    // tunes 119.2 for is Approach, not "other".
    assert_eq!(approach.kind, "APP");
    // ...and nothing was thrown away — every distinct remark survives,
    // deduplicated.
    assert_eq!(
        approach.remarks.as_deref(),
        Some("APCH/P DEP/P, CLASS B, SEATTLE DP, SUMMA DP")
    );
}

#[test]
fn frequencies_are_ordered_by_what_a_pilot_reaches_for_first() {
    let fixture = Fixture::with_cycle("2026-07-09");
    let frequencies = fixture
        .core
        .airport("KSEA".to_string())
        .unwrap()
        .frequencies;
    assert_eq!(
        frequencies
            .iter()
            .map(|f| f.kind.as_str())
            .collect::<Vec<_>>(),
        vec!["TWR", "APP"]
    );
}

#[test]
fn an_unknown_airport_is_not_found_rather_than_an_empty_record() {
    let fixture = Fixture::with_cycle("2026-07-09");
    let err = fixture.core.airport("KZZZ".to_string()).unwrap_err();
    assert!(matches!(err, CoreError::NotFound(_)), "got {err:?}");
}

#[test]
fn a_procedures_legs_resolve_to_coordinates_including_the_runway_threshold() {
    let fixture = Fixture::with_cycle("2026-07-09");

    let detail = fixture.core.procedure("KSEA-I16L".to_string()).unwrap();

    assert_eq!(detail.procedure.ident, "ILS 16L");
    assert_eq!(detail.chart_name.as_deref(), Some("ILS OR LOC RWY 16L"));
    let legs = &detail.transitions[0].legs;
    assert_eq!(legs.len(), 3);
    // A waypoint, a navaid, and — the one that needs the runway fallback —
    // the threshold the approach actually ends at. Without it the drawn
    // approach stops short of the runway.
    assert_eq!(legs[0].lat, Some(47.6));
    assert_eq!(legs[1].lat, Some(47.4353));
    assert_eq!(legs[2].fix_ident.as_deref(), Some("RW16L"));
    assert_eq!(legs[2].lat, Some(47.4636));
}

#[test]
fn an_airports_procedures_are_listed_for_it() {
    let fixture = Fixture::with_cycle("2026-07-09");
    let procedures = fixture.core.airport_procedures("KSEA".to_string()).unwrap();
    assert_eq!(procedures.len(), 1);
    assert_eq!(procedures[0].kind, "APPROACH");
    assert_eq!(procedures[0].runway_ident.as_deref(), Some("16L"));
}

#[test]
fn airspace_overlapping_the_viewport_is_returned_even_when_it_dwarfs_it() {
    let fixture = Fixture::with_cycle("2026-07-09");
    // Entirely inside the Class B's footprint: containment would find
    // nothing, overlap must find it.
    let inside = BoundingBox {
        min_lat: 47.4,
        min_lon: -122.4,
        max_lat: 47.5,
        max_lon: -122.2,
    };

    let airspace = fixture.core.airspace_in_bbox(inside).unwrap();

    assert_eq!(airspace.len(), 1);
    assert_eq!(airspace[0].class, "B");
    assert!(airspace[0]
        .boundary_geojson
        .starts_with("{\"type\":\"Polygon\""));
}

#[test]
fn attributions_come_from_the_bundle() {
    let fixture = Fixture::with_cycle("2026-07-09");
    let credits = fixture.core.attributions().unwrap();
    assert_eq!(credits.len(), 1);
    assert_eq!(credits[0].attribution, "Source: FAA");
}

// ---- charts -------------------------------------------------------------

#[test]
fn a_catalogued_chart_reads_as_not_installed_until_it_is() {
    let fixture = Fixture::with_cycle("2026-07-09");

    let charts = fixture.core.charts().unwrap();
    assert_eq!(charts.len(), 1);
    assert_eq!(charts[0].name, "Seattle Sectional");
    // No archive on disk yet, so no zoom range to report.
    assert_eq!((charts[0].min_zoom, charts[0].max_zoom), (0, 0));
    assert_eq!(
        charts[0].tile_url,
        "/bundles/2026-07-09/chart-seattle.pmtiles"
    );
    assert!(!charts[0].installed);

    fixture.install_chart("2026-07-09-seattle", b"\x89PNG fake");

    let charts = fixture.core.charts().unwrap();
    assert!(charts[0].installed);
    assert!(charts[0].installed_bytes > 0);
    // Read back out of the archive's header, not assumed: the map draws
    // nothing outside this range, so a wrong guess blanks the chart.
    assert_eq!((charts[0].min_zoom, charts[0].max_zoom), (8, 8));
    assert_eq!(
        fixture.core.current_cycle().unwrap().installed_chart_ids,
        vec!["2026-07-09-seattle".to_string()]
    );
}

#[test]
fn an_installed_chart_serves_its_tiles_and_nothing_where_it_has_none() {
    let fixture = Fixture::with_cycle("2026-07-09");
    fixture.install_chart("2026-07-09-seattle", b"\x89PNG fake");

    let tile = fixture
        .core
        .chart_tile("2026-07-09-seattle".to_string(), 8, 41, 89)
        .unwrap();
    assert_eq!(tile.as_deref(), Some(&b"\x89PNG fake"[..]));

    // Off the edge of the archive: normal, and must not be an error.
    let gap = fixture
        .core
        .chart_tile("2026-07-09-seattle".to_string(), 8, 200, 200)
        .unwrap();
    assert!(gap.is_none());
}

#[test]
fn a_chart_that_was_never_downloaded_has_no_tiles() {
    let fixture = Fixture::with_cycle("2026-07-09");
    let tile = fixture
        .core
        .chart_tile("2026-07-09-seattle".to_string(), 8, 41, 89)
        .unwrap();
    assert!(tile.is_none());
}

#[test]
fn removing_a_chart_frees_it_and_stops_it_serving_tiles() {
    let fixture = Fixture::with_cycle("2026-07-09");
    fixture.install_chart("2026-07-09-seattle", b"\x89PNG fake");
    // Read one first, so there is an open handle that has to be dropped.
    fixture
        .core
        .chart_tile("2026-07-09-seattle".to_string(), 8, 41, 89)
        .unwrap()
        .unwrap();

    fixture
        .core
        .remove_chart("2026-07-09-seattle".to_string())
        .unwrap();

    assert!(!fixture.core.charts().unwrap()[0].installed);
    assert!(fixture
        .core
        .chart_tile("2026-07-09-seattle".to_string(), 8, 41, 89)
        .unwrap()
        .is_none());
}

#[test]
fn chart_ids_that_would_escape_the_chart_directory_are_refused() {
    let fixture = Fixture::with_cycle("2026-07-09");
    for hostile in ["../../secrets", "a/b", ""] {
        let err = fixture
            .core
            .chart_tile(hostile.to_string(), 8, 41, 89)
            .unwrap_err();
        assert!(matches!(err, CoreError::Chart(_)), "{hostile:?} -> {err:?}");
    }
}

#[test]
fn pruning_reclaims_the_superseded_cycle_and_its_charts() {
    let fixture = Fixture::with_cycle("2026-07-09");
    fixture.install_chart("2026-07-09-seattle", &vec![0u8; 4096]);

    let staged = fixture.stage_bundle("2026-08-06");
    let sha = ff_sync::sha256_file_hex(&staged).unwrap();
    fixture
        .core
        .apply_cycle("2026-08-06".to_string(), staged.display().to_string(), sha)
        .unwrap();

    let freed = fixture.core.prune_old_cycles().unwrap();

    assert!(freed > 4096, "expected bundle + chart bytes, got {freed}");
    assert_eq!(fixture.core.current_cycle().unwrap().cycle_id, "2026-08-06");
    assert_eq!(
        fixture
            .core
            .airport("KSEA".to_string())
            .unwrap()
            .airport
            .icao,
        "KSEA"
    );
}

// ---- shared planning math ------------------------------------------------

#[test]
fn the_planning_math_is_reachable_through_the_binding() {
    // KSEA -> KPDX, give or take: a sanity check that the same core the web
    // client calls through ff-wasm is wired up here, not a test of the math.
    let nm = distance_nm(47.4502, -122.3088, 45.5887, -122.5975);
    assert!((nm - 112.0).abs() < 3.0, "got {nm}");
    let bearing = initial_bearing_deg(47.4502, -122.3088, 45.5887, -122.5975);
    assert!((175.0..=190.0).contains(&bearing), "got {bearing}");
}

#[test]
fn a_route_plans_through_the_binding() {
    let summary = plan_route_json(
        r#"[{"lat":47.4502,"lon":-122.3088},{"lat":45.5887,"lon":-122.5975}]"#.to_string(),
        r#"{"name":"C172","cruise_tas_kt":110.0,"fuel_burn_gph":8.5}"#.to_string(),
        "[null]".to_string(),
        2026.5,
    )
    .unwrap();
    assert!(summary.contains("legs"), "got {summary}");
}

#[test]
fn bad_planning_input_reports_which_input_was_bad() {
    let err = plan_route_json(
        "not json".to_string(),
        "{}".to_string(),
        "[]".to_string(),
        2026.5,
    )
    .unwrap_err();
    assert!(matches!(err, CoreError::InvalidPoints(_)), "got {err:?}");
}
