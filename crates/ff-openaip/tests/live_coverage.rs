//! Coverage report across every state openAIP currently serves as a
//! fallback tier (DESIGN.md §3.1.2), run against the live API.
//!
//! Its job is not a pass/fail on numbers that will drift, but to surface
//! *how much of each country converts and what is lost*, so a gap is a
//! visible figure rather than a silent omission. The one hard assertion
//! is that no country loses its controlled airspace.
//!
//! ```text
//! FF_OPENAIP_API_KEY=… cargo test -p ff-openaip --test live_coverage -- --ignored --nocapture
//! ```

use ff_core::airspace::AirspaceClass;
use ff_openaip::{convert_country, OpenAipClient};

/// The states §3.1.2 covers, with the ICAO region stamped on navaids.
const STATES: &[(&str, &str, &str)] = &[
    ("DE", "ED", "Germany"),
    ("GB", "EG", "United Kingdom"),
    ("CA", "CY", "Canada"),
    ("GL", "BG", "Greenland"),
];

#[tokio::test]
#[ignore = "hits the live openAIP API; needs FF_OPENAIP_API_KEY"]
async fn every_supported_state_converts() {
    let Ok(key) = std::env::var("FF_OPENAIP_API_KEY") else {
        eprintln!("skipping: FF_OPENAIP_API_KEY not set");
        return;
    };
    let client = OpenAipClient::new(key);

    for (cc, region, name) in STATES {
        let airports = client.airports(cc).await.expect("airports");
        let navaids = client.navaids(cc).await.expect("navaids");
        let airspaces = client.airspaces(cc).await.expect("airspaces");
        let data = convert_country(&airports, &navaids, &airspaces, region);

        let controlled = data
            .airspaces
            .iter()
            .filter(|a| {
                matches!(
                    a.class,
                    AirspaceClass::A
                        | AirspaceClass::B
                        | AirspaceClass::C
                        | AirspaceClass::D
                        | AirspaceClass::E
                )
            })
            .count();

        eprintln!(
            "{name:<15} {cc}  airports {:>5}/{:<5} navaids {:>4}/{:<4} airspace {:>5}/{:<5} (controlled {controlled}, skipped {})",
            data.airports.len(),
            airports.len(),
            data.navaids.len(),
            navaids.len(),
            data.airspaces.len(),
            airspaces.len(),
            data.skipped.airspaces,
        );

        // A state with no controlled airspace at all means the class
        // mapping broke for it — the failure mode that would silently
        // remove every VFR airspace warning for that country.
        assert!(
            controlled > 0,
            "{name} converted no controlled airspace — the icaoClass mapping does not hold there"
        );
        assert!(!data.airports.is_empty(), "{name} converted no airports");
    }
}
