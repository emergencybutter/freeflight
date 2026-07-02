use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

/// A single published data cycle (FAA 28-day AIRAC or 56-day chart cycle)
/// that a `cycle-*.sqlite` / `charts-*.pmtiles` bundle was built from.
///
/// See DESIGN.md §7 (Data Pipeline) — clients pin all offline data to one
/// `AiracCycle` at a time so the UI can always show "data current as of".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiracCycle {
    /// Cycle identifier as published by the FAA, e.g. "2026-07".
    pub id: String,
    /// Date the cycle becomes effective.
    pub effective_date: NaiveDate,
    /// Free-form identifier for the upstream source revision (e.g. a CIFP
    /// file hash or NASR subscription date), for provenance/debugging.
    pub source_version: String,
}
