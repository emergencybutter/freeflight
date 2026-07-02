//! Downloads the current CIFP cycle and matching NASR 28-day subscription
//! directly from FAA hosts. Confirmed reachable from this environment
//! (`aeronav.faa.gov`, `nfdc.faa.gov`) as of this writing — see TODO.md's
//! "Environment/network quirks" note: this has flipped between sessions,
//! so if these calls start failing with connection errors (as opposed to
//! a 4xx/5xx from FAA itself), that's the first thing to check, not a
//! bug here.
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use thiserror::Error;

const CIFP_DIR_URL: &str = "https://aeronav.faa.gov/Upload_313-d/cifp/";
const NASR_URL_TEMPLATE: &str = "https://nfdc.faa.gov/webContent/28DaySub/28DaySubscription_Effective_";

#[derive(Debug, Error)]
pub enum FetchError {
    #[error("http request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("zip error: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("no CIFP_YYMMDD.zip filename found in the FAA CIFP directory listing")]
    NoCifpCycleFound,
    #[error("no CSV_Data/*.zip entry found inside the NASR subscription package")]
    NoNasrCsvFound,
}

pub struct FetchedCifp {
    /// The cycle's effective date, e.g. "2026-07-09" — derived from the
    /// CIFP zip's filename (`CIFP_YYMMDD.zip`) and reused as-is to name
    /// the matching NASR package, since both are published on the same
    /// AIRAC schedule (confirmed: CIFP_260709.zip pairs with NASR's
    /// `..._Effective_2026-07-09.zip`).
    pub cycle_date: String,
    pub cifp_path: PathBuf,
}

fn http_client() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .user_agent("freeflight-etl/0.1 (+https://github.com/emergencybutter/freeflight)")
        .build()
        .expect("failed to build reqwest client")
}

/// Scans a directory-listing HTML page for `CIFP_YYMMDD.zip` filenames
/// and returns the most recent one (YYMMDD sorts correctly as a plain
/// string for any date within the current century).
fn latest_cifp_zip_name(listing_html: &str) -> Option<String> {
    let mut candidates = Vec::new();
    let mut rest = listing_html;
    while let Some(pos) = rest.find("CIFP_") {
        rest = &rest[pos + "CIFP_".len()..];
        if rest.len() >= 10 && rest.as_bytes()[..6].iter().all(u8::is_ascii_digit) && &rest[6..10] == ".zip" {
            candidates.push(format!("CIFP_{}.zip", &rest[..6]));
        }
    }
    candidates.sort();
    candidates.dedup();
    candidates.pop()
}

fn cycle_date_from_yymmdd(yymmdd: &str) -> String {
    let (yy, rest) = yymmdd.split_at(2);
    let (mm, dd) = rest.split_at(2);
    format!("20{yy}-{mm}-{dd}")
}

/// Downloads the current CIFP cycle zip, extracts `FAACIFP18` into
/// `workdir`, and returns its path plus the cycle's effective date.
pub fn fetch_cifp(workdir: &Path) -> Result<FetchedCifp, FetchError> {
    let client = http_client();

    let listing = client.get(CIFP_DIR_URL).send()?.error_for_status()?.text()?;
    let filename = latest_cifp_zip_name(&listing).ok_or(FetchError::NoCifpCycleFound)?;
    let yymmdd = &filename["CIFP_".len()..filename.len() - ".zip".len()];
    let cycle_date = cycle_date_from_yymmdd(yymmdd);

    let zip_bytes = client
        .get(format!("{CIFP_DIR_URL}{filename}"))
        .send()?
        .error_for_status()?
        .bytes()?;
    let zip_path = workdir.join(&filename);
    std::fs::write(&zip_path, &zip_bytes)?;

    let cifp_path = workdir.join("FAACIFP18");
    {
        let zip_file = std::fs::File::open(&zip_path)?;
        let mut archive = zip::ZipArchive::new(zip_file)?;
        let mut entry = archive.by_name("FAACIFP18")?;
        let mut out = std::fs::File::create(&cifp_path)?;
        let mut buf = Vec::new();
        entry.read_to_end(&mut buf)?;
        out.write_all(&buf)?;
    }

    Ok(FetchedCifp { cycle_date, cifp_path })
}

/// Downloads the NASR 28-day subscription package matching `cycle_date`,
/// extracts the four CSVs `ff-nasr` needs into a fresh `nasr/` directory
/// under `workdir`, and returns that directory's path.
///
/// The package structure is a zip containing a nested
/// `CSV_Data/<date>_CSV.zip` (exact inner filename varies by cycle, so
/// this searches for it by directory prefix rather than hardcoding the
/// date-derived name), which in turn contains the flat CSVs.
pub fn fetch_nasr(workdir: &Path, cycle_date: &str) -> Result<PathBuf, FetchError> {
    let client = http_client();

    let outer_bytes = client
        .get(format!("{NASR_URL_TEMPLATE}{cycle_date}.zip"))
        .send()?
        .error_for_status()?
        .bytes()?;
    let outer_zip_path = workdir.join("nasr.zip");
    std::fs::write(&outer_zip_path, &outer_bytes)?;

    let outer_file = std::fs::File::open(&outer_zip_path)?;
    let mut outer_archive = zip::ZipArchive::new(outer_file)?;

    let csv_zip_index = (0..outer_archive.len())
        .find(|&i| {
            outer_archive
                .by_index(i)
                .map(|entry| entry.name().starts_with("CSV_Data/") && entry.name().ends_with(".zip"))
                .unwrap_or(false)
        })
        .ok_or(FetchError::NoNasrCsvFound)?;

    let inner_zip_path = workdir.join("nasr_csv.zip");
    {
        let mut entry = outer_archive.by_index(csv_zip_index)?;
        let mut buf = Vec::new();
        entry.read_to_end(&mut buf)?;
        std::fs::write(&inner_zip_path, &buf)?;
    }

    let nasr_dir = workdir.join("nasr");
    std::fs::create_dir_all(&nasr_dir)?;
    let inner_file = std::fs::File::open(&inner_zip_path)?;
    let mut inner_archive = zip::ZipArchive::new(inner_file)?;
    for name in ["APT_BASE.csv", "APT_RWY.csv", "APT_RWY_END.csv", "FRQ.csv"] {
        let mut entry = inner_archive.by_name(name)?;
        let mut buf = Vec::new();
        entry.read_to_end(&mut buf)?;
        std::fs::write(nasr_dir.join(name), &buf)?;
    }

    Ok(nasr_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_the_latest_cifp_filename_from_a_directory_listing() {
        let html = r#"
            <A HREF="/Upload_313-d/cifp/CIFP_260514.zip">CIFP_260514.zip</A>
            <A HREF="/Upload_313-d/cifp/CIFP_260611.zip">CIFP_260611.zip</A>
            <A HREF="/Upload_313-d/cifp/CIFP_260709.zip">CIFP_260709.zip</A>
        "#;
        assert_eq!(latest_cifp_zip_name(html), Some("CIFP_260709.zip".to_string()));
    }

    #[test]
    fn returns_none_when_no_cifp_filename_present() {
        assert_eq!(latest_cifp_zip_name("<html><body>empty</body></html>"), None);
    }

    #[test]
    fn derives_cycle_date_from_yymmdd() {
        assert_eq!(cycle_date_from_yymmdd("260709"), "2026-07-09");
    }
}
