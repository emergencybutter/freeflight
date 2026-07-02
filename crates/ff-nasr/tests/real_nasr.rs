//! Opt-in integration test that runs the parser against a real, current
//! FAA NASR 28-day CSV subscription package. Not run by default — set
//! `NASR_DIR` to the directory the package was unzipped into and run:
//!
//! ```sh
//! NASR_DIR=/path/to/unzipped/nasr cargo test -p ff-nasr --test real_nasr -- --ignored --nocapture
//! ```
use ff_nasr::{
    airport_from_row, frequencies_for_airport, parse_apt_base, parse_apt_runway,
    parse_apt_runway_end, parse_frq, runway_from_rows,
};
use std::collections::HashMap;
use std::fs;

fn nasr_dir() -> String {
    std::env::var("NASR_DIR").expect("set NASR_DIR to an unzipped NASR CSV subscription directory")
}

fn read(dir: &str, name: &str) -> Vec<u8> {
    fs::read(format!("{dir}/{name}")).unwrap_or_else(|e| panic!("failed to read {name}: {e}"))
}

#[test]
#[ignore]
fn parses_a_real_nasr_csv_subscription() {
    let dir = nasr_dir();

    let airports =
        parse_apt_base(&read(&dir, "APT_BASE.csv")).expect("APT_BASE.csv should parse cleanly");
    let runways =
        parse_apt_runway(&read(&dir, "APT_RWY.csv")).expect("APT_RWY.csv should parse cleanly");
    let runway_ends = parse_apt_runway_end(&read(&dir, "APT_RWY_END.csv"))
        .expect("APT_RWY_END.csv should parse cleanly");
    let freqs = parse_frq(&read(&dir, "FRQ.csv")).expect("FRQ.csv should parse cleanly");

    println!("airports: {}", airports.len());
    println!("runways: {}", runways.len());
    println!("runway ends: {}", runway_ends.len());
    println!("freq rows: {}", freqs.len());
    assert!(
        airports.len() > 10_000,
        "expected the full nationwide airport list"
    );
    assert!(runways.len() > 10_000);
    assert!(runway_ends.len() > 10_000);
    assert!(freqs.len() > 10_000);

    // Every airport row should convert without panicking.
    let converted: Vec<_> = airports.iter().map(airport_from_row).collect();
    assert_eq!(converted.len(), airports.len());

    // Spot-check a real, known airport end to end.
    let ksfo_row = airports
        .iter()
        .find(|a| a.arpt_id == "SFO")
        .expect("SFO should be in a real NASR extract");
    let ksfo = airport_from_row(ksfo_row);
    println!("\nKSFO: {ksfo:?}");
    assert_eq!(ksfo.icao, "KSFO");
    assert_eq!(ksfo.elevation_ft, 13);
    assert!(ksfo.fuel_types.contains(&"100LL".to_string()));
    assert!((37.0..38.0).contains(&ksfo.lat));

    let ksfo_runways: Vec<_> = runways.iter().filter(|r| r.arpt_id == "SFO").collect();
    let ksfo_ends: Vec<_> = runway_ends
        .iter()
        .filter(|e| e.arpt_id == "SFO")
        .cloned()
        .collect();
    println!(
        "KSFO runways: {}, ends: {}",
        ksfo_runways.len(),
        ksfo_ends.len()
    );
    assert!(!ksfo_runways.is_empty());
    for rwy in &ksfo_runways {
        let runway = runway_from_rows(rwy, &ksfo_ends, "KSFO");
        println!(
            "  {} len={}ft width={}ft surface={:?} low={}({:.0}) high={}({:.0})",
            runway.ident,
            runway.length_ft,
            runway.width_ft,
            runway.surface,
            runway.low_end.ident,
            runway.low_end.heading_deg,
            runway.high_end.ident,
            runway.high_end.heading_deg,
        );
        // Real end coordinates/heading must actually populate, not fall
        // back to the zero-placeholder.
        assert_ne!(runway.low_end.lat, 0.0);
        assert_ne!(runway.high_end.lat, 0.0);
        assert_ne!(runway.low_end.heading_deg, 0.0);
    }

    let paos = frequencies_for_airport(&freqs, "PAO", "KPAO");
    println!("\nKPAO frequencies:");
    for f in &paos {
        println!("  {:?} {} MHz ({:?})", f.kind, f.freq_mhz, f.remarks);
    }
    assert!(paos.iter().any(|f| f.kind == ff_core::FrequencyKind::Ctaf));
    assert!(paos
        .iter()
        .any(|f| f.kind == ff_core::FrequencyKind::Unicom));
}

#[test]
#[ignore]
fn surveys_freq_use_and_site_type_coverage_across_the_whole_file() {
    let dir = nasr_dir();
    let freqs = parse_frq(&read(&dir, "FRQ.csv")).expect("FRQ.csv should parse cleanly");

    let mut site_types: HashMap<String, usize> = HashMap::new();
    let mut kinds: HashMap<String, usize> = HashMap::new();
    let mut unparseable_freq = 0usize;

    for row in &freqs {
        *site_types
            .entry(row.serviced_site_type.clone())
            .or_default() += 1;
        if row.serviced_site_type == "AIRPORT" {
            *kinds
                .entry(format!("{:?}", ff_nasr::freq_use_kind(&row.freq_use)))
                .or_default() += 1;
            if row.freq.parse::<f64>().is_err()
                && !row
                    .freq
                    .chars()
                    .next()
                    .map(|c| c.is_ascii_digit())
                    .unwrap_or(false)
            {
                unparseable_freq += 1;
            }
        }
    }

    println!("=== SERVICED_SITE_TYPE counts ===");
    let mut counts: Vec<_> = site_types.into_iter().collect();
    counts.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    for (k, n) in &counts {
        println!("{k:20} {n}");
    }

    println!("\n=== FrequencyKind coverage among AIRPORT rows ===");
    let mut kind_counts: Vec<_> = kinds.into_iter().collect();
    kind_counts.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    for (k, n) in &kind_counts {
        println!("{k:20} {n}");
    }
    println!("\nunparseable FREQ among AIRPORT rows: {unparseable_freq}");
}
