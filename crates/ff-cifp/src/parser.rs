use crate::record::{classify_line, RawRecord};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CifpError {
    #[error("input is not valid UTF-8/ASCII text: {0}")]
    Encoding(#[from] std::str::Utf8Error),
    #[error("field extraction for this record category is not implemented yet")]
    NotImplemented,
}

/// Split a raw CIFP file into classified, still-unparsed records.
///
/// This is the first pass of the pipeline described in DESIGN.md §7: turn
/// the flat file into a stream of records `ff-etl` can route to per-category
/// field extractors. Extraction of individual ARINC 424 fields into
/// `ff_core` types (airports, procedures, legs, ...) is not implemented in
/// this scaffold — see `crate::extract`.
pub fn classify_file(contents: &str) -> Vec<RawRecord> {
    contents.lines().filter_map(classify_line).collect()
}

pub fn classify_bytes(bytes: &[u8]) -> Result<Vec<RawRecord>, CifpError> {
    let text = std::str::from_utf8(bytes)?;
    Ok(classify_file(text))
}
