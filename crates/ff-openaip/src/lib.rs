//! Reader for **openAIP** community aeronautical data → `ff-core` types
//! (DESIGN.md §3.1.2).
//!
//! # What this is for
//!
//! The primary non-US source is each state's own AIS publication, parsed
//! by [`ff-aixm`](../ff_aixm/index.html). That is the right source and
//! stays the default. But some states — Germany among them — publish
//! usable data under terms that forbid **re-hosting** it, and freeflight
//! bundles data into a client-side cycle. For those, the real choice is
//! not "openAIP versus official AIXM" but **"openAIP versus no coverage
//! at all"**, and this crate is the fallback tier.
//!
//! A state is served by official AIXM *or* by openAIP, never both, so
//! features from the two can never silently conflict.
//!
//! # Licence and attribution
//!
//! openAIP data may be included in commercial software, provided the
//! underlying data stays free to use and the product is not simply
//! reselling the data. freeflight — a planning tool that bundles data,
//! not a data vendor — sits inside that. The obligation that follows is
//! **attribution**: anything derived from this crate must be credited to
//! openAIP wherever it appears, alongside the existing SIA attribution.
//!
//! (An earlier draft of DESIGN.md recorded openAIP as CC BY-NC and cited
//! it when reverting an earlier adoption. That reading was wrong and is
//! corrected in §3.1.2.)
//!
//! # What it deliberately does not import
//!
//! **No waypoints and no airways.** openAIP does not carry enroute RNAV
//! fixes or airways at official completeness — verified first-hand
//! against the live API, and the decisive reason the earlier adoption was
//! reverted. A route with no airways is honest; one with a partial airway
//! network is worse than none, because the gaps stay invisible until a
//! leg silently fails to expand.
//!
//! What it does carry, and this crate reads: **airports, navaids,
//! airspace**. openAIP also publishes VFR **reporting points** (336 for
//! Germany) — the entry/exit points German VFR flying is organised
//! around, with no FAA equivalent — which are a natural follow-up.
//!
//! # Coverage
//!
//! Against the live API (2026-07-26):
//!
//! | State | Airports | Navaids | Airspace | Controlled |
//! |---|---|---|---|---|
//! | Germany `ED` | 1364 / 1364 | 79 | 729 / 745 | 342 |
//! | UK `EG` | 469 / 469 | 136 | 1074 / 1185 | 388 |
//! | Canada `CY` | 1452 / 1452 | 22 | 2264 / 2264 | 2010 |
//! | Greenland `BG` | 77 / 77 | 19 | 26 / 27 | 10 |
//!
//! Canada's 22 navaids is a **coverage warning, not a bug** — Germany has
//! 79 and the UK 136 for far smaller areas. Its airports and airspace are
//! complete.
//!
//! What is still skipped, deliberately: FIR/UIR boundaries (country-sized
//! polygons that would bury every real warning), FIS information sectors,
//! and 91 UK volumes of `type 18` whose meaning could not be established.
//! Unidentified types are skipped rather than guessed.
//!
//! # Provenance is not cosmetic
//!
//! This is community-maintained data sitting in the same tables as
//! official AIS data. Rendered identically, a pilot cannot tell which is
//! which. Callers are expected to record the source per feature and
//! surface it, the way §9.5.3 flags unverified aircraft figures wherever
//! they feed a plan.
//!
//! # Undocumented enums
//!
//! openAIP encodes types, classes and units as bare integers and
//! publishes no schema (`/api/{docs,schema,enums,openapi.json}` all 404).
//! Every mapping in [`convert`] was derived by correlating live data
//! against independently known facts, carries its evidence in a comment,
//! and is asserted in tests against real named airspace and navaids. See
//! that module before changing any of it.

#[cfg(feature = "blocking")]
pub mod blocking;
pub mod client;
pub mod convert;
pub mod model;

#[cfg(feature = "blocking")]
pub use blocking::BlockingOpenAipClient;
pub use client::OpenAipClient;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum OpenAipError {
    #[error("openAIP request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("openAIP {endpoint} returned HTTP {status}: {body}")]
    Http {
        endpoint: String,
        status: u16,
        body: String,
    },
}

/// Everything read for one country, already converted to `ff-core` types.
#[derive(Debug, Clone, Default)]
pub struct OpenAipData {
    pub airports: Vec<ff_core::airport::Airport>,
    pub navaids: Vec<ff_core::navaid::Navaid>,
    pub airspaces: Vec<ff_core::airspace::AirspaceVolume>,
    /// Features that arrived but could not be converted — a missing
    /// position, an unrecognised unit, an airspace whose class could not
    /// be established. Reported rather than silently dropped so an ETL
    /// run can show what it skipped.
    pub skipped: Skipped,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Skipped {
    pub airports: usize,
    pub navaids: usize,
    pub airspaces: usize,
}

/// Convert a fetched country's wire data into `ff-core` types.
///
/// `region` is the ICAO region to stamp on navaids (`"ED"` for Germany) —
/// openAIP carries a country code, which is not the same thing.
pub fn convert_country(
    airports: &[model::Airport],
    navaids: &[model::Navaid],
    airspaces: &[model::Airspace],
    region: &str,
) -> OpenAipData {
    let mut out = OpenAipData::default();
    for (index, a) in airports.iter().enumerate() {
        // openAIP's `_id` is not modelled on the airport wire type, so a
        // stable positional fallback keys the (many) entries with no ICAO
        // code. Callers with the raw id should prefer it.
        match convert::airport(a, &format!("{}-{index}", a.country)) {
            Some(converted) => out.airports.push(converted),
            None => out.skipped.airports += 1,
        }
    }
    for n in navaids {
        match convert::navaid(n, region) {
            Some(converted) => out.navaids.push(converted),
            None => out.skipped.navaids += 1,
        }
    }
    for a in airspaces {
        match convert::airspace(a) {
            Some(converted) => out.airspaces.push(converted),
            None => out.skipped.airspaces += 1,
        }
    }
    out
}
