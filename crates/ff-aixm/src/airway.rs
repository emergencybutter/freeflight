//! Airway assembly from AIXM 4.5 `Rte` (route) + `Rsg` (route segment)
//! features — the richest of the multi-feature joins.
//!
//! `Rte` is just the airway's identity (`RteUid/txtDesig`, e.g. `"L615"`).
//! Each `Rsg` is one segment linking a **start** fix to an **end** fix,
//! with vertical limits (`valDistVerLower/Upper` as flight levels). Two
//! wrinkles drive the design:
//!
//! - **The fix elements are polymorphic**: a segment's endpoints are named
//!   `<{Dpn|Vor|Dme|Ndb}Uid{Sta|End}>` by fix type (confirmed against the
//!   real export), so the start/end `codeId` is found by scanning for the
//!   `…UidSta/codeId` / `…UidEnd/codeId` path rather than a fixed tag.
//! - **Segments carry no sequence number** (unlike CIFP airway records),
//!   so leg order is *reconstructed* by chaining segments end-to-start into
//!   a path — see [`chain_fixes`].
use ff_core::{Airway, AirwayKind, AirwayLeg};
use std::collections::{HashMap, HashSet};

type Fields = HashMap<String, String>;

fn get<'a>(f: &'a Fields, key: &str) -> Option<&'a str> {
    f.get(key).map(String::as_str).filter(|s| !s.is_empty())
}

/// Finds a segment endpoint's `codeId` by role (`"Sta"` or `"End"`),
/// matching whichever `RsgUid/<FixType>Uid{role}/codeId` path is present.
fn role_fix(f: &Fields, role: &str) -> Option<String> {
    let suffix = format!("Uid{role}/codeId");
    f.iter()
        .find(|(k, v)| k.starts_with("RsgUid/") && k.ends_with(&suffix) && !v.is_empty())
        .map(|(_, v)| v.clone())
}

/// A flight-level limit → feet: `valX` is an FL number, `uomX` its unit.
fn fl_to_ft(f: &Fields, val_key: &str, uom_key: &str) -> Option<u32> {
    let v: f64 = get(f, val_key)?.parse().ok()?;
    match get(f, uom_key) {
        Some("FL") => Some((v * 100.0).round() as u32),
        Some("FT") => Some(v.round() as u32),
        _ => None,
    }
}

pub(crate) struct RteRaw {
    pub ident: String,
}

pub(crate) struct RsgRaw {
    pub airway_ident: String,
    pub start_fix: String,
    pub end_fix: String,
    pub min_ft: Option<u32>,
    pub max_ft: Option<u32>,
    pub code_lvl: Option<String>,
    pub code_type: Option<String>,
}

pub(crate) fn rte_raw_from_paths(f: &Fields) -> Option<RteRaw> {
    Some(RteRaw {
        ident: get(f, "RteUid/txtDesig")?.to_string(),
    })
}

pub(crate) fn rsg_raw_from_paths(f: &Fields) -> Option<RsgRaw> {
    Some(RsgRaw {
        airway_ident: get(f, "RsgUid/RteUid/txtDesig")?.to_string(),
        start_fix: role_fix(f, "Sta")?,
        end_fix: role_fix(f, "End")?,
        min_ft: fl_to_ft(f, "valDistVerLower", "uomDistVerLower"),
        max_ft: fl_to_ft(f, "valDistVerUpper", "uomDistVerUpper"),
        code_lvl: get(f, "codeLvl").map(str::to_string),
        code_type: get(f, "codeType").map(str::to_string),
    })
}

/// Maps onto `ff-core`'s US-shaped [`AirwayKind`] from whether the airway
/// is RNAV and upper-level. VERIFY: French routes are all `RNAV` and split
/// L/B/U by `codeLvl`; the mapping (`U`→high, else low) is a coarse fit to
/// an enum built for the US V/J/T/Q scheme.
fn airway_kind(rnav: bool, upper: bool) -> AirwayKind {
    match (rnav, upper) {
        (true, true) => AirwayKind::RnavHigh,
        (true, false) => AirwayKind::RnavLow,
        (false, true) => AirwayKind::Jet,
        (false, false) => AirwayKind::Victor,
    }
}

/// Orders an airway's segments into a single fix sequence by chaining
/// `start → end` edges: the head is a fix that is only ever a start
/// (in-degree 0), and each step follows an unused outgoing edge. Returns
/// `(fix, min_ft, max_ft)` per fix, where the altitudes are those of the
/// segment *arriving* at that fix (the head has none).
///
/// This is a heuristic for what AIXM doesn't state explicitly. It orders
/// the common linear airway exactly; for a branch it follows one path
/// (lowest segment index) and for a cycle it stops at the repeat — so a
/// non-linear route may omit some fixes. Deterministic regardless of
/// segment order in the file.
fn chain_fixes(segs: &[&RsgRaw]) -> Vec<(String, Option<u32>, Option<u32>)> {
    if segs.is_empty() {
        return Vec::new();
    }

    let mut out: HashMap<&str, Vec<usize>> = HashMap::new();
    let mut indeg: HashMap<&str, i32> = HashMap::new();
    for (i, s) in segs.iter().enumerate() {
        out.entry(&s.start_fix).or_default().push(i);
        *indeg.entry(&s.end_fix).or_insert(0) += 1;
        indeg.entry(&s.start_fix).or_insert(0);
    }

    // Head: an in-degree-0 fix (lowest ident for determinism); if the graph
    // is a pure cycle, fall back to the lowest start fix.
    let mut zero_indeg: Vec<&str> = indeg
        .iter()
        .filter(|(_, &d)| d == 0)
        .map(|(k, _)| *k)
        .collect();
    zero_indeg.sort_unstable();
    let head = zero_indeg
        .first()
        .copied()
        .or_else(|| segs.iter().map(|s| s.start_fix.as_str()).min())
        .expect("non-empty segments have a start fix");

    let mut used = vec![false; segs.len()];
    let mut visited: HashSet<&str> = HashSet::new();
    let mut order: Vec<(String, Option<u32>, Option<u32>)> = vec![(head.to_string(), None, None)];
    visited.insert(head);
    let mut current = head;

    while let Some(i) = out
        .get(current)
        .and_then(|edges| edges.iter().copied().find(|&i| !used[i]))
    {
        used[i] = true;
        let s = segs[i];
        if visited.contains(s.end_fix.as_str()) {
            break; // cycle / rejoin — stop rather than loop
        }
        order.push((s.end_fix.clone(), s.min_ft, s.max_ft));
        visited.insert(&s.end_fix);
        current = &s.end_fix;
    }

    order
}

/// Builds airways and their ordered legs. Airway identity comes from the
/// `Rte` features (plus any ident that only appears in segments); kind and
/// legs come from that airway's `Rsg` segments.
pub(crate) fn assemble_airways(rtes: &[RteRaw], rsgs: &[RsgRaw]) -> (Vec<Airway>, Vec<AirwayLeg>) {
    let mut by_airway: HashMap<&str, Vec<&RsgRaw>> = HashMap::new();
    for s in rsgs {
        by_airway.entry(&s.airway_ident).or_default().push(s);
    }

    // Airway idents: the Rte list first (stable), then any segment-only
    // ident (sorted) so nothing is silently lost.
    let mut idents: Vec<String> = Vec::new();
    let mut seen: HashSet<&str> = HashSet::new();
    for r in rtes {
        if seen.insert(r.ident.as_str()) {
            idents.push(r.ident.clone());
        }
    }
    let mut segment_only: Vec<&str> = by_airway
        .keys()
        .copied()
        .filter(|k| !seen.contains(k))
        .collect();
    segment_only.sort_unstable();
    idents.extend(segment_only.into_iter().map(str::to_string));

    let mut airways = Vec::with_capacity(idents.len());
    let mut legs = Vec::new();
    for ident in &idents {
        let segs = by_airway
            .get(ident.as_str())
            .map(Vec::as_slice)
            .unwrap_or(&[]);

        let kind = if segs.is_empty() {
            AirwayKind::Other
        } else {
            let upper = segs.iter().any(|s| s.code_lvl.as_deref() == Some("U"));
            let rnav = segs.iter().all(|s| s.code_type.as_deref() == Some("RNAV"));
            airway_kind(rnav, upper)
        };
        airways.push(Airway {
            ident: ident.clone(),
            kind,
        });

        for (i, (fix, min, max)) in chain_fixes(segs).into_iter().enumerate() {
            legs.push(AirwayLeg {
                airway_ident: ident.clone(),
                seq: (i + 1) as u32,
                fix_ident: fix,
                min_altitude_ft: min,
                max_altitude_ft: max,
            });
        }
    }

    (airways, legs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(awy: &str, sta: &str, end: &str, min: Option<u32>, max: Option<u32>, lvl: &str) -> RsgRaw {
        RsgRaw {
            airway_ident: awy.into(),
            start_fix: sta.into(),
            end_fix: end.into(),
            min_ft: min,
            max_ft: max,
            code_lvl: Some(lvl.into()),
            code_type: Some("RNAV".into()),
        }
    }

    #[test]
    fn chains_segments_into_order_regardless_of_input_order() {
        let rtes = [RteRaw { ident: "L615".into() }];
        // Given out of order: LUREN->BRY listed before DJL->LUREN.
        let rsgs = [
            seg("L615", "LUREN", "BRY", Some(6500), Some(11500), "L"),
            seg("L615", "DJL", "LUREN", Some(6500), Some(34500), "B"),
        ];
        let (airways, legs) = assemble_airways(&rtes, &rsgs);

        assert_eq!(airways.len(), 1);
        assert_eq!(airways[0].ident, "L615");
        assert_eq!(airways[0].kind, AirwayKind::RnavLow); // no U segment

        let l: Vec<_> = legs.iter().map(|l| l.fix_ident.as_str()).collect();
        assert_eq!(l, vec!["DJL", "LUREN", "BRY"]);
        assert_eq!(legs[0].seq, 1);
        assert_eq!(legs[0].min_altitude_ft, None); // head fix
        // Fix altitudes come from the arriving segment.
        assert_eq!(legs[1].max_altitude_ft, Some(34500)); // DJL->LUREN
        assert_eq!(legs[2].max_altitude_ft, Some(11500)); // LUREN->BRY
    }

    #[test]
    fn upper_level_airway_is_classified_high() {
        let rtes = [RteRaw { ident: "UN491".into() }];
        let rsgs = [seg("UN491", "A", "B", Some(19500), Some(46000), "U")];
        let (airways, _) = assemble_airways(&rtes, &rsgs);
        assert_eq!(airways[0].kind, AirwayKind::RnavHigh);
    }

    #[test]
    fn route_without_segments_yields_airway_with_no_legs() {
        let rtes = [RteRaw { ident: "EMPTY1".into() }];
        let (airways, legs) = assemble_airways(&rtes, &[]);
        assert_eq!(airways.len(), 1);
        assert_eq!(airways[0].kind, AirwayKind::Other);
        assert!(legs.is_empty());
    }

    #[test]
    fn segment_only_airway_is_still_emitted() {
        // An ident present in Rsg but with no Rte must not be lost.
        let rsgs = [seg("Z999", "P", "Q", None, None, "L")];
        let (airways, legs) = assemble_airways(&[], &rsgs);
        assert_eq!(airways.len(), 1);
        assert_eq!(airways[0].ident, "Z999");
        assert_eq!(legs.len(), 2);
    }
}
