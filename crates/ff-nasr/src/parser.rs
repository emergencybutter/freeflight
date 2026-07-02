use crate::records::{AptBaseRow, AptFrequencyRow, AptRunwayEndRow, AptRunwayRow};
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

pub fn parse_apt_runway_end(csv_bytes: &[u8]) -> Result<Vec<AptRunwayEndRow>, NasrError> {
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
        let csv =
            "SITE_NO,ARPT_ID,ICAO_ID,ARPT_NAME,LAT_DECIMAL,LONG_DECIMAL,ELEV,SITE_TYPE_CODE\n\
                   12345.*A,SFO,KSFO,SAN FRANCISCO INTL,37.6188,-122.375,13,A\n";
        let rows = parse_apt_base(csv.as_bytes()).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].site_no, "12345.*A");
        assert_eq!(rows[0].icao_id.as_deref(), Some("KSFO"));
    }

    #[test]
    fn parses_minimal_apt_runway_end_csv() {
        let csv = "SITE_NO,RWY_ID,RWY_END_ID,TRUE_ALIGNMENT,LAT_DECIMAL,LONG_DECIMAL\n\
                   12345.*A,01/19,01,104,37.6167,-122.3900\n\
                   12345.*A,01/19,19,284,37.6300,-122.3600\n";
        let rows = parse_apt_runway_end(csv.as_bytes()).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].rwy_end_id, "01");
        assert_eq!(rows[0].true_alignment, Some(104.0));
    }
}
