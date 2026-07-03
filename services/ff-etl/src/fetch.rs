//! Downloads the current CIFP cycle and matching NASR 28-day subscription
//! directly from FAA hosts. Reachability of `aeronav.faa.gov`/
//! `nfdc.faa.gov` has flipped between sessions in some sandboxed
//! environments, so if these calls start failing with connection errors
//! (as opposed to a 4xx/5xx from FAA itself), check egress policy first,
//! not this code.
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use thiserror::Error;

const CIFP_DIR_URL: &str = "https://aeronav.faa.gov/Upload_313-d/cifp/";
const NASR_URL_TEMPLATE: &str =
    "https://nfdc.faa.gov/webContent/28DaySub/28DaySubscription_Effective_";
const VFR_CHARTS_PAGE_URL: &str =
    "https://www.faa.gov/air_traffic/flight_info/aeronav/digital_products/vfr/";

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
    #[error("no visual-chart cycle dates found on the FAA VFR charts page")]
    NoChartCycleFound,
    #[error("no .tif entry found inside the sectional chart package")]
    NoChartTifFound,
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
        if rest.len() >= 10
            && rest.as_bytes()[..6].iter().all(u8::is_ascii_digit)
            && &rest[6..10] == ".zip"
        {
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

    let listing = client
        .get(CIFP_DIR_URL)
        .send()?
        .error_for_status()?
        .text()?;
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

    Ok(FetchedCifp {
        cycle_date,
        cifp_path,
    })
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
                .map(|entry| {
                    entry.name().starts_with("CSV_Data/") && entry.name().ends_with(".zip")
                })
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

/// The current 56-day chart cycle's candidate dates (newest first, see
/// [`chart_cycle_dates`]) plus every sectional name published under the
/// cycle actually used to discover them — i.e. every FAA sectional chart
/// that currently exists (CONUS + Alaska + Hawaii + a few Canadian
/// border charts FAA also publishes), not a hardcoded regional subset.
pub struct ChartCycle {
    pub dates: Vec<String>,
    pub sectional_names: Vec<String>,
}

/// Scans a `sectional-files/` directory listing page for `NAME.zip`
/// entries and returns the names (e.g. `"San_Francisco"`), sorted and
/// deduplicated. Same substring-scanning approach as
/// [`latest_cifp_zip_name`]/[`chart_cycle_dates`] — no HTML parser
/// dependency needed for FAA's simple directory-listing markup.
fn sectional_names_from_listing(listing_html: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = listing_html;
    while let Some(pos) = rest.find("sectional-files/") {
        rest = &rest[pos + "sectional-files/".len()..];
        if let Some(end) = rest.find(".zip") {
            let candidate = &rest[..end];
            if !candidate.is_empty() && !candidate.contains('/') && !candidate.contains('"') {
                names.push(candidate.to_string());
            }
        }
    }
    names.sort();
    names.dedup();
    names
}

/// Discovers the current chart cycle's candidate dates and the full list
/// of sectional charts FAA publishes under it. Tries each candidate date
/// (newest first) until one has a non-empty `sectional-files/` listing —
/// same newest-with-fallback reasoning as [`fetch_sectional_chart`], but
/// done once up front here rather than by every individual chart fetch.
pub fn discover_chart_cycle() -> Result<ChartCycle, FetchError> {
    let client = http_client();
    let page = client
        .get(VFR_CHARTS_PAGE_URL)
        .send()?
        .error_for_status()?
        .text()?;
    let dates = chart_cycle_dates(&page);
    if dates.is_empty() {
        return Err(FetchError::NoChartCycleFound);
    }

    for date in &dates {
        let url = format!("https://aeronav.faa.gov/visual/{date}/sectional-files/");
        let resp = client.get(&url).send()?;
        if resp.status().is_success() {
            let listing = resp.text()?;
            let sectional_names = sectional_names_from_listing(&listing);
            if !sectional_names.is_empty() {
                return Ok(ChartCycle {
                    dates: dates.clone(),
                    sectional_names,
                });
            }
        }
    }
    Err(FetchError::NoChartCycleFound)
}

/// Scans the FAA VFR charts page for `visual/MM-DD-YYYY/` chart-cycle
/// directory URLs and returns the dates found, most recent first. The
/// page lists the current and next 56-day chart cycles; both directories
/// exist on `aeronav.faa.gov` (confirmed live), so trying newest-first
/// with fallback covers the window where the "next" cycle is listed but
/// a given chart file hasn't been uploaded yet.
fn chart_cycle_dates(page_html: &str) -> Vec<String> {
    let mut dates = Vec::new();
    let mut rest = page_html;
    while let Some(pos) = rest.find("visual/") {
        rest = &rest[pos + "visual/".len()..];
        if rest.len() >= 10 {
            let candidate = &rest[..10];
            let bytes = candidate.as_bytes();
            let shaped = bytes[2] == b'-'
                && bytes[5] == b'-'
                && bytes
                    .iter()
                    .enumerate()
                    .all(|(i, b)| i == 2 || i == 5 || b.is_ascii_digit());
            if shaped && !dates.contains(&candidate.to_string()) {
                dates.push(candidate.to_string());
            }
        }
    }
    // MM-DD-YYYY doesn't sort chronologically as a string; sort by the
    // rearranged YYYY-MM-DD form, newest first.
    dates.sort_by_key(|d| std::cmp::Reverse(format!("{}-{}", &d[6..10], &d[0..5])));
    dates
}

/// Downloads the named sectional chart (e.g. `"San_Francisco"`) for
/// whichever of `cycle`'s candidate dates (newest first) actually has it,
/// extracts the GeoTIFF into `workdir`, and returns its path. `cycle`
/// comes from [`discover_chart_cycle`], called once per pipeline run
/// rather than re-scraping the FAA VFR page for every individual chart.
pub fn fetch_sectional_chart(
    workdir: &Path,
    sectional_name: &str,
    cycle: &ChartCycle,
) -> Result<PathBuf, FetchError> {
    let client = http_client();

    let mut zip_bytes = None;
    for date in &cycle.dates {
        let url =
            format!("https://aeronav.faa.gov/visual/{date}/sectional-files/{sectional_name}.zip");
        let resp = client.get(&url).send()?;
        if resp.status().is_success() {
            tracing::info!(chart_cycle = %date, "downloading sectional chart");
            zip_bytes = Some(resp.bytes()?);
            break;
        }
        tracing::warn!(chart_cycle = %date, status = %resp.status(), "sectional not available for this cycle, trying older");
    }
    let zip_bytes = zip_bytes.ok_or(FetchError::NoChartCycleFound)?;

    let zip_path = workdir.join("sectional.zip");
    std::fs::write(&zip_path, &zip_bytes)?;

    let zip_file = std::fs::File::open(&zip_path)?;
    let mut archive = zip::ZipArchive::new(zip_file)?;
    let tif_index = (0..archive.len())
        .find(|&i| {
            archive
                .by_index(i)
                .map(|entry| entry.name().ends_with(".tif"))
                .unwrap_or(false)
        })
        .ok_or(FetchError::NoChartTifFound)?;

    let tif_path = workdir.join("sectional.tif");
    {
        let mut entry = archive.by_index(tif_index)?;
        let mut buf = Vec::new();
        entry.read_to_end(&mut buf)?;
        std::fs::write(&tif_path, &buf)?;
    }

    Ok(tif_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_chart_cycle_dates_newest_first() {
        let html = r#"
            <a href="https://aeronav.faa.gov/visual/05-14-2026/All_Files/Sectional.zip">x</a>
            <a href="https://aeronav.faa.gov/visual/07-09-2026/All_Files/Sectional.zip">y</a>
            <a href="https://aeronav.faa.gov/visual/05-14-2026/Caribbean/Caribbean_1_VFR.zip">z</a>
        "#;
        assert_eq!(
            chart_cycle_dates(html),
            vec!["07-09-2026".to_string(), "05-14-2026".to_string()]
        );
    }

    #[test]
    fn ignores_malformed_visual_paths() {
        assert!(chart_cycle_dates("visual/notadate/x.zip").is_empty());
    }

    #[test]
    fn picks_the_latest_cifp_filename_from_a_directory_listing() {
        let html = r#"
            <A HREF="/Upload_313-d/cifp/CIFP_260514.zip">CIFP_260514.zip</A>
            <A HREF="/Upload_313-d/cifp/CIFP_260611.zip">CIFP_260611.zip</A>
            <A HREF="/Upload_313-d/cifp/CIFP_260709.zip">CIFP_260709.zip</A>
        "#;
        assert_eq!(
            latest_cifp_zip_name(html),
            Some("CIFP_260709.zip".to_string())
        );
    }

    #[test]
    fn returns_none_when_no_cifp_filename_present() {
        assert_eq!(
            latest_cifp_zip_name("<html><body>empty</body></html>"),
            None
        );
    }

    #[test]
    fn derives_cycle_date_from_yymmdd() {
        assert_eq!(cycle_date_from_yymmdd("260709"), "2026-07-09");
    }

    #[test]
    fn finds_sectional_names_from_a_real_directory_listing() {
        // Excerpt shape confirmed against a live
        // aeronav.faa.gov/visual/<date>/sectional-files/ listing.
        let html = r#"
            <A HREF="/visual/07-09-2026/sectional-files/Albuquerque.zip">Albuquerque.zip</A> 12-Jun-2026 09:14AM 61234567
            <A HREF="/visual/07-09-2026/sectional-files/Dallas-Ft_Worth.zip">Dallas-Ft_Worth.zip</A> 12-Jun-2026 09:14AM 61234567
            <A HREF="/visual/07-09-2026/sectional-files/San_Francisco.zip">San_Francisco.zip</A> 12-Jun-2026 09:14AM 77919083
        "#;
        assert_eq!(
            sectional_names_from_listing(html),
            vec![
                "Albuquerque".to_string(),
                "Dallas-Ft_Worth".to_string(),
                "San_Francisco".to_string()
            ]
        );
    }

    #[test]
    fn ignores_listing_pages_with_no_sectional_entries() {
        assert!(sectional_names_from_listing("<html><body>empty</body></html>").is_empty());
    }
}
