//! Golden test against the **real openAIP API**, mirroring `ff-aixm`'s
//! `real_sia.rs`: the unit tests prove the conversion logic, this proves
//! the logic still matches what the service actually sends.
//!
//! Skipped (not failed) without a key, so `cargo test` stays green for
//! anyone without one:
//!
//! ```text
//! FF_OPENAIP_API_KEY=… cargo test -p ff-openaip -- --ignored --nocapture
//! ```
//!
//! Marked `#[ignore]` because it makes real network calls against a third
//! party's API; it should run deliberately, not on every `cargo test`.

use ff_core::airspace::{AirspaceClass, SpecialUseKind};
use ff_openaip::{convert_country, OpenAipClient};

fn api_key() -> Option<String> {
    match std::env::var("FF_OPENAIP_API_KEY") {
        Ok(key) if !key.trim().is_empty() => Some(key),
        _ => {
            eprintln!("skipping: FF_OPENAIP_API_KEY not set");
            None
        }
    }
}

#[tokio::test]
#[ignore = "hits the live openAIP API; needs FF_OPENAIP_API_KEY"]
async fn germany_converts_with_the_shape_we_expect() {
    let Some(key) = api_key() else { return };
    let client = OpenAipClient::new(key);

    let airports = client.airports("DE").await.expect("fetch airports");
    let navaids = client.navaids("DE").await.expect("fetch navaids");
    let airspaces = client.airspaces("DE").await.expect("fetch airspaces");

    // Paging actually followed: Germany is ~1364 airports, i.e. more than
    // one page at limit=1000. A paging bug shows up as exactly 1000.
    assert!(
        airports.len() > 1000,
        "expected >1000 German airports (paging), got {}",
        airports.len()
    );
    assert!(navaids.len() > 50, "got {} navaids", navaids.len());
    assert!(airspaces.len() > 500, "got {} airspaces", airspaces.len());

    let data = convert_country(&airports, &navaids, &airspaces, "ED");
    eprintln!(
        "converted: {} airports, {} navaids, {} airspaces (skipped {:?})",
        data.airports.len(),
        data.navaids.len(),
        data.airspaces.len(),
        data.skipped
    );

    // --- identity ------------------------------------------------------
    let eddf = data
        .airports
        .iter()
        .find(|a| a.icao == "EDDF")
        .expect("EDDF present");
    // Frankfurt is ~364 ft; if metres leaked through unconverted this is
    // ~111 and the assertion catches it.
    assert!(
        (300..=420).contains(&eddf.elevation_ft),
        "EDDF elevation {} ft looks like unconverted metres",
        eddf.elevation_ft
    );
    assert!((49.0..51.0).contains(&eddf.lat) && (7.5..9.5).contains(&eddf.lon));

    // Most German entries have no ICAO code and must survive under a
    // marked synthetic key rather than being dropped.
    let synthetic = data
        .airports
        .iter()
        .filter(|a| a.icao.starts_with("OAIP:"))
        .count();
    assert!(
        synthetic > 0,
        "no synthetic keys — airports without an ICAO code are being dropped"
    );

    // --- airspace class, the safety-relevant mapping --------------------
    let ctr = data
        .airspaces
        .iter()
        .find(|a| a.name.starts_with("CTR "))
        .expect("at least one CTR");
    assert_eq!(
        ctr.class,
        AirspaceClass::D,
        "German CTRs are Class D; {} came back as {:?}",
        ctr.name,
        ctr.class
    );

    if let Some(edr) = data.airspaces.iter().find(|a| a.name.contains("ED-R")) {
        assert!(
            matches!(
                edr.class,
                AirspaceClass::SpecialUse(SpecialUseKind::Restricted)
            ),
            "{} should be restricted special-use, got {:?}",
            edr.name,
            edr.class
        );
    }

    // Every boundary must be a real polygon — a degenerate ring would
    // make the VFR crossing check silently never fire.
    for a in &data.airspaces {
        assert!(
            a.boundary.points.len() >= 3,
            "{} has a degenerate boundary",
            a.name
        );
    }

    // --- navaids --------------------------------------------------------
    // ff-core stores kHz; a VHF VOR must be ~108-118 MHz = 108000-118000.
    for n in &data.navaids {
        if let Some(khz) = n.freq_khz {
            assert!(
                (150..=520).contains(&khz) || (108_000..=118_000).contains(&khz),
                "{} has implausible frequency {khz} kHz",
                n.ident
            );
        }
        assert_eq!(n.region, "ED");
    }
    assert!(
        data.navaids.iter().any(|n| n.ident == "BBI"),
        "expected the Berlin-Brandenburg VOR"
    );
}
