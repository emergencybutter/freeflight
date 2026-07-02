use thiserror::Error;

#[derive(Debug, Error)]
pub enum EtlError {
    #[error("step '{0}' is not implemented yet")]
    NotImplemented(&'static str),
}

/// Runs the DESIGN.md §7 data pipeline end to end: fetch each FAA/NOAA
/// source for the current cycle, parse it with `ff-cifp`/`ff-nasr`/
/// `ff-charts`, validate against the previous cycle, then emit a single
/// versioned `cycle-*.sqlite` + `charts-*.pmtiles` bundle for `ff-api` to
/// serve.
///
/// Every step below is a placeholder: this scaffold establishes the
/// pipeline's shape and stops at the first unimplemented step rather than
/// pretending to succeed, so `cargo run -p ff-etl` honestly reports how
/// far the pipeline actually gets.
pub fn run() -> Result<(), EtlError> {
    fetch_cifp()?;
    fetch_nasr()?;
    fetch_charts()?;
    build_cycle_bundle()?;
    validate_bundle()?;
    publish_bundle()?;
    Ok(())
}

fn fetch_cifp() -> Result<(), EtlError> {
    tracing::info!("fetch_cifp: download the current-cycle CIFP file from the FAA CIFP site");
    Err(EtlError::NotImplemented("fetch_cifp"))
}

fn fetch_nasr() -> Result<(), EtlError> {
    tracing::info!("fetch_nasr: download the current 28-day NASR subscription CSV set");
    Err(EtlError::NotImplemented("fetch_nasr"))
}

fn fetch_charts() -> Result<(), EtlError> {
    tracing::info!("fetch_charts: download current-cycle VFR/IFR GeoTIFF chart releases");
    Err(EtlError::NotImplemented("fetch_charts"))
}

fn build_cycle_bundle() -> Result<(), EtlError> {
    tracing::info!(
        "build_cycle_bundle: parse fetched sources via ff-cifp/ff-nasr, write into an \
         ff-storage-schema SQLite file, and tile fetched charts via ff-charts::ingest into PMTiles"
    );
    Err(EtlError::NotImplemented("build_cycle_bundle"))
}

fn validate_bundle() -> Result<(), EtlError> {
    tracing::info!(
        "validate_bundle: sanity-check row counts and geometry against the previous cycle \
         before publishing"
    );
    Err(EtlError::NotImplemented("validate_bundle"))
}

fn publish_bundle() -> Result<(), EtlError> {
    tracing::info!(
        "publish_bundle: upload the bundle to object storage and flip the 'latest' pointer"
    );
    Err(EtlError::NotImplemented("publish_bundle"))
}
