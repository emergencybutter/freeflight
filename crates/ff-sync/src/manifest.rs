use serde::{Deserialize, Serialize};

/// Response shape of `ff-api`'s `GET /cycles/latest` (DESIGN.md §7, §8).
///
/// `ff-api` constructs this type directly rather than hand-rolling a
/// matching JSON object, so the two can't drift out of sync the way an
/// earlier version of this struct did (it had `effective_date` and
/// required `pmtiles_url`/`pmtiles_sha256` fields that `ff-api`'s actual
/// route never sent — never actually exercised against each other until
/// checked). `cycle_id` doubles as the effective date already (`ff-etl`
/// publishes cycles named e.g. `"2026-07-09"`), so there's no separate
/// `effective_date` field. `pmtiles_*` are optional because `ff-etl`'s
/// real pipeline doesn't fetch/tile chart imagery yet (see TODO.md) —
/// there's often no chart bundle to point at.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CycleManifest {
    pub cycle_id: String,
    pub sqlite_url: String,
    pub sqlite_sha256: String,
    pub pmtiles_url: Option<String>,
    pub pmtiles_sha256: Option<String>,
}

/// Whether `candidate` is a newer cycle than `current`. Cycle ids are
/// formatted `YYYY-MM[-N]` (DESIGN.md §6/§7 examples use `"2026-07"`), so
/// plain lexicographic comparison orders them correctly; `None` (no cycle
/// downloaded yet) is always older than any candidate.
pub fn is_newer(current: Option<&str>, candidate: &str) -> bool {
    match current {
        Some(current) => candidate > current,
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_current_cycle_means_any_candidate_is_newer() {
        assert!(is_newer(None, "2026-07"));
    }

    #[test]
    fn later_cycle_id_is_newer() {
        assert!(is_newer(Some("2026-06"), "2026-07"));
        assert!(!is_newer(Some("2026-07"), "2026-06"));
        assert!(!is_newer(Some("2026-07"), "2026-07"));
    }
}
