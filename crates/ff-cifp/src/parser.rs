use crate::record::{classify_line, RawRecord};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CifpError {
    #[error("input is not valid UTF-8/ASCII text: {0}")]
    Encoding(#[from] std::str::Utf8Error),
    #[error("record is shorter than the standard 132-column ARINC 424 record length")]
    LineTooShort,
    #[error("record is not the category this extractor handles")]
    WrongCategory,
    #[error("required field '{0}' was blank or missing")]
    MissingField(&'static str),
    #[error("field '{0}' could not be decoded")]
    InvalidField(&'static str),
}

/// Split a raw CIFP file into classified, still-unparsed records.
///
/// This is the first pass of the pipeline described in DESIGN.md §7: turn
/// the flat file into a stream of records `ff-etl` can route to per-category
/// field extractors in `crate::extract`.
pub fn classify_file(contents: &str) -> Vec<RawRecord> {
    contents.lines().filter_map(classify_line).collect()
}

pub fn classify_bytes(bytes: &[u8]) -> Result<Vec<RawRecord>, CifpError> {
    let text = std::str::from_utf8(bytes)?;
    Ok(classify_file(text))
}
