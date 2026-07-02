use crate::records::{AptBaseRow, AptRunwayEndRow, AptRunwayRow, FrqRow};
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

/// Parses `FRQ.csv` (not `APT_FREQ.csv` — see [`crate::records::FrqRow`]
/// docs for why).
pub fn parse_frq(csv_bytes: &[u8]) -> Result<Vec<FrqRow>, NasrError> {
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
        let csv = "SITE_NO,ARPT_ID,ICAO_ID,ARPT_NAME,LAT_DECIMAL,LONG_DECIMAL,ELEV,SITE_TYPE_CODE,FUEL_TYPES\n\
                   02187.,SFO,KSFO,SAN FRANCISCO INTL,37.6188,-122.375,13,A,\"100LL,A,A++\"\n";
        let rows = parse_apt_base(csv.as_bytes()).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].site_no, "02187.");
        assert_eq!(rows[0].icao_id.as_deref(), Some("KSFO"));
        assert_eq!(rows[0].fuel_types, "100LL,A,A++");
    }

    #[test]
    fn parses_minimal_apt_runway_end_csv() {
        let csv = "ARPT_ID,RWY_ID,RWY_END_ID,TRUE_ALIGNMENT,LAT_DECIMAL,LONG_DECIMAL\n\
                   PAO,13/31,13,142,37.46375061,-122.11765513\n\
                   PAO,13/31,31,322,37.45849213,-122.11243811\n";
        let rows = parse_apt_runway_end(csv.as_bytes()).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].rwy_end_id, "13");
        assert_eq!(rows[0].true_alignment, Some(142.0));
    }

    #[test]
    fn parses_minimal_frq_csv() {
        let csv =
            "FACILITY,FACILITY_TYPE,SERVICED_FACILITY,SERVICED_SITE_TYPE,FREQ,FREQ_USE,REMARK\n\
                   PAO,ATCT,PAO,AIRPORT,118.6,CTAF,\n";
        let rows = parse_frq(csv.as_bytes()).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].freq_use, "CTAF");
    }
}
