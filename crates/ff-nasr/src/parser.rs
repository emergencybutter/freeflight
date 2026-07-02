use crate::records::{AptBaseRow, AptFrequencyRow, AptRunwayRow};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum NasrError {
    #[error("CSV parse error: {0}")]
    Csv(#[from] csv::Error),
}

pub fn parse_apt_base(csv_bytes: &[u8]) -> Result<Vec<AptBaseRow>, NasrError> {
    let mut reader = csv::Reader::from_reader(csv_bytes);
    reader
        .deserialize()
        .collect::<Result<Vec<_>, _>>()
        .map_err(NasrError::from)
}

pub fn parse_apt_runway(csv_bytes: &[u8]) -> Result<Vec<AptRunwayRow>, NasrError> {
    let mut reader = csv::Reader::from_reader(csv_bytes);
    reader
        .deserialize()
        .collect::<Result<Vec<_>, _>>()
        .map_err(NasrError::from)
}

pub fn parse_apt_frequency(csv_bytes: &[u8]) -> Result<Vec<AptFrequencyRow>, NasrError> {
    let mut reader = csv::Reader::from_reader(csv_bytes);
    reader
        .deserialize()
        .collect::<Result<Vec<_>, _>>()
        .map_err(NasrError::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_apt_base_csv() {
        let csv = "ARPT_ID,ICAO_ID,ARPT_NAME,LAT_DECIMAL,LONG_DECIMAL,ELEV,SITE_TYPE_CODE\n\
                   SFO,KSFO,SAN FRANCISCO INTL,37.6188,-122.375,13,A\n";
        let rows = parse_apt_base(csv.as_bytes()).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].icao_id.as_deref(), Some("KSFO"));
    }
}
