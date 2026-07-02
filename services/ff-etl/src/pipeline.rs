use crate::bundle::{build_bundle, BundleSource};
use crate::fetch::{fetch_cifp, fetch_nasr};
use crate::publish::{latest_bundle_path, publish_bundle};
use crate::validate::validate_bundle;
use std::collections::HashSet;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EtlError {
    #[error(transparent)]
    Fetch(#[from] crate::fetch::FetchError),
    #[error(transparent)]
    Bundle(#[from] crate::bundle::BundleError),
    #[error(transparent)]
    Validate(#[from] crate::validate::ValidateError),
    #[error(transparent)]
    Publish(#[from] crate::publish::PublishError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Bay Area demo scope (matches `apps/web`'s checked-in demo bundle) —
/// DESIGN.md §13 says "one FAA region, e.g. a single ARTCC" for Phase 0;
/// this reuses the same 5 airports already validated end to end rather
/// than a full ARTCC boundary, to get the first real pipeline run
/// working. Expanding to a real ARTCC selection is a follow-up (see
/// TODO.md).
const REGION_ICAOS: &[&str] = &["KSFO", "KOAK", "KSJC", "KPAO", "KHWD"];

/// Runs the DESIGN.md §7 data pipeline end to end: fetch the current
/// CIFP/NASR cycle, parse them into an `ff-storage`-schema SQLite
/// bundle, validate it against the previously published cycle, then
/// publish it locally under `FF_ETL_DATA_DIR` (default `data/`) for
/// `ff-api` to serve. Chart imagery isn't fetched by this pipeline yet
/// — see TODO.md.
pub fn run() -> Result<(), EtlError> {
    let data_dir = PathBuf::from(std::env::var("FF_ETL_DATA_DIR").unwrap_or_else(|_| "data".to_string()));
    let workdir = tempfile::tempdir()?;

    tracing::info!("fetching current CIFP cycle from aeronav.faa.gov");
    let cifp = fetch_cifp(workdir.path())?;
    tracing::info!(cycle = %cifp.cycle_date, "fetched CIFP");

    tracing::info!(cycle = %cifp.cycle_date, "fetching matching NASR 28-day subscription from nfdc.faa.gov");
    let nasr_dir = fetch_nasr(workdir.path(), &cifp.cycle_date)?;
    tracing::info!("fetched NASR");

    let icaos: HashSet<String> = REGION_ICAOS.iter().map(|s| s.to_string()).collect();
    let bundle_path = workdir.path().join("cycle.sqlite");
    let stats = build_bundle(
        &BundleSource {
            cifp_path: cifp.cifp_path,
            nasr_dir: Some(nasr_dir),
            chart: None,
            icaos,
        },
        &bundle_path,
    )?;
    tracing::info!(?stats, "built cycle bundle");

    let previous_bundle_path = latest_bundle_path(&data_dir)?;
    validate_bundle(&bundle_path, &stats, previous_bundle_path.as_deref())?;
    tracing::info!("validated cycle bundle");

    let published_path = publish_bundle(&bundle_path, &cifp.cycle_date, &data_dir)?;
    tracing::info!(path = %published_path.display(), "published cycle bundle");

    Ok(())
}
