use serde::{Deserialize, Serialize};

/// Response shape of `ff-api`'s `GET /cycles/latest` (DESIGN.md §7, §8).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CycleManifest {
    pub cycle_id: String,
    pub effective_date: String,
    pub sqlite_url: String,
    pub sqlite_sha256: String,
    pub pmtiles_url: String,
    pub pmtiles_sha256: String,
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
