//! Streaming parser over an AIXM 4.5 `<AIXM-Snapshot>` document.
//!
//! Event-based (quick-xml) rather than serde-derived: the snapshot is one
//! large, flat document interleaving many heterogeneous feature types, so
//! streaming past the features we don't yet handle is simpler and cheaper
//! than modeling the whole schema. Each handled feature's subtree is
//! collapsed into a flat `field -> text` map, then interpreted by
//! [`crate::convert`].
//!
//! Field capture is **scoped**, not a blind flatten: only leaves that are
//! a *direct child of the feature* or a child of the feature's *own*
//! `<TagUid>` identity block are recorded. This matters because AIXM 4.5
//! features embed *referenced* Uid blocks — every feature carries an
//! `<OrgUid><txtName>…</txtName></OrgUid>` naming its issuing org/region,
//! which appears **before** the feature's own `<txtName>`. A naive
//! first-occurrence flatten would read the org/region name as the
//! airport's name (confirmed against the real SIA export). Scoping to the
//! feature's own level and its own Uid avoids that.
use crate::airspace::{ase_from_paths, assemble_airspaces, AbdRaw};
use crate::airway::{assemble_airways, rsg_raw_from_paths, rte_raw_from_paths};
use crate::convert::{airport_from_fields, navaid_from_fields, waypoint_from_fields};
use crate::runway::{assemble_runways, rdn_raw_from_paths, rwy_raw_from_paths};
use ff_core::{Airport, AirspaceVolume, Airway, AirwayLeg, Navaid, Runway, Waypoint};
use quick_xml::events::Event;
use quick_xml::Reader;
use std::collections::HashMap;
use std::io::BufRead;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AixmError {
    #[error("XML parse error: {0}")]
    Xml(#[from] quick_xml::Error),
}

/// Converted `ff-core` output of one AIXM snapshot.
#[derive(Debug, Default)]
pub struct AixmData {
    pub airports: Vec<Airport>,
    pub navaids: Vec<Navaid>,
    pub waypoints: Vec<Waypoint>,
    pub runways: Vec<Runway>,
    pub airways: Vec<Airway>,
    pub airway_legs: Vec<AirwayLeg>,
    pub airspaces: Vec<AirspaceVolume>,
    /// AIRAC effective date from the `<AIXM-Snapshot effective="…">` root
    /// (date part only, e.g. `"2026-07-09"`) — needed for the Licence
    /// Ouverte attribution (which requires the data's update date).
    pub effective: Option<String>,
}

/// The navaid feature tags mapped in [`crate::convert::navaid_from_fields`].
const NAVAID_TAGS: [&str; 4] = ["Vor", "Ndb", "Dme", "Tcn"];

fn local_name(raw: &[u8]) -> String {
    String::from_utf8_lossy(raw).into_owned()
}

/// Parses an AIXM 4.5 snapshot into `ff-core` types. `region` is the
/// dataset's ICAO region (e.g. `"LF"` for France), stamped onto navaids
/// and waypoints since AIXM carries no FAA-style region code per feature.
pub fn parse_snapshot(xml: &[u8], region: &str) -> Result<AixmData, AixmError> {
    let mut reader = Reader::from_reader(xml);
    let mut data = AixmData::default();
    let mut buf = Vec::new();

    // Runways are a two-feature join (Rwy + Rdn), and the two feature types
    // can appear in any order, so buffer the raw records and assemble after
    // the single streaming pass.
    let mut raw_rwys = Vec::new();
    let mut raw_rdns = Vec::new();
    // Airways are likewise a two-feature join (Rte + Rsg), reconstructed
    // after the pass since segments carry no sequence.
    let mut raw_rtes = Vec::new();
    let mut raw_rsgs = Vec::new();
    // Airspace: Ase (metadata) + Abd (border geometry), joined by codeId.
    let mut raw_ases = Vec::new();
    let mut raw_abds = Vec::new();

    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Start(e) => {
                let tag = local_name(e.local_name().as_ref());
                if tag == "AIXM-Snapshot" {
                    // Root element (fires once): grab the AIRAC effective
                    // date, keeping only the date part before the `T`.
                    data.effective = e
                        .attributes()
                        .flatten()
                        .find(|a| a.key.local_name().as_ref() == b"effective")
                        .map(|a| {
                            String::from_utf8_lossy(&a.value)
                                .split('T')
                                .next()
                                .unwrap_or_default()
                                .trim()
                                .to_string()
                        })
                        .filter(|s| !s.is_empty());
                } else if tag == "Ahp" {
                    let f = read_feature(&mut reader, "Ahp")?;
                    if let Some(a) = airport_from_fields(&f) {
                        data.airports.push(a);
                    }
                } else if NAVAID_TAGS.contains(&tag.as_str()) {
                    let f = read_feature(&mut reader, &tag)?;
                    if let Some(n) = navaid_from_fields(&tag, &f, region) {
                        data.navaids.push(n);
                    }
                } else if tag == "Dpn" {
                    let f = read_feature(&mut reader, "Dpn")?;
                    if let Some(w) = waypoint_from_fields(&f, region) {
                        data.waypoints.push(w);
                    }
                } else if tag == "Rwy" {
                    // Path-keyed: Rwy nests the airport ICAO at
                    // RwyUid/AhpUid/codeId (see runway.rs).
                    let p = read_feature_paths(&mut reader, "Rwy")?;
                    if let Some(r) = rwy_raw_from_paths(&p) {
                        raw_rwys.push(r);
                    }
                } else if tag == "Rdn" {
                    let p = read_feature_paths(&mut reader, "Rdn")?;
                    if let Some(r) = rdn_raw_from_paths(&p) {
                        raw_rdns.push(r);
                    }
                } else if tag == "Rte" {
                    let p = read_feature_paths(&mut reader, "Rte")?;
                    if let Some(r) = rte_raw_from_paths(&p) {
                        raw_rtes.push(r);
                    }
                } else if tag == "Rsg" {
                    let p = read_feature_paths(&mut reader, "Rsg")?;
                    if let Some(r) = rsg_raw_from_paths(&p) {
                        raw_rsgs.push(r);
                    }
                } else if tag == "Ase" {
                    let p = read_feature_paths(&mut reader, "Ase")?;
                    if let Some(a) = ase_from_paths(&p) {
                        raw_ases.push(a);
                    }
                } else if tag == "Abd" {
                    // Custom reader: an Abd holds a *list* of <Avx> or a
                    // <Circle>, which the first-occurrence readers can't
                    // capture.
                    raw_abds.push(read_abd(&mut reader)?);
                }
                // Any other feature: its events stream past and are ignored.
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }

    data.runways = assemble_runways(&raw_rwys, &raw_rdns);
    let (airways, airway_legs) = assemble_airways(&raw_rtes, &raw_rsgs);
    data.airways = airways;
    data.airway_legs = airway_legs;
    data.airspaces = assemble_airspaces(&raw_ases, &raw_abds);
    Ok(data)
}

/// Reads an `<Abd>` (airspace border): captures the linked airspace id
/// (the first `codeId`, from `AbdUid/AseUid`) and collects the border
/// geometry — a list of `<Avx>` vertices *or* a single `<Circle>`, each
/// read as its own flat field-map via [`read_feature`]. Positioned among
/// the `Abd`'s children on entry, returns at its close.
fn read_abd<B: BufRead>(reader: &mut Reader<B>) -> Result<AbdRaw, AixmError> {
    let mut airspace_id: Option<String> = None;
    let mut vertices = Vec::new();
    let mut circle = None;
    let mut buf = Vec::new();
    let mut depth = 0usize;

    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Start(e) => {
                let name = local_name(e.local_name().as_ref());
                if name == "Avx" {
                    vertices.push(read_feature(reader, "Avx")?);
                } else if name == "Circle" {
                    circle = Some(read_feature(reader, "Circle")?);
                } else {
                    // The airspace link is the `AbdUid/AseUid` feature's `mid`
                    // — matches the Ase's own `AseUid@mid`, giving a reliable
                    // 1:1 join even for multi-part airspaces sharing a codeId.
                    if name == "AseUid" && airspace_id.is_none() {
                        airspace_id = e
                            .attributes()
                            .flatten()
                            .find(|a| a.key.local_name().as_ref() == b"mid")
                            .map(|a| String::from_utf8_lossy(&a.value).trim().to_string());
                    }
                    depth += 1;
                }
            }
            Event::End(e) => {
                if depth == 0 && local_name(e.local_name().as_ref()) == "Abd" {
                    break;
                }
                depth = depth.saturating_sub(1);
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }

    Ok(AbdRaw {
        airspace_id: airspace_id.unwrap_or_default(),
        vertices,
        circle,
    })
}

/// Reads from just after a feature's start tag up to and including its
/// matching end tag, collecting *scoped* leaf-element text into a flat map
/// (see the module docs on why scoping — not a blind flatten — is needed).
/// A leaf's text is recorded only when the element is a direct child of the
/// feature, or a child of the feature's own `<{end_tag}Uid>` identity
/// block; text inside referenced blocks (`OrgUid`, `Vtt`, …) is ignored.
/// First value wins per field. A malformed/truncated text value is skipped
/// rather than failing the whole document.
fn read_feature<B: BufRead>(
    reader: &mut Reader<B>,
    end_tag: &str,
) -> Result<HashMap<String, String>, AixmError> {
    let primary_uid = format!("{end_tag}Uid");
    let mut fields = HashMap::new();
    let mut buf = Vec::new();
    // Names of the currently-open elements *below* the feature (the feature
    // itself isn't pushed — we enter positioned among its children).
    let mut stack: Vec<String> = Vec::new();

    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Start(e) => stack.push(local_name(e.local_name().as_ref())),
            Event::Text(e) => {
                // Record only: a direct child of the feature (depth 1), or
                // a child of the feature's own identity block (depth 2 with
                // that block as parent).
                let in_scope = match stack.len() {
                    1 => true,
                    2 => stack[0] == primary_uid,
                    _ => false,
                };
                if in_scope {
                    if let (Some(key), Ok(text)) = (stack.last(), e.unescape()) {
                        let text = text.trim();
                        if !text.is_empty() {
                            fields.entry(key.clone()).or_insert_with(|| text.to_string());
                        }
                    }
                }
            }
            Event::End(e) => {
                if stack.is_empty() && local_name(e.local_name().as_ref()) == end_tag {
                    break;
                }
                stack.pop();
            }
            Event::Eof => break, // truncated document: stop gracefully
            _ => {}
        }
        buf.clear();
    }

    Ok(fields)
}

/// Like [`read_feature`] but keys each leaf by its **full element path**
/// within the feature (ancestors joined by `/`, e.g.
/// `"RwyUid/AhpUid/codeId"`), first value winning. Used for the runway
/// features (`Rwy`/`Rdn`), whose identity fields are nested several levels
/// deep and whose `txtDesig` appears at two different depths — a flat
/// last-segment key would collide them (see [`crate::runway`]).
fn read_feature_paths<B: BufRead>(
    reader: &mut Reader<B>,
    end_tag: &str,
) -> Result<HashMap<String, String>, AixmError> {
    let mut fields = HashMap::new();
    let mut buf = Vec::new();
    let mut stack: Vec<String> = Vec::new();

    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Start(e) => {
                stack.push(local_name(e.local_name().as_ref()));
                // Capture attributes as `path@attr` keys (e.g. `AseUid@mid`)
                // — some features' stable identity is an attribute, not text.
                let path = stack.join("/");
                for attr in e.attributes().flatten() {
                    let name = local_name(attr.key.local_name().as_ref());
                    // Attribute values used here (e.g. `mid`) are plain ASCII,
                    // so the raw bytes suffice (no entity unescaping needed).
                    let val = String::from_utf8_lossy(&attr.value);
                    let val = val.trim();
                    if !val.is_empty() {
                        fields
                            .entry(format!("{path}@{name}"))
                            .or_insert_with(|| val.to_string());
                    }
                }
            }
            Event::Text(e) => {
                if !stack.is_empty() {
                    if let Ok(text) = e.unescape() {
                        let text = text.trim();
                        if !text.is_empty() {
                            fields.entry(stack.join("/")).or_insert_with(|| text.to_string());
                        }
                    }
                }
            }
            Event::End(e) => {
                if stack.is_empty() && local_name(e.local_name().as_ref()) == end_tag {
                    break;
                }
                stack.pop();
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }

    Ok(fields)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ff_core::NavaidType;

    // A minimal, standards-shaped AIXM 4.5 snapshot. NOT a captured SIA
    // file — a real `AIXM4.5_all_FR_OM_*.xml` still needs to back a
    // golden-file test (see crate docs) before the schema is validated.
    const SAMPLE: &[u8] = br#"<?xml version="1.0" encoding="ISO-8859-1"?>
<AIXM-Snapshot version="4.5" origin="SIA" effective="2026-07-09">
  <Ahp>
    <AhpUid mid="1"><codeId>LFPG</codeId></AhpUid>
    <OrgUid mid="9"><txtName>FRANCE</txtName></OrgUid>
    <txtName>PARIS CHARLES DE GAULLE</txtName>
    <codeIcao>LFPG</codeIcao>
    <codeIata>CDG</codeIata>
    <codeType>AD</codeType>
    <geoLat>490042.00N</geoLat>
    <geoLong>0023259.00E</geoLong>
    <valElev>392</valElev>
    <uomDistVer>FT</uomDistVer>
  </Ahp>
  <Vor>
    <VorUid mid="2">
      <codeId>PON</codeId>
      <geoLat>490900.00N</geoLat>
      <geoLong>0020200.00E</geoLong>
    </VorUid>
    <txtName>PONTOISE</txtName>
    <valFreq>111.6</valFreq>
    <uomFreq>MHZ</uomFreq>
  </Vor>
  <Dpn>
    <DpnUid mid="3">
      <codeId>ABABA</codeId>
      <geoLat>483000.00N</geoLat>
      <geoLong>0020600.00E</geoLong>
    </DpnUid>
    <codeType>ICAO</codeType>
  </Dpn>
  <Uni><UniUid mid="4"><txtName>IGNORED FEATURE</txtName></UniUid></Uni>
</AIXM-Snapshot>"#;

    #[test]
    fn parses_snapshot_into_ff_core_types() {
        let data = parse_snapshot(SAMPLE, "LF").unwrap();

        assert_eq!(data.airports.len(), 1);
        let a = &data.airports[0];
        assert_eq!(a.icao, "LFPG");
        assert_eq!(a.iata.as_deref(), Some("CDG"));
        assert_eq!(a.name, "PARIS CHARLES DE GAULLE");
        assert_eq!(a.elevation_ft, 392);
        assert!((a.lat - (49.0 + 42.0 / 3600.0)).abs() < 1e-6);
        assert!((a.lon - (2.0 + 32.0 / 60.0 + 59.0 / 3600.0)).abs() < 1e-6);

        assert_eq!(data.navaids.len(), 1);
        let n = &data.navaids[0];
        assert_eq!(n.ident, "PON");
        assert_eq!(n.navaid_type, NavaidType::Vor);
        assert_eq!(n.freq_khz, Some(111_600));
        assert_eq!(n.region, "LF");
        // Coordinates come from inside the <VorUid> identity block.
        assert!((n.lat - (49.0 + 9.0 / 60.0)).abs() < 1e-6);

        assert_eq!(data.waypoints.len(), 1);
        assert_eq!(data.waypoints[0].ident, "ABABA");
        assert_eq!(data.waypoints[0].region, "LF");
    }

    #[test]
    fn ignores_unhandled_features_without_confusing_field_scope() {
        // The trailing <Uni> feature's <txtName> must not leak into any
        // parsed feature, and must not error.
        let data = parse_snapshot(SAMPLE, "LF").unwrap();
        assert!(data.airports.iter().all(|a| a.name != "IGNORED FEATURE"));
    }
}
