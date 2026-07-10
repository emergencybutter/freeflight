//! Downloads the current CIFP cycle and matching NASR 28-day subscription
//! directly from FAA hosts. Reachability of `aeronav.faa.gov`/
//! `nfdc.faa.gov` has flipped between sessions in some sandboxed
//! environments, so if these calls start failing with connection errors
//! (as opposed to a 4xx/5xx from FAA itself), check egress policy first,
//! not this code.
use ff_charts::ChartKind;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use thiserror::Error;

const CIFP_DIR_URL: &str = "https://aeronav.faa.gov/Upload_313-d/cifp/";
const NASR_URL_TEMPLATE: &str =
    "https://nfdc.faa.gov/webContent/28DaySub/28DaySubscription_Effective_";
const VFR_CHARTS_PAGE_URL: &str =
    "https://www.faa.gov/air_traffic/flight_info/aeronav/digital_products/vfr/";
const IFR_CHARTS_PAGE_URL: &str =
    "https://www.faa.gov/air_traffic/flight_info/aeronav/digital_products/ifr/";

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

/// Shared by `airspace.rs` too — same FAA-facing ETL job, same client
/// config.
pub(crate) fn http_client() -> reqwest::blocking::Client {
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
    /// Terminal Area Chart names under `tac-files/` (e.g. `"Los_Angeles_TAC"`)
    /// — each zip also carries that city's VFR Flyway chart (see
    /// [`fetch_tac_chart`]). Empty if the cycle's `tac-files/` listing is
    /// unavailable, which just means no TAC/Flyway charts this run.
    pub tac_names: Vec<String>,
    /// Helicopter route chart names under `Heli_files/` (e.g.
    /// `"Los_Angeles_Heli"`). Empty if that listing is unavailable.
    pub heli_names: Vec<String>,
}

/// Scans a directory-listing page for `<subdir>/NAME.zip` entries and
/// returns the names, sorted and deduplicated. Same substring-scanning
/// approach as [`latest_cifp_zip_name`]/[`chart_cycle_dates`] — no HTML
/// parser dependency needed for FAA's simple directory-listing markup.
fn zip_names_from_listing(listing_html: &str, subdir: &str) -> Vec<String> {
    let marker = format!("{subdir}/");
    let mut names = Vec::new();
    let mut rest = listing_html;
    while let Some(pos) = rest.find(&marker) {
        rest = &rest[pos + marker.len()..];
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

/// One extracted, kind-classified chart TIF from a TAC or Heli zip (see
/// [`fetch_tac_chart`]/[`fetch_heli_chart`]) — the analog of
/// [`SectionalTif`] but carrying the [`ChartKind`] since a single TAC zip
/// holds both a TAC and a VFR Flyway chart, distinguished by filename.
pub struct ChartPart {
    /// e.g. `"Los_Angeles"`, or `"Los_Angeles_East"` for a split heli chart.
    pub label: String,
    pub tif_path: PathBuf,
    pub kind: ChartKind,
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
            let sectional_names = zip_names_from_listing(&listing, "sectional-files");
            if !sectional_names.is_empty() {
                // TAC and Helicopter charts live in sibling directories for
                // the same cycle date. Best-effort: a missing/empty listing
                // (some cycles, or an egress hiccup) just yields no charts
                // of that kind rather than failing the whole run, since the
                // sectionals — the primary VFR layer — are already in hand.
                let tac_names = list_chart_dir(&client, date, "tac-files");
                let heli_names = list_chart_dir(&client, date, "Heli_files");
                return Ok(ChartCycle {
                    dates: dates.clone(),
                    sectional_names,
                    tac_names,
                    heli_names,
                });
            }
        }
    }
    Err(FetchError::NoChartCycleFound)
}

/// Lists `<subdir>/NAME.zip` entries for a chart cycle date, returning an
/// empty list (not an error) if the directory is missing or unreadable —
/// see the call site in [`discover_chart_cycle`] for why these secondary
/// chart kinds degrade rather than fail the run.
fn list_chart_dir(client: &reqwest::blocking::Client, date: &str, subdir: &str) -> Vec<String> {
    let url = format!("https://aeronav.faa.gov/visual/{date}/{subdir}/");
    match client.get(&url).send() {
        Ok(resp) if resp.status().is_success() => match resp.text() {
            Ok(listing) => zip_names_from_listing(&listing, subdir),
            Err(_) => Vec::new(),
        },
        _ => Vec::new(),
    }
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

/// One georeferenced `.tif` extracted from a sectional's zip, labeled by
/// its own filename rather than the zip's name — see
/// [`fetch_sectional_chart`] for why a zip can hold more than one.
pub struct SectionalTif {
    /// e.g. `"San_Francisco"`, or `"Western_Aleutian_Islands_East"` for
    /// a part of a split sectional.
    pub label: String,
    pub tif_path: PathBuf,
}

/// Downloads the named sectional chart (e.g. `"San_Francisco"`) for
/// whichever of `cycle`'s candidate dates (newest first) actually has
/// it, and extracts every `.tif` inside — usually exactly one, but
/// confirmed live that some sectionals ship their zip with more than
/// one separately-georeferenced file (e.g. `Western_Aleutian_Islands`
/// splits into an East and a West `.tif`; FAA's own per-file metadata
/// notes Hawaiian Islands similarly bundles Honolulu/Mariana/Samoan
/// insets). Taking only the first `.tif` — what an earlier version of
/// this function did — silently dropped the rest. `cycle` comes from
/// [`discover_chart_cycle`], called once per pipeline run rather than
/// re-scraping the FAA VFR page for every individual chart.
pub fn fetch_sectional_chart(
    workdir: &Path,
    sectional_name: &str,
    cycle: &ChartCycle,
) -> Result<Vec<SectionalTif>, FetchError> {
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
    let tif_indices: Vec<usize> = (0..archive.len())
        .filter(|&i| {
            archive
                .by_index(i)
                .map(|entry| entry.name().ends_with(".tif"))
                .unwrap_or(false)
        })
        .collect();
    if tif_indices.is_empty() {
        return Err(FetchError::NoChartTifFound);
    }

    let mut results = Vec::with_capacity(tif_indices.len());
    for (n, index) in tif_indices.into_iter().enumerate() {
        let mut entry = archive.by_index(index)?;
        let label = Path::new(entry.name())
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(sectional_name)
            .trim_end_matches(" SEC")
            .replace(' ', "_");
        let tif_path = workdir.join(format!("sectional_{n}.tif"));
        let mut buf = Vec::new();
        entry.read_to_end(&mut buf)?;
        std::fs::write(&tif_path, &buf)?;
        results.push(SectionalTif { label, tif_path });
    }

    Ok(results)
}

/// Classifies a chart TIF filename from a TAC/Heli zip by its trailing
/// product suffix and returns the chart kind plus a clean, underscored
/// label. FAA names these `"<City> TAC.tif"`, `"<City> FLY.tif"`, and
/// `"<City> [East|West] HEL.tif"` (confirmed live). `None` for anything
/// that doesn't match a known suffix, so the caller can skip it.
fn classify_chart_tif(filename: &str) -> Option<(ChartKind, String)> {
    let stem = Path::new(filename).file_stem()?.to_str()?;
    for (suffix, kind) in [
        (" TAC", ChartKind::TerminalAreaChart),
        (" FLY", ChartKind::VfrFlyway),
        (" HEL", ChartKind::HelicopterRoute),
    ] {
        if let Some(base) = stem.strip_suffix(suffix) {
            return Some((kind, base.trim().replace(' ', "_")));
        }
    }
    None
}

/// Downloads a TAC or Helicopter chart zip (`subdir` = `"tac-files"` or
/// `"Heli_files"`) for whichever of `cycle`'s candidate dates has it, and
/// extracts every recognized `.tif` inside, kind-classified by filename
/// (see [`classify_chart_tif`]). A TAC zip yields two parts — the TAC and
/// its VFR Flyway — while a heli zip yields one or more `HEL` tifs. Same
/// newest-first date fallback and multi-tif handling as
/// [`fetch_sectional_chart`].
pub fn fetch_terminal_chart_zip(
    workdir: &Path,
    name: &str,
    subdir: &str,
    cycle: &ChartCycle,
) -> Result<Vec<ChartPart>, FetchError> {
    let client = http_client();

    let mut zip_bytes = None;
    for date in &cycle.dates {
        let url = format!("https://aeronav.faa.gov/visual/{date}/{subdir}/{name}.zip");
        let resp = client.get(&url).send()?;
        if resp.status().is_success() {
            tracing::info!(chart_cycle = %date, chart = %name, "downloading terminal/heli chart");
            zip_bytes = Some(resp.bytes()?);
            break;
        }
        tracing::warn!(chart_cycle = %date, status = %resp.status(), "terminal/heli chart not available for this cycle, trying older");
    }
    let zip_bytes = zip_bytes.ok_or(FetchError::NoChartCycleFound)?;

    let zip_path = workdir.join(format!("{name}.zip"));
    std::fs::write(&zip_path, &zip_bytes)?;

    let zip_file = std::fs::File::open(&zip_path)?;
    let mut archive = zip::ZipArchive::new(zip_file)?;
    let tif_indices: Vec<usize> = (0..archive.len())
        .filter(|&i| {
            archive
                .by_index(i)
                .map(|entry| entry.name().ends_with(".tif"))
                .unwrap_or(false)
        })
        .collect();
    if tif_indices.is_empty() {
        return Err(FetchError::NoChartTifFound);
    }

    let mut results = Vec::with_capacity(tif_indices.len());
    for (n, index) in tif_indices.into_iter().enumerate() {
        let mut entry = archive.by_index(index)?;
        let name_in_zip = entry.name().to_string();
        let Some((kind, label)) = classify_chart_tif(&name_in_zip) else {
            tracing::warn!(file = %name_in_zip, "unrecognized chart tif name (not TAC/FLY/HEL), skipping");
            continue;
        };
        let tif_path = workdir.join(format!("terminal_{n}.tif"));
        let mut buf = Vec::new();
        entry.read_to_end(&mut buf)?;
        std::fs::write(&tif_path, &buf)?;
        results.push(ChartPart {
            label,
            tif_path,
            kind,
        });
    }
    Ok(results)
}

/// Discovered once per pipeline run, like [`ChartCycle`] — but unlike VFR
/// sectionals (whose names live on a *separate* directory-listing page,
/// requiring a second live request per candidate date), the FAA IFR
/// digital-products page embeds direct panel-zip links inline, so both
/// the candidate dates and the exact panel names come from one fetch.
pub struct IfrEnrouteCycle {
    pub dates: Vec<String>,
    /// `"enr_l01"`..`"enr_l36"` as of writing — CONUS Low Altitude Enroute
    /// panels. Caribbean/oceanic route charts aren't included; this
    /// covers the CONUS low/high panels only.
    pub low_panel_names: Vec<String>,
    pub high_panel_names: Vec<String>,
}

/// Scans the FAA IFR charts page for `enroute/MM-DD-YYYY/` chart-cycle
/// directory URLs — same shape/reasoning as [`chart_cycle_dates`], just a
/// different literal prefix (`enroute/` vs `visual/`).
fn enroute_cycle_dates(page_html: &str) -> Vec<String> {
    let mut dates = Vec::new();
    let mut rest = page_html;
    while let Some(pos) = rest.find("enroute/") {
        rest = &rest[pos + "enroute/".len()..];
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
    dates.sort_by_key(|d| std::cmp::Reverse(format!("{}-{}", &d[6..10], &d[0..5])));
    dates
}

/// Scans the IFR charts page for `{prefix}NN.zip` panel names (e.g.
/// `prefix="enr_l"` → `"enr_l01"`..`"enr_l36"`) — CONUS Low/High panels
/// use these prefixes exclusively for their GEO-TIFF product (the PDF
/// product lives under an unrelated `delusN`/`dehusN` naming scheme, so
/// there's no GEO-TIFF/PDF disambiguation needed here, just the prefix).
/// Each name appears once per cycle-date column on the real page, so
/// this dedupes like [`sectional_names_from_listing`].
fn ifr_panel_names_from_page(page_html: &str, prefix: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = page_html;
    while let Some(pos) = rest.find(prefix) {
        let candidate_start = &rest[pos..];
        if let Some(end) = candidate_start.find(".zip") {
            let candidate = &candidate_start[..end];
            let digits = &candidate[prefix.len()..];
            if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
                names.push(candidate.to_string());
            }
        }
        rest = &rest[pos + prefix.len()..];
    }
    names.sort();
    names.dedup();
    names
}

/// Discovers the current IFR enroute chart cycle's candidate dates and
/// every CONUS Low/High Altitude panel name published under it — see
/// [`IfrEnrouteCycle`].
pub fn discover_ifr_enroute_cycle() -> Result<IfrEnrouteCycle, FetchError> {
    let client = http_client();
    let page = client
        .get(IFR_CHARTS_PAGE_URL)
        .send()?
        .error_for_status()?
        .text()?;
    let dates = enroute_cycle_dates(&page);
    if dates.is_empty() {
        return Err(FetchError::NoChartCycleFound);
    }
    let low_panel_names = ifr_panel_names_from_page(&page, "enr_l");
    let high_panel_names = ifr_panel_names_from_page(&page, "enr_h");
    if low_panel_names.is_empty() && high_panel_names.is_empty() {
        return Err(FetchError::NoChartCycleFound);
    }
    Ok(IfrEnrouteCycle {
        dates,
        low_panel_names,
        high_panel_names,
    })
}

/// Downloads the named IFR enroute panel (e.g. `"enr_l01"`) for whichever
/// of `cycle`'s candidate dates (newest first) actually has it, and
/// extracts every `.tif` inside — mirrors [`fetch_sectional_chart`]'s
/// multi-tif robustness even though every panel sampled while building
/// this (`enr_l01`, `enr_h01`) had exactly one; sectionals' own multi-tif
/// surprise (Western Aleutian Islands East/West) was exactly this kind
/// of untested assumption, so the same safety margin applies here.
pub fn fetch_ifr_enroute_panel(
    workdir: &Path,
    panel_name: &str,
    cycle: &IfrEnrouteCycle,
) -> Result<Vec<SectionalTif>, FetchError> {
    let client = http_client();

    let mut zip_bytes = None;
    for date in &cycle.dates {
        let url = format!("https://aeronav.faa.gov/enroute/{date}/{panel_name}.zip");
        let resp = client.get(&url).send()?;
        if resp.status().is_success() {
            tracing::info!(chart_cycle = %date, panel = %panel_name, "downloading IFR enroute chart panel");
            zip_bytes = Some(resp.bytes()?);
            break;
        }
        tracing::warn!(chart_cycle = %date, status = %resp.status(), "IFR enroute panel not available for this cycle, trying older");
    }
    let zip_bytes = zip_bytes.ok_or(FetchError::NoChartCycleFound)?;

    let zip_path = workdir.join(format!("{panel_name}.zip"));
    std::fs::write(&zip_path, &zip_bytes)?;

    let zip_file = std::fs::File::open(&zip_path)?;
    let mut archive = zip::ZipArchive::new(zip_file)?;
    let tif_indices: Vec<usize> = (0..archive.len())
        .filter(|&i| {
            archive
                .by_index(i)
                .map(|entry| entry.name().ends_with(".tif"))
                .unwrap_or(false)
        })
        .collect();
    if tif_indices.is_empty() {
        return Err(FetchError::NoChartTifFound);
    }

    let mut results = Vec::with_capacity(tif_indices.len());
    for (n, index) in tif_indices.into_iter().enumerate() {
        let mut entry = archive.by_index(index)?;
        let label = Path::new(entry.name())
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(panel_name)
            .to_string();
        let tif_path = workdir.join(format!("{panel_name}_{n}.tif"));
        let mut buf = Vec::new();
        entry.read_to_end(&mut buf)?;
        std::fs::write(&tif_path, &buf)?;
        results.push(SectionalTif { label, tif_path });
    }

    Ok(results)
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
            zip_names_from_listing(html, "sectional-files"),
            vec![
                "Albuquerque".to_string(),
                "Dallas-Ft_Worth".to_string(),
                "San_Francisco".to_string()
            ]
        );
    }

    #[test]
    fn finds_tac_names_from_a_real_directory_listing() {
        // Shape confirmed against a live aeronav.faa.gov tac-files listing;
        // Heli_files uses the same markup with its own subdir marker.
        let html = r#"
            <A HREF="/visual/07-09-2026/tac-files/Los_Angeles_TAC.zip">Los_Angeles_TAC.zip</A> 10-Jun-2026 01:53PM 37425023
            <A HREF="/visual/07-09-2026/tac-files/New_York_TAC.zip">New_York_TAC.zip</A> 10-Jun-2026 01:53PM 41234567
        "#;
        assert_eq!(
            zip_names_from_listing(html, "tac-files"),
            vec!["Los_Angeles_TAC".to_string(), "New_York_TAC".to_string()]
        );
    }

    #[test]
    fn classifies_chart_tifs_by_product_suffix() {
        assert_eq!(
            classify_chart_tif("Los Angeles TAC.tif"),
            Some((ChartKind::TerminalAreaChart, "Los_Angeles".to_string()))
        );
        assert_eq!(
            classify_chart_tif("Los Angeles FLY.tif"),
            Some((ChartKind::VfrFlyway, "Los_Angeles".to_string()))
        );
        assert_eq!(
            classify_chart_tif("Los Angeles East HEL.tif"),
            Some((ChartKind::HelicopterRoute, "Los_Angeles_East".to_string()))
        );
        assert_eq!(classify_chart_tif("Something Else.tif"), None);
    }

    #[test]
    fn ignores_listing_pages_with_no_sectional_entries() {
        assert!(zip_names_from_listing("<html><body>empty</body></html>", "sectional-files").is_empty());
    }

    #[test]
    fn finds_enroute_cycle_dates_newest_first() {
        // Excerpt shape confirmed against the real FAA IFR digital-products
        // page: each panel row lists both the current and next cycle dates
        // in separate columns.
        let html = r#"
            <a href="https://aeronav.faa.gov/enroute/05-14-2026/enr_l01.zip">GEO-TIFF</a>
            <a href="https://aeronav.faa.gov/enroute/07-09-2026/enr_l01.zip">GEO-TIFF</a>
        "#;
        assert_eq!(
            enroute_cycle_dates(html),
            vec!["07-09-2026".to_string(), "05-14-2026".to_string()]
        );
    }

    #[test]
    fn finds_ifr_panel_names_from_a_real_page_excerpt() {
        // Excerpt confirmed against a live download of
        // https://www.faa.gov/air_traffic/flight_info/aeronav/digital_products/ifr/ —
        // each panel appears twice (once per cycle-date column), so this
        // also exercises the dedup.
        let html = r#"
            <tr><td>ELUS1</td><td>May 14 2026<br><cfoutput><a href="https://aeronav.faa.gov/enroute/05-14-2026/enr_l01.zip">GEO-TIFF</a> <small>(Zip)</small></cfoutput><br><cfoutput><a href="https://aeronav.faa.gov/enroute/05-14-2026/delus1.zip">PDF</a> <small>(Zip)</small></cfoutput></td><td>Jul 09 2026<br><cfoutput><a href="https://aeronav.faa.gov/enroute/07-09-2026/enr_l01.zip">GEO-TIFF</a> <small>(Zip)</small></cfoutput><br><cfoutput><a href="https://aeronav.faa.gov/enroute/07-09-2026/delus1.zip">PDF</a> <small>(Zip)</small></cfoutput></td></tr>
            <tr><td>ELUS2</td><td>May 14 2026<br><cfoutput><a href="https://aeronav.faa.gov/enroute/05-14-2026/enr_l02.zip">GEO-TIFF</a> <small>(Zip)</small></cfoutput></td><td>Jul 09 2026<br><cfoutput><a href="https://aeronav.faa.gov/enroute/07-09-2026/enr_l02.zip">GEO-TIFF</a> <small>(Zip)</small></cfoutput></td></tr>
            <tr><td>EHUS1</td><td>May 14 2026<br><cfoutput><a href="https://aeronav.faa.gov/enroute/05-14-2026/enr_h01.zip">GEO-TIFF</a> <small>(Zip)</small></cfoutput></td><td>Jul 09 2026<br><cfoutput><a href="https://aeronav.faa.gov/enroute/07-09-2026/enr_h01.zip">GEO-TIFF</a> <small>(Zip)</small></cfoutput></td></tr>
        "#;
        assert_eq!(
            ifr_panel_names_from_page(html, "enr_l"),
            vec!["enr_l01".to_string(), "enr_l02".to_string()]
        );
        assert_eq!(
            ifr_panel_names_from_page(html, "enr_h"),
            vec!["enr_h01".to_string()]
        );
    }

    #[test]
    fn ignores_pdf_links_when_scanning_for_geotiff_panel_names() {
        // "delus1.zip" (PDF) must not be picked up when scanning for the
        // "enr_l" GEO-TIFF prefix, and vice versa.
        let html = r#"<a href="https://aeronav.faa.gov/enroute/07-09-2026/delus1.zip">PDF</a>"#;
        assert!(ifr_panel_names_from_page(html, "enr_l").is_empty());
    }
}
