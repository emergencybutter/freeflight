//! Golden-file test over **real** records from the French SIA AIXM 4.5
//! export (`AIXM4.5_all_FR_OM_2026-07-09.xml`, AIRAC 07/26). The four
//! features below are copied verbatim from that file (a closed New
//! Caledonia landing site, the Guadeloupe VOR, the Saint-Yan NDB, and the
//! UTELA designated point) — chosen because between them they exercise the
//! parts most likely to be got wrong from the spec alone:
//!
//! - the `<OrgUid><txtName>` region name that precedes each feature's own
//!   `<txtName>` (the parser must NOT read it as the feature name),
//! - southern/western-hemisphere DMS coordinates (negative results),
//! - both navaid frequency units (`MHZ` for the VOR, `KHZ` for the NDB),
//! - navaid position carried inside the `<XxxUid>` identity block.
//!
//! This is the `ff-aixm` analog of `ff-nasr`'s `tests/real_nasr.rs`.
use ff_aixm::parse_snapshot;
use ff_core::NavaidType;

// Verbatim excerpt (real SIA data), wrapped in a minimal snapshot root.
const REAL_EXCERPT: &[u8] = br#"<?xml version="1.0" encoding="UTF-8"?>
<AIXM-Snapshot version="4.5" origin="Sia-France" effective="2026-07-09T00:00:00.000+02:00">
    <Ahp>
        <AhpUid mid="1521876"><codeId>NW01</codeId></AhpUid>
        <OrgUid mid="1520816"><txtName>NOUVELLE CALEDONIE</txtName></OrgUid>
        <txtName>POUM MALABOU (CLOSED)</txtName>
        <codeType>LS</codeType>
        <geoLat>201721.00S</geoLat>
        <geoLong>1640558.00E</geoLong>
        <codeDatum>WGE</codeDatum>
        <valCrc>34D2880A</valCrc>
        <txtRmk>AD CLOSED</txtRmk>
    </Ahp>
    <Vor>
        <VorUid mid="1526850">
            <codeId>PPR</codeId>
            <geoLat>161554.70N</geoLat>
            <geoLong>0613224.50W</geoLong>
        </VorUid>
        <OrgUid mid="1520788"><txtName>ANTILLES FRANCAISES</txtName></OrgUid>
        <txtName>GUADELOUPE MARYSE CONDE</txtName>
        <codeType>VOR</codeType>
        <valFreq>112.9</valFreq>
        <uomFreq>MHZ</uomFreq>
        <codeDatum>WGE</codeDatum>
        <valElev>44</valElev>
        <uomDistVer>FT</uomDistVer>
        <Vtt><codeWorkHr>H24</codeWorkHr></Vtt>
    </Vor>
    <Ndb>
        <NdbUid mid="1523956">
            <codeId>SN</codeId>
            <geoLat>461739.74N</geoLat>
            <geoLong>0040716.98E</geoLong>
        </NdbUid>
        <OrgUid mid="1520800"><txtName>FRANCE</txtName></OrgUid>
        <txtName>SAINT YAN</txtName>
        <valFreq>430</valFreq>
        <uomFreq>KHZ</uomFreq>
        <codeClass>B</codeClass>
        <codeDatum>WGE</codeDatum>
        <valElev>1431</valElev>
        <uomDistVer>FT</uomDistVer>
        <Ntt><codeWorkHr>H24</codeWorkHr></Ntt>
    </Ndb>
    <Dpn>
        <DpnUid mid="1538188">
            <codeId>UTELA</codeId>
            <geoLat>485421.20N</geoLat>
            <geoLong>0025737.60E</geoLong>
        </DpnUid>
        <codeDatum>WGE</codeDatum>
        <codeType>ICAO</codeType>
        <txtName>UTELA</txtName>
    </Dpn>
    <Rwy>
        <RwyUid mid="1527957">
            <AhpUid mid="1520886"><codeId>LFRC</codeId></AhpUid>
            <txtDesig>10/28</txtDesig>
        </RwyUid>
        <valLen>2440</valLen>
        <valWid>45</valWid>
        <uomDimRwy>M</uomDimRwy>
        <codeComposition>CONC</codeComposition>
        <txtPcnNote>40 R/B/W/T</txtPcnNote>
    </Rwy>
    <Rdn>
        <RdnUid mid="1534979">
            <RwyUid mid="1527957">
                <AhpUid mid="1520886"><codeId>LFRC</codeId></AhpUid>
                <txtDesig>10/28</txtDesig>
            </RwyUid>
            <txtDesig>10</txtDesig>
        </RdnUid>
        <geoLat>493907.92N</geoLat>
        <geoLong>0012912.71W</geoLong>
        <valTrueBrg>100.749</valTrueBrg>
        <valMagBrg>100.1</valMagBrg>
    </Rdn>
    <Rdn>
        <RdnUid mid="1535037">
            <RwyUid mid="1527957">
                <AhpUid mid="1520886"><codeId>LFRC</codeId></AhpUid>
                <txtDesig>10/28</txtDesig>
            </RwyUid>
            <txtDesig>28</txtDesig>
        </RdnUid>
        <geoLat>493853.17N</geoLat>
        <geoLong>0012713.19W</geoLong>
        <valTrueBrg>280.774</valTrueBrg>
        <valMagBrg>280.11</valMagBrg>
    </Rdn>
    <Rte>
        <RteUid mid="1543698"><txtDesig>L615</txtDesig><txtLocDesig>EUR</txtLocDesig></RteUid>
    </Rte>
    <Rsg>
        <RsgUid mid="1545074">
            <RteUid mid="1543698"><txtDesig>L615</txtDesig><txtLocDesig>EUR</txtLocDesig></RteUid>
            <DpnUidSta mid="1537228"><codeId>LUREN</codeId><geoLat>480133.00N</geoLat><geoLong>0035450.00E</geoLong></DpnUidSta>
            <DmeUidEnd mid="1527375"><codeId>BRY</codeId><geoLat>482425.18N</geoLat><geoLong>0031741.23E</geoLong></DmeUidEnd>
        </RsgUid>
        <codeType>RNAV</codeType>
        <codeLvl>L</codeLvl>
        <valDistVerUpper>115</valDistVerUpper>
        <uomDistVerUpper>FL</uomDistVerUpper>
        <valDistVerLower>065</valDistVerLower>
        <uomDistVerLower>FL</uomDistVerLower>
    </Rsg>
    <Rsg>
        <RsgUid mid="23717169">
            <RteUid mid="1543698"><txtDesig>L615</txtDesig><txtLocDesig>EUR</txtLocDesig></RteUid>
            <VorUidSta mid="1526958"><codeId>DJL</codeId><geoLat>471614.82N</geoLat><geoLong>0050550.38E</geoLong></VorUidSta>
            <DpnUidEnd mid="1537228"><codeId>LUREN</codeId><geoLat>480133.00N</geoLat><geoLong>0035450.00E</geoLong></DpnUidEnd>
        </RsgUid>
        <codeType>RNAV</codeType>
        <codeLvl>B</codeLvl>
        <valDistVerUpper>345</valDistVerUpper>
        <uomDistVerUpper>FL</uomDistVerUpper>
        <valDistVerLower>065</valDistVerLower>
        <uomDistVerLower>FL</uomDistVerLower>
    </Rsg>
    <Ase>
        <AseUid mid="1561293"><codeType>R</codeType><codeId>LFR92</codeId></AseUid>
        <txtName>92</txtName>
        <codeDistVerUpper>HEI</codeDistVerUpper>
        <valDistVerUpper>1000</valDistVerUpper>
        <uomDistVerUpper>FT</uomDistVerUpper>
        <codeDistVerLower>HEI</codeDistVerLower>
        <valDistVerLower>0</valDistVerLower>
        <uomDistVerLower>FT</uomDistVerLower>
    </Ase>
    <Abd>
        <AbdUid mid="1568438"><AseUid mid="1561293"><codeType>R</codeType><codeId>LFR92</codeId></AseUid></AbdUid>
        <Avx><codeType>GRC</codeType><geoLat>483533.00N</geoLat><geoLong>0054448.00E</geoLong></Avx>
        <Avx><codeType>GRC</codeType><geoLat>483140.00N</geoLat><geoLong>0054830.00E</geoLong></Avx>
        <Avx><codeType>GRC</codeType><geoLat>482900.00N</geoLat><geoLong>0054530.00E</geoLong></Avx>
        <Avx><codeType>GRC</codeType><geoLat>483215.00N</geoLat><geoLong>0054005.00E</geoLong></Avx>
    </Abd>
</AIXM-Snapshot>"#;

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-4
}

#[test]
fn parses_real_sia_records() {
    let data = parse_snapshot(REAL_EXCERPT, "LF").unwrap();

    // ---- AIRAC effective date (for Licence Ouverte attribution) ----
    assert_eq!(data.effective.as_deref(), Some("2026-07-09"));

    // ---- airport (with the OrgUid region-name trap) ----
    assert_eq!(data.airports.len(), 1);
    let a = &data.airports[0];
    assert_eq!(a.icao, "NW01");
    // The critical assertion: the airport's own name, NOT its OrgUid
    // region "NOUVELLE CALEDONIE".
    assert_eq!(a.name, "POUM MALABOU (CLOSED)");
    // 20°17'21"S, 164°05'58"E — southern hemisphere is negative.
    assert!(close(a.lat, -(20.0 + 17.0 / 60.0 + 21.0 / 3600.0)), "lat={}", a.lat);
    assert!(close(a.lon, 164.0 + 5.0 / 60.0 + 58.0 / 3600.0), "lon={}", a.lon);

    // ---- VOR (MHz -> kHz, western hemisphere) ----
    let vor = data.navaids.iter().find(|n| n.ident == "PPR").expect("PPR VOR");
    assert_eq!(vor.navaid_type, NavaidType::Vor);
    assert_eq!(vor.freq_khz, Some(112_900));
    assert_eq!(vor.elevation_ft, Some(44));
    assert_eq!(vor.region, "LF");
    assert!(close(vor.lon, -(61.0 + 32.0 / 60.0 + 24.50 / 3600.0)), "lon={}", vor.lon);

    // ---- NDB (kHz stays kHz) ----
    let ndb = data.navaids.iter().find(|n| n.ident == "SN").expect("SN NDB");
    assert_eq!(ndb.navaid_type, NavaidType::Ndb);
    assert_eq!(ndb.freq_khz, Some(430));
    assert_eq!(ndb.elevation_ft, Some(1431));

    // ---- designated point (waypoint) ----
    assert_eq!(data.waypoints.len(), 1);
    let w = &data.waypoints[0];
    assert_eq!(w.ident, "UTELA");
    assert!(close(w.lat, 48.0 + 54.0 / 60.0 + 21.20 / 3600.0), "lat={}", w.lat);

    // ---- runway (Rwy + two Rdn directions joined) ----
    assert_eq!(data.runways.len(), 1);
    let r = &data.runways[0];
    assert_eq!(r.airport_icao, "LFRC");
    assert_eq!(r.ident, "10/28");
    assert_eq!(r.length_ft, 8005); // 2440 m
    assert_eq!(r.width_ft, 148); // 45 m
    assert_eq!(r.surface, ff_core::RunwaySurface::Concrete);
    // Directions matched to their thresholds, low/high by designator order.
    assert_eq!(r.low_end.ident, "10");
    assert_eq!(r.low_end.heading_deg, 100.749);
    assert!(close(r.low_end.lat, 49.0 + 39.0 / 60.0 + 7.92 / 3600.0), "lat={}", r.low_end.lat);
    assert!(close(r.low_end.lon, -(1.0 + 29.0 / 60.0 + 12.71 / 3600.0)), "lon={}", r.low_end.lon);
    assert_eq!(r.high_end.ident, "28");
    assert_eq!(r.high_end.heading_deg, 280.774);

    // ---- airway (Rte + two Rsg segments, chained into order) ----
    assert_eq!(data.airways.len(), 1);
    let awy = &data.airways[0];
    assert_eq!(awy.ident, "L615");
    assert_eq!(awy.kind, ff_core::AirwayKind::RnavLow); // L + B segments, no U

    // Segments were given as LUREN->BRY and DJL->LUREN; chaining yields the
    // path DJL -> LUREN -> BRY.
    let l615: Vec<_> = data
        .airway_legs
        .iter()
        .filter(|l| l.airway_ident == "L615")
        .collect();
    assert_eq!(l615.len(), 3);
    assert_eq!(l615[0].fix_ident, "DJL");
    assert_eq!(l615[0].seq, 1);
    assert_eq!(l615[1].fix_ident, "LUREN");
    assert_eq!(l615[1].max_altitude_ft, Some(34_500)); // FL345 on DJL->LUREN
    assert_eq!(l615[2].fix_ident, "BRY");
    assert_eq!(l615[2].min_altitude_ft, Some(6_500)); // FL065
    assert_eq!(l615[2].max_altitude_ft, Some(11_500)); // FL115 on LUREN->BRY

    // ---- airspace (Ase + Abd, 4 straight vertices) ----
    assert_eq!(data.airspaces.len(), 1);
    let a = &data.airspaces[0];
    // id is the AseUid mid (unique); name is the codeId designation.
    assert_eq!(a.id, "1561293");
    assert_eq!(a.name, "LFR92");
    assert_eq!(a.class, ff_core::AirspaceClass::SpecialUse(ff_core::SpecialUseKind::Restricted));
    assert_eq!(a.ceiling, ff_core::AltitudeLimit::Agl(1000));
    assert_eq!(a.floor, ff_core::AltitudeLimit::Agl(0));
    assert_eq!(a.boundary.points.len(), 4);
    let (lat0, lon0) = a.boundary.points[0];
    assert!(close(lat0, 48.0 + 35.0 / 60.0 + 33.0 / 3600.0), "lat0={lat0}");
    assert!(close(lon0, 5.0 + 44.0 / 60.0 + 48.0 / 3600.0), "lon0={lon0}");
}
