//! Loads a locally-provided French SIA **AIXM 4.5** export and parses it
//! to `ff-core` types via `ff-aixm` (DESIGN.md §3.1). The non-US analog of
//! `fetch_cifp`/`fetch_nasr`, but *not* an HTTP fetch: the SIA export is
//! distributed only through a cart/checkout flow (no stable direct URL —
//! confirmed), so the operator downloads `export_xml_bd_SIA*.zip` and
//! points the ETL at it. This is the same pattern DESIGN.md §7 already
//! uses for anything that can't be pulled unattended.
//!
//! # Licensing
//!
//! SIA data is under the **Licence Ouverte** — redistribution and
//! commercial use permitted, but **attribution is required** ("Service de
//! l'Information Aéronautique (SIA)" + the export's effective date). Any
//! bundle built with this data must surface that attribution in the
//! clients (see the note at the pipeline call site).
//!
//! # Configuration
//!
//! - `FF_AIXM_FR_PATH` — path to the SIA export: either the
//!   `export_xml_bd_SIA*.zip` or the `AIXM4.5_all_FR_OM_*.xml` inside it.
//!   Unset → the AIXM step is skipped and a US-only cycle is unaffected.
use ff_aixm::{parse_snapshot, AixmData};
use std::io::Read;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// ICAO region stamped on navaids/waypoints. TODO(region): the SIA
/// `FR_OM` export spans several ICAO regions (metropolitan `LF`, New
/// Caledonia, Antilles, …); a single stamp is a known simplification —
/// per-feature region is a follow-up (see `ff-aixm` crate docs).
const REGION: &str = "LF";

#[derive(Debug, Error)]
pub enum AixmLoadError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("zip error: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("AIXM parse error: {0}")]
    Parse(#[from] ff_aixm::AixmError),
    #[error("no AIXM4.5 .xml entry found inside {0}")]
    NoAixmEntry(String),
}

/// The configured local SIA export path, or `None` when `FF_AIXM_FR_PATH`
/// is unset (the signal to skip the AIXM step).
pub fn configured_source() -> Option<PathBuf> {
    std::env::var("FF_AIXM_FR_PATH")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
}

/// Reads and parses the SIA export at `path` into `ff-core` types.
/// Airports with implausible coordinates are dropped here so one bad row
/// can't fail the whole cycle at validation time (same guard the FAA path
/// relies on `validate` for).
pub fn load(path: &Path) -> Result<AixmData, AixmLoadError> {
    let bytes = read_aixm_bytes(path)?;
    let mut data = parse_snapshot(&bytes, REGION)?;
    data.airports.retain(|a| plausible(a.lat, a.lon));
    Ok(data)
}

/// Returns the raw AIXM XML bytes: read directly if `path` is an `.xml`,
/// or extracted from the `AIXM4.5_*.xml` entry if `path` is the SIA zip.
fn read_aixm_bytes(path: &Path) -> Result<Vec<u8>, AixmLoadError> {
    let is_zip = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("zip"));
    if !is_zip {
        return Ok(std::fs::read(path)?);
    }

    let file = std::fs::File::open(path)?;
    let mut archive = zip::ZipArchive::new(file)?;
    // The zip carries both the AIXM 4.5 file and the SIA-proprietary
    // XML_SIA file; pick the AIXM one by name.
    let idx = (0..archive.len())
        .find(|&i| {
            archive
                .by_index(i)
                .map(|e| {
                    let n = e.name();
                    n.contains("AIXM") && n.to_ascii_lowercase().ends_with(".xml")
                })
                .unwrap_or(false)
        })
        .ok_or_else(|| AixmLoadError::NoAixmEntry(path.display().to_string()))?;

    let mut entry = archive.by_index(idx)?;
    let mut buf = Vec::with_capacity(entry.size() as usize);
    entry.read_to_end(&mut buf)?;
    Ok(buf)
}

fn plausible(lat: f64, lon: f64) -> bool {
    (-90.0..=90.0).contains(&lat) && (-180.0..=180.0).contains(&lon) && !(lat == 0.0 && lon == 0.0)
}
