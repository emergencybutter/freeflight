//! Loads non-US data from **openAIP** for states whose official AIS may
//! not be re-hosted (DESIGN.md §3.1.2) — the fallback tier beside
//! [`crate::aixm`]'s official-AIXM tier.
//!
//! # Licensing
//!
//! openAIP data may be bundled into commercial software provided the
//! underlying data stays free to use and the product is not reselling the
//! data. The obligation is **attribution**: any cycle built with this
//! must credit openAIP in the clients, alongside the SIA attribution the
//! AIXM step records.
//!
//! # A state belongs to exactly one tier
//!
//! Official AIXM and openAIP must never both supply the same state, or
//! the bundle would carry two versions of the same airport with no way to
//! tell which won. That is enforced here rather than left to
//! configuration discipline — see [`load_configured`].
//!
//! # Configuration
//!
//! - `FF_OPENAIP_API_KEY` — required; unset means the step is skipped and
//!   a US-only (or US+France) cycle is unaffected.
//! - `FF_OPENAIP_STATES` — comma-separated `CC:REGION` pairs, e.g.
//!   `DE:ED,GB:EG,CA:CY,GL:BG,IS:BI,IE:EI,PT:LP,ES:LE`. `CC` is openAIP's
//!   two-letter country code; `REGION` is the ICAO region stamped on
//!   navaids, which is *not* the same thing (Canada is `CA` but `CY`).
//!   Defaults to [`DEFAULT_STATES`].

use ff_core::airport::Airport;
use ff_core::airspace::AirspaceVolume;
use ff_core::navaid::Navaid;
use ff_openaip::{convert_country, BlockingOpenAipClient, OpenAipError};
use thiserror::Error;

/// The states §3.1.2 covers today, as `(openAIP country, ICAO region)`.
/// France is deliberately absent: it has an official AIXM export under an
/// open licence, so it belongs to the other tier.
pub const DEFAULT_STATES: &[(&str, &str)] = &[
    ("DE", "ED"), // Germany — DFS forbids re-hosting
    ("GB", "EG"), // United Kingdom — Crown copyright
    ("CA", "CY"), // Canada — NAV CANADA commercial licensing
    ("GL", "BG"), // Greenland — Naviair publishes no dataset at all
    ("IS", "BI"), // Iceland — Isavia publishes no direct AIXM (EAD-gated), eAIP is HTML/PDF only
    ("IE", "EI"), // Ireland — AirNav Ireland is EAD-gated too, same as Iceland
    ("PT", "LP"), // Portugal — NAV Portugal forbids redistribution without prior agreement
    // Spain — ENAIRE does publish AIXM 5.1 directly (unlike most of this
    // list), but ff-aixm only parses 4.5, so that source is unusable
    // until a second parser exists. `region` is stamped "LE" on every
    // navaid, including the Canary Islands' (properly "GC") — the same
    // cosmetic simplification already accepted for France's FR_OM export
    // (DESIGN.md §3.1's known limitation), since openAIP has no way to
    // split one country fetch into two regions.
    ("ES", "LE"),
];

/// States served by the official-AIXM tier. A state here must never also
/// be fetched from openAIP.
const AIXM_STATES: &[&str] = &["FR"];

#[derive(Debug, Error)]
pub enum OpenAipLoadError {
    #[error("openAIP fetch failed for {country}: {source}")]
    Fetch {
        country: String,
        #[source]
        source: OpenAipError,
    },
    #[error("state {0} is served by the official AIXM tier and must not also be fetched from openAIP (DESIGN.md §3.1.2)")]
    TierConflict(String),
    #[error("malformed FF_OPENAIP_STATES entry {0:?}; expected CC:REGION, e.g. DE:ED")]
    MalformedState(String),
}

/// One state's converted data, plus what it cost.
#[derive(Debug, Default)]
pub struct OpenAipLoad {
    pub airports: Vec<Airport>,
    pub navaids: Vec<Navaid>,
    pub airspaces: Vec<AirspaceVolume>,
    /// Per-state counts, for the pipeline log — a country silently
    /// contributing nothing should be visible, not inferred from a total.
    pub per_state: Vec<StateSummary>,
}

#[derive(Debug, Clone)]
pub struct StateSummary {
    pub country: String,
    pub region: String,
    pub airports: usize,
    pub navaids: usize,
    pub airspaces: usize,
    pub skipped_airspaces: usize,
}

/// The configured API key, or `None` when unset (skip the step).
pub fn configured_key() -> Option<String> {
    std::env::var("FF_OPENAIP_API_KEY")
        .ok()
        .filter(|s| !s.trim().is_empty())
}

/// Parses `FF_OPENAIP_STATES`, falling back to [`DEFAULT_STATES`].
pub fn configured_states() -> Result<Vec<(String, String)>, OpenAipLoadError> {
    let Ok(raw) = std::env::var("FF_OPENAIP_STATES") else {
        return Ok(DEFAULT_STATES
            .iter()
            .map(|(c, r)| (c.to_string(), r.to_string()))
            .collect());
    };
    let mut states = Vec::new();
    for entry in raw.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let (country, region) = entry
            .split_once(':')
            .ok_or_else(|| OpenAipLoadError::MalformedState(entry.to_string()))?;
        let (country, region) = (country.trim(), region.trim());
        if country.is_empty() || region.is_empty() {
            return Err(OpenAipLoadError::MalformedState(entry.to_string()));
        }
        states.push((country.to_uppercase(), region.to_uppercase()));
    }
    Ok(states)
}

/// Fetch and convert every configured state.
///
/// Fetch failures are **per state**: one unreachable country must not
/// sink a whole cycle, so it is logged and skipped, matching the policy
/// the AIXM and d-TPP steps already use. A tier conflict is different —
/// that is a configuration error that would corrupt the bundle, so it
/// fails the run.
pub fn load_configured(api_key: &str) -> Result<OpenAipLoad, OpenAipLoadError> {
    let states = configured_states()?;
    for (country, _) in &states {
        if AIXM_STATES.contains(&country.as_str()) {
            return Err(OpenAipLoadError::TierConflict(country.clone()));
        }
    }

    let client = BlockingOpenAipClient::new(api_key);
    let mut out = OpenAipLoad::default();

    for (country, region) in &states {
        match load_state(&client, country, region) {
            Ok((data, summary)) => {
                out.airports.extend(data.airports);
                out.navaids.extend(data.navaids);
                out.airspaces.extend(data.airspaces);
                out.per_state.push(summary);
            }
            Err(err) => {
                tracing::warn!(%country, error = %err, "openAIP fetch failed — skipping this state");
            }
        }
    }
    Ok(out)
}

fn load_state(
    client: &BlockingOpenAipClient,
    country: &str,
    region: &str,
) -> Result<(ff_openaip::OpenAipData, StateSummary), OpenAipLoadError> {
    let fetch_err = |source| OpenAipLoadError::Fetch {
        country: country.to_string(),
        source,
    };
    let airports = client.airports(country).map_err(fetch_err)?;
    let navaids = client.navaids(country).map_err(fetch_err)?;
    let airspaces = client.airspaces(country).map_err(fetch_err)?;

    let data = convert_country(&airports, &navaids, &airspaces, region);
    let summary = StateSummary {
        country: country.to_string(),
        region: region.to_string(),
        airports: data.airports.len(),
        navaids: data.navaids.len(),
        airspaces: data.airspaces.len(),
        skipped_airspaces: data.skipped.airspaces,
    };
    Ok((data, summary))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Environment variables are process-global while Rust runs tests in
    /// parallel threads, so two tests both setting `FF_OPENAIP_STATES`
    /// race — observed failing roughly one run in six before this guard.
    /// Poisoning is recovered from rather than propagated: one failing
    /// test should not cascade into the other.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn lock_env() -> std::sync::MutexGuard<'static, ()> {
        ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn state_config_parses_and_rejects_malformed_entries() {
        let _guard = lock_env();
        std::env::remove_var("FF_OPENAIP_STATES");
        let defaults = configured_states().expect("defaults");
        assert!(defaults.iter().any(|(c, r)| c == "DE" && r == "ED"));
        // France is served by official AIXM and must not be in the
        // openAIP tier's defaults.
        assert!(
            !defaults.iter().any(|(c, _)| c == "FR"),
            "France must not default to the fallback tier"
        );

        std::env::set_var("FF_OPENAIP_STATES", "de:ed, GB:EG ,CA:CY");
        let parsed = configured_states().expect("parses");
        assert_eq!(
            parsed,
            vec![
                ("DE".to_string(), "ED".to_string()),
                ("GB".to_string(), "EG".to_string()),
                ("CA".to_string(), "CY".to_string()),
            ],
            "codes are upper-cased and whitespace tolerated"
        );

        for bad in ["DE", "DE:", ":ED"] {
            std::env::set_var("FF_OPENAIP_STATES", bad);
            assert!(
                matches!(
                    configured_states(),
                    Err(OpenAipLoadError::MalformedState(_))
                ),
                "{bad:?} should be rejected"
            );
        }
        std::env::remove_var("FF_OPENAIP_STATES");
    }

    #[test]
    fn a_state_cannot_be_served_by_both_tiers() {
        let _guard = lock_env();
        // France has an official AIXM export; fetching it from openAIP as
        // well would put two versions of every French airport in one
        // bundle with no way to tell which won.
        std::env::set_var("FF_OPENAIP_STATES", "FR:LF");
        let result = load_configured("dummy-key");
        assert!(
            matches!(result, Err(OpenAipLoadError::TierConflict(ref c)) if c == "FR"),
            "expected a tier conflict, got {result:?}"
        );
        std::env::remove_var("FF_OPENAIP_STATES");
    }
}
