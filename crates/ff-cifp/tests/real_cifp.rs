//! Opt-in integration test that runs the parser against a real, full FAA
//! CIFP cycle file. Not run by default — the file is ~50MB and isn't
//! checked into the repo. To run:
//!
//! ```sh
//! FF_CIFP_TEST_FILE=/path/to/FAACIFP18 cargo test -p ff-cifp --test real_cifp -- --ignored --nocapture
//! ```
use ff_cifp::{
    build_procedures, classify_line, extract_airport, extract_procedure_leg_row,
    extract_runway_end, pair_runway_ends, RecordCategory,
};
use ff_core::ProcedureKind;
use std::collections::HashMap;

#[test]
#[ignore]
fn parses_a_real_cifp_cycle_file() {
    let path =
        std::env::var("FF_CIFP_TEST_FILE").expect("set FF_CIFP_TEST_FILE to a real CIFP file path");
    let contents = std::fs::read_to_string(&path).expect("failed to read CIFP file");

    let mut category_counts: HashMap<&'static str, usize> = HashMap::new();
    let mut airports = Vec::new();
    let mut runway_ends = Vec::new();
    let mut leg_rows = Vec::new();
    let mut airport_errors = 0usize;
    let mut runway_errors = 0usize;
    let mut leg_errors = 0usize;

    for line in contents.lines() {
        let Some(record) = classify_line(line) else {
            continue;
        };
        let key = match record.category {
            RecordCategory::Airport => "Airport",
            RecordCategory::Runway => "Runway",
            RecordCategory::VhfNavaid => "VhfNavaid",
            RecordCategory::NdbNavaid => "NdbNavaid",
            RecordCategory::Waypoint => "Waypoint",
            RecordCategory::Airway => "Airway",
            RecordCategory::EnrouteCommunication => "EnrouteCommunication",
            RecordCategory::Procedure(ProcedureKind::Sid) => "Procedure::Sid",
            RecordCategory::Procedure(ProcedureKind::Star) => "Procedure::Star",
            RecordCategory::Procedure(ProcedureKind::Approach) => "Procedure::Approach",
            RecordCategory::Unknown => "Unknown",
        };
        *category_counts.entry(key).or_default() += 1;

        match record.category {
            RecordCategory::Airport => match extract_airport(&record) {
                Ok(airport) => airports.push(airport),
                Err(_) => airport_errors += 1,
            },
            RecordCategory::Runway => match extract_runway_end(&record) {
                Ok(end) => runway_ends.push(end),
                Err(_) => runway_errors += 1,
            },
            RecordCategory::Procedure(_) => match extract_procedure_leg_row(&record) {
                Ok(row) => leg_rows.push(row),
                Err(_) => leg_errors += 1,
            },
            _ => {}
        }
    }

    println!("=== record category counts ===");
    let mut counts: Vec<_> = category_counts.into_iter().collect();
    counts.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    for (k, n) in &counts {
        println!("{k:25} {n}");
    }

    println!("\n=== extraction ===");
    println!(
        "airports extracted: {} (errors: {})",
        airports.len(),
        airport_errors
    );
    println!(
        "runway ends extracted: {} (errors: {})",
        runway_ends.len(),
        runway_errors
    );
    println!(
        "procedure leg rows extracted: {} (errors: {})",
        leg_rows.len(),
        leg_errors
    );

    let ksfo = airports
        .iter()
        .find(|a| a.icao == "KSFO")
        .expect("KSFO should be in a real CIFP file");
    println!("\nKSFO: {ksfo:?}");
    assert!(
        (36.0..39.0).contains(&ksfo.lat),
        "KSFO lat out of range: {}",
        ksfo.lat
    );
    assert!(
        (-123.5..-121.5).contains(&ksfo.lon),
        "KSFO lon out of range: {}",
        ksfo.lon
    );
    assert_eq!(ksfo.elevation_ft, 13);
    assert_eq!(ksfo.name.trim(), "SAN FRANCISCO INTL");

    let ksfo_runway_ends: Vec<_> = runway_ends
        .iter()
        .filter(|r| r.airport_icao == "KSFO")
        .cloned()
        .collect();
    println!("KSFO runway ends: {}", ksfo_runway_ends.len());
    let ksfo_runways = pair_runway_ends(&ksfo_runway_ends);
    println!("KSFO paired runways: {}", ksfo_runways.len());
    for rw in &ksfo_runways {
        println!(
            "  {} len={}ft width={}ft hdg={:.0}/{:.0}",
            rw.ident, rw.length_ft, rw.width_ft, rw.low_end.heading_deg, rw.high_end.heading_deg
        );
    }
    assert!(!ksfo_runways.is_empty());

    let ksfo_legs: Vec<_> = leg_rows
        .iter()
        .filter(|r| r.airport_icao == "KSFO")
        .cloned()
        .collect();
    println!("KSFO procedure leg rows: {}", ksfo_legs.len());
    let parsed = build_procedures(&ksfo_legs);
    println!("KSFO procedures: {}", parsed.procedures.len());
    println!("KSFO transitions: {}", parsed.transitions.len());
    println!("KSFO legs: {}", parsed.legs.len());
    assert!(!parsed.procedures.is_empty());

    println!("\nfirst 8 KSFO procedures:");
    for proc in parsed.procedures.iter().take(8) {
        println!(
            "  {:?} {} (runway {:?})",
            proc.kind, proc.ident, proc.runway_ident
        );
    }

    println!("\nfirst 12 legs of the first procedure's first transition:");
    if let Some(first_transition) = parsed.transitions.first() {
        for leg in parsed
            .legs
            .iter()
            .filter(|l| l.transition_id == first_transition.id)
            .take(12)
        {
            println!(
                "  seq={:<4} {:?} fix={:?} course={:?} alt={:?}",
                leg.seq, leg.path_and_term, leg.fix_ident, leg.course_deg, leg.altitude
            );
        }
    }

    // Extraction should succeed for the overwhelming majority of records;
    // a handful of edge cases are fine, but wholesale failure means the
    // column offsets are wrong.
    let error_rate = |errors: usize, ok: usize| errors as f64 / (ok + errors).max(1) as f64;
    let airport_error_rate = error_rate(airport_errors, airports.len());
    let runway_error_rate = error_rate(runway_errors, runway_ends.len());
    let leg_error_rate = error_rate(leg_errors, leg_rows.len());
    println!(
        "\nerror rates: airport={airport_error_rate:.4} runway={runway_error_rate:.4} leg={leg_error_rate:.4}"
    );
    assert!(
        airport_error_rate < 0.01,
        "airport error rate too high: {airport_error_rate}"
    );
    assert!(
        runway_error_rate < 0.01,
        "runway error rate too high: {runway_error_rate}"
    );
    assert!(
        leg_error_rate < 0.01,
        "leg error rate too high: {leg_error_rate}"
    );
}

#[test]
#[ignore]
fn surveys_leg_type_coverage_and_transition_kinds_across_the_whole_file() {
    let path =
        std::env::var("FF_CIFP_TEST_FILE").expect("set FF_CIFP_TEST_FILE to a real CIFP file path");
    let contents = std::fs::read_to_string(&path).expect("failed to read CIFP file");

    let mut all_rows = Vec::new();

    for line in contents.lines() {
        let Some(record) = classify_line(line) else {
            continue;
        };
        if let RecordCategory::Procedure(_) = record.category {
            if let Ok(row) = extract_procedure_leg_row(&record) {
                all_rows.push(row);
            }
        }
    }

    let parsed = build_procedures(&all_rows);

    // Count leg types on the *built* legs, not the raw rows: build_procedures
    // drops continuation records (same airport/procedure/transition/seq as
    // an already-seen leg, different field layout) that would otherwise
    // masquerade as extra legs with a blank path-and-term code.
    let mut leg_type_counts: HashMap<String, usize> = HashMap::new();
    for leg in &parsed.legs {
        *leg_type_counts
            .entry(format!("{:?}", leg.path_and_term))
            .or_default() += 1;
    }

    println!("=== path-and-terminator leg type counts (whole file, post-dedup) ===");
    let mut counts: Vec<_> = leg_type_counts.into_iter().collect();
    counts.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    for (k, n) in &counts {
        println!("{k:20} {n}");
    }

    println!("\n=== transition kind counts (whole file) ===");
    let mut kind_counts: HashMap<String, usize> = HashMap::new();
    for t in &parsed.transitions {
        *kind_counts.entry(format!("{:?}", t.kind)).or_default() += 1;
    }
    for (k, n) in &kind_counts {
        println!("{k:20} {n}");
    }

    let unsupported = counts
        .iter()
        .find(|(k, _)| k == "Unsupported")
        .map(|(_, n)| *n)
        .unwrap_or(0);
    let total: usize = counts.iter().map(|(_, n)| n).sum();
    println!(
        "\nUnsupported leg types: {unsupported} / {total} ({:.2}%)",
        100.0 * unsupported as f64 / total as f64
    );
}

#[test]
#[ignore]
fn diagnoses_unsupported_leg_type_codes() {
    let path =
        std::env::var("FF_CIFP_TEST_FILE").expect("set FF_CIFP_TEST_FILE to a real CIFP file path");
    let contents = std::fs::read_to_string(&path).expect("failed to read CIFP file");

    let mut raw_codes: HashMap<String, usize> = HashMap::new();
    let mut sample_lines: HashMap<String, String> = HashMap::new();

    for line in contents.lines() {
        let Some(record) = classify_line(line) else {
            continue;
        };
        if let RecordCategory::Procedure(_) = record.category {
            if let Ok(row) = extract_procedure_leg_row(&record) {
                if row.path_and_term == ff_core::PathAndTerm::Unsupported {
                    // Re-extract the raw 2-char code directly for diagnosis.
                    let raw = &line[47..49];
                    *raw_codes.entry(raw.to_string()).or_default() += 1;
                    sample_lines
                        .entry(raw.to_string())
                        .or_insert_with(|| line.to_string());
                }
            }
        }
    }

    let mut counts: Vec<_> = raw_codes.into_iter().collect();
    counts.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    println!("=== raw path-and-term codes behind 'Unsupported' ===");
    for (code, n) in &counts {
        println!("{code:?} {n}");
        if let Some(line) = sample_lines.get(code) {
            println!("  sample: {line}");
        }
    }
}
