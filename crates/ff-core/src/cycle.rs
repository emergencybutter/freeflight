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

/// Where a cycle sits relative to a given day.
///
/// The FAA publishes a cycle ahead of the day it takes effect, so a
/// client can pre-load it — normal and useful. What is not useful is a
/// client presenting a cycle it has pre-loaded as though it were the one
/// in force: the map says "Effective 2026-10-01" whether that is next
/// month or last month, and a pilot reading procedures two weeks early is
/// reading procedures that are not yet legal.
///
/// DESIGN.md §11 asks that data freshness be explicit and never silent.
/// Showing a date satisfies that only if the reader knows how the date
/// relates to today, which is what this answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CycleCurrency {
    /// Published but not yet in force — pre-loaded ahead of time.
    NotYetEffective,
    /// In force today.
    Current,
    /// Superseded: a newer cycle took effect after this one.
    Expired,
}

/// Classify `effective_date` against `today`, both as `YYYY-MM-DD`.
///
/// A cycle stays current for the 28 days of its AIRAC period. Returns
/// `None` when either date is unparseable rather than guessing, because
/// every wrong answer here is a wrong claim about how current a pilot's
/// data is.
pub fn cycle_currency(effective_date: &str, today: &str) -> Option<CycleCurrency> {
    let effective = NaiveDate::parse_from_str(effective_date.trim(), "%Y-%m-%d").ok()?;
    let today = NaiveDate::parse_from_str(today.trim(), "%Y-%m-%d").ok()?;
    let days = (today - effective).num_days();
    Some(if days < 0 {
        CycleCurrency::NotYetEffective
    } else if days < AIRAC_PERIOD_DAYS {
        CycleCurrency::Current
    } else {
        CycleCurrency::Expired
    })
}

/// The ICAO AIRAC period. A cycle is the current one for exactly this
/// long before the next takes its place.
pub const AIRAC_PERIOD_DAYS: i64 = 28;

/// Whole days until `effective_date`, or `None` if it is not in the
/// future. For telling a pilot *how far* ahead a pre-loaded cycle is.
pub fn days_until_effective(effective_date: &str, today: &str) -> Option<i64> {
    let effective = NaiveDate::parse_from_str(effective_date.trim(), "%Y-%m-%d").ok()?;
    let today = NaiveDate::parse_from_str(today.trim(), "%Y-%m-%d").ok()?;
    let days = (effective - today).num_days();
    (days > 0).then_some(days)
}

#[cfg(test)]
mod currency_tests {
    use super::*;

    #[test]
    fn a_cycle_published_ahead_of_its_date_is_not_yet_effective() {
        // Exactly the situation this was written for: 2026-10-01 was
        // published and serving while the day was 2026-09-17.
        assert_eq!(
            cycle_currency("2026-10-01", "2026-09-17"),
            Some(CycleCurrency::NotYetEffective)
        );
        assert_eq!(days_until_effective("2026-10-01", "2026-09-17"), Some(14));
    }

    #[test]
    fn a_cycle_is_current_on_its_effective_day_and_through_its_period() {
        assert_eq!(
            cycle_currency("2026-09-03", "2026-09-03"),
            Some(CycleCurrency::Current)
        );
        assert_eq!(
            cycle_currency("2026-09-03", "2026-09-30"),
            Some(CycleCurrency::Current)
        );
    }

    #[test]
    fn a_cycle_expires_when_the_next_one_takes_effect() {
        // Day 28 is the next cycle's effective date, so this one is done.
        assert_eq!(
            cycle_currency("2026-09-03", "2026-10-01"),
            Some(CycleCurrency::Expired)
        );
    }

    #[test]
    fn there_is_no_gap_between_current_and_expired() {
        assert_eq!(
            cycle_currency("2026-09-03", "2026-09-30"),
            Some(CycleCurrency::Current)
        );
        assert_eq!(
            cycle_currency("2026-09-03", "2026-10-01"),
            Some(CycleCurrency::Expired)
        );
    }

    #[test]
    fn days_until_is_none_once_the_cycle_is_in_force() {
        assert_eq!(days_until_effective("2026-09-03", "2026-09-03"), None);
        assert_eq!(days_until_effective("2026-09-03", "2026-09-10"), None);
    }

    #[test]
    fn an_unparseable_date_is_not_guessed_at() {
        assert_eq!(cycle_currency("", "2026-09-17"), None);
        assert_eq!(cycle_currency("2026-10-01", "not a date"), None);
        assert_eq!(days_until_effective("nonsense", "2026-09-17"), None);
    }
}
