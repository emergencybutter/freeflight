//! How charts and airports are *named and ordered* for a pilot — the one
//! copy both clients read.
//!
//! None of this is data the bundle carries. It is a set of product
//! decisions: that `TerminalAreaChart` reads "TAC", that sectionals come
//! before IFR in a selector, that an en-route zoom shows only airports
//! with instrument procedures. They were duplicated in
//! `apps/web/src/MapView.tsx` and again in Kotlin, and duplicated
//! decisions drift: the two clients disagreed about which chart kinds
//! existed before this was consolidated, and the Android chart list spent
//! a cycle naming IFR sheets after their FAA file stem while web never
//! showed sheet names at all.
//!
//! Reaching each client differently, because their costs differ:
//!
//! - **Android** calls these through `ff-uniffi` at runtime. The core is
//!   already loaded and the calls are string lookups, so this is free.
//! - **Web** gets a *generated* TypeScript module (`vocabulary::to_typescript`,
//!   emitted by the `gen-web-vocabulary` bin into
//!   `apps/web/src/chartVocabulary.ts`). The map path does not load wasm
//!   today — `MapView.tsx` imports only types from it — and making the
//!   layer menu await a wasm init to render a label would be a poor trade
//!   on the app's most important screen. A checked-in generated file
//!   costs nothing at runtime, and `generated_typescript_is_current`
//!   fails the build if someone edits this file without regenerating it.

use std::fmt::Write as _;

/// Short label for a `chart_catalog.kind`, as a pilot reads it.
///
/// Terse on purpose: these sit in a control on top of a chart. Anything
/// unrecognised falls back to the raw string rather than vanishing — a
/// kind added by a future `ff-etl` must stay selectable by a client that
/// predates it.
pub fn chart_kind_label(kind: &str) -> &str {
    match kind {
        "Sectional" => "Sectional",
        "TerminalAreaChart" => "TAC",
        "VfrFlyway" => "Flyway",
        "HelicopterRoute" => "Heli",
        "IfrEnrouteLow" => "IFR Low",
        "IfrEnrouteHigh" => "IFR High",
        "WorldAeronauticalChart" => "WAC",
        other => other,
    }
}

/// Where a kind sits in a chart selector: VFR broad → terminal, then IFR,
/// then the specialty charts. Unknown kinds sort last.
pub fn chart_kind_order(kind: &str) -> u32 {
    match kind {
        "Sectional" => 0,
        "TerminalAreaChart" => 1,
        "VfrFlyway" => 2,
        "IfrEnrouteLow" => 3,
        "IfrEnrouteHigh" => 4,
        "HelicopterRoute" => 5,
        "WorldAeronauticalChart" => 6,
        _ => 99,
    }
}

/// The kind a fresh install draws when it has one.
pub const DEFAULT_CHART_KIND: &str = "Sectional";

/// Every kind this table knows, in selector order. Used to generate the
/// web module and to keep the two lookups above in step.
const KNOWN_KINDS: &[&str] = &[
    "Sectional",
    "TerminalAreaChart",
    "VfrFlyway",
    "IfrEnrouteLow",
    "IfrEnrouteHigh",
    "HelicopterRoute",
    "WorldAeronauticalChart",
];

// ---- individual sheets ---------------------------------------------------

/// A parsed IFR enroute panel: series letter, panel number, and which
/// part of a multi-part panel this is.
struct IfrSheet<'a> {
    series: char,
    number: u32,
    half: Option<char>,
    inset: Option<&'a str>,
}

/// Parses the slug an `ff-etl` chart id ends with — `enr_l01`, `enr_l06n`,
/// `enr_l23_wilm_inset` — or `None` when it isn't an IFR enroute sheet.
fn parse_ifr_slug(slug: &str) -> Option<IfrSheet<'_>> {
    let rest = slug.strip_prefix("enr_")?;
    let mut chars = rest.chars();
    let series = chars.next()?;
    if series != 'l' && series != 'h' {
        return None;
    }
    let rest = &rest[1..];

    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return None;
    }
    let number = digits.parse().ok()?;
    let tail = &rest[digits.len()..];

    let (half, tail) = match tail.chars().next() {
        Some(c @ ('n' | 's')) => (Some(c), &tail[1..]),
        _ => (None, tail),
    };

    let inset = if tail.is_empty() {
        None
    } else {
        Some(tail.strip_prefix('_')?.strip_suffix("_inset")?)
    };

    Some(IfrSheet {
        series,
        number,
        half,
        inset,
    })
}

/// FAA abbreviations for the cities whose insets the IFR series carries.
fn inset_place(token: &str) -> String {
    match token {
        "wilm" => "Wilmington".to_string(),
        "bost" => "Boston".to_string(),
        other => {
            let mut c = other.chars();
            match c.next() {
                Some(first) => first.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        }
    }
}

/// What one chart sheet is called in a list a pilot chooses from.
///
/// `catalogue_name` is used as-is for every kind but the IFR enroute
/// series, which the catalogue names after the FAA's own file stem
/// (`IFR Low Altitude Enroute enr_l06n`) — no use to anybody. Those
/// render as the designator a pilot actually says: `L-6 North`,
/// `L-23 inset (Wilmington)`, `H-12`.
///
/// Derived from the *slug*, not the name, because the name was not even
/// unique before `ff-etl` was fixed: a panel shipping several parts gave
/// every part the panel's name, so two different downloads appeared under
/// one label. The slug was always right, including in cycles already on a
/// device.
pub fn chart_sheet_label(slug: &str, catalogue_name: &str) -> String {
    let Some(sheet) = parse_ifr_slug(slug) else {
        return catalogue_name.to_string();
    };
    let mut out = format!("{}-{}", sheet.series.to_ascii_uppercase(), sheet.number);
    match sheet.half {
        Some('n') => out.push_str(" North"),
        Some('s') => out.push_str(" South"),
        _ => {}
    }
    if let Some(token) = sheet.inset {
        let _ = write!(out, " inset ({})", inset_place(token));
    }
    out
}

/// Sort key for sheets within one kind: IFR by panel number so L-2
/// precedes L-10, everything else by name. Zero-padded so a single string
/// comparison orders both cases — a kind is either all IFR or none of it.
pub fn chart_sheet_sort_key(slug: &str, catalogue_name: &str) -> String {
    let Some(sheet) = parse_ifr_slug(slug) else {
        return catalogue_name.to_lowercase();
    };
    let mut key = format!("{:03}", sheet.number);
    if let Some(half) = sheet.half {
        key.push(half);
    }
    if let Some(inset) = sheet.inset {
        key.push_str(inset);
    }
    key
}

/// The slug part of an `ff-etl` chart id (`2026-10-01-enr_l06n` →
/// `enr_l06n`). Ids are `<cycle date>-<slug>`.
pub fn chart_slug(chart_id: &str) -> &str {
    let mut parts = chart_id.splitn(4, '-');
    match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(y), Some(m), Some(d), Some(slug))
            if y.len() == 4
                && m.len() == 2
                && d.len() == 2
                && y.bytes().all(|b| b.is_ascii_digit())
                && m.bytes().all(|b| b.is_ascii_digit())
                && d.bytes().all(|b| b.is_ascii_digit()) =>
        {
            slug
        }
        _ => chart_id,
    }
}

// ---- airport display -----------------------------------------------------
//
// Deliberately NOT here: the zoom thresholds at which airport markers
// appear. Both clients graduate the same way — nothing, then only the
// significant airports, then all of them — but the numbers and the
// definition of "significant" differ for reasons that are real rather
// than accidental, and pretending otherwise would be worse than the
// duplication.
//
// Web starts at zoom 7 and calls an airport significant when it has a
// current METAR. It streams from `ff-api` with no per-view cap, so a
// lower floor means a continent-sized bbox query, and it is always online
// so live weather is a fair proxy.
//
// Android starts at zoom 5 and uses "has instrument procedures". It reads
// a local bundle behind a 400-row cap, so a wide view is bounded; and it
// is the client that must work with the radios off, where a METAR-gated
// tier would be empty exactly when it matters.
//
// See `AIRPORT_MIN_ZOOM` in apps/web/src/MapView.tsx and
// `AIRPORT_PROCEDURES_ZOOM` in ChartMap.kt. If they ever should converge,
// that is a product decision to make once, here.

// ---- web codegen ---------------------------------------------------------

/// The TypeScript module web reads, generated from everything above.
pub fn to_typescript() -> String {
    let mut out = String::new();
    out.push_str(
        "// GENERATED — do not edit.\n\
         //\n\
         // Source of truth: crates/ff-charts/src/vocabulary.rs\n\
         // Regenerate:     cargo run -p ff-core --bin gen-web-vocabulary\n\
         //\n\
         // Chart naming and airport display thresholds live in Rust so the web\n\
         // and Android clients cannot drift apart on them. Generated rather than\n\
         // called through wasm because the map path loads no wasm today, and a\n\
         // label is not worth an async init on that screen.\n\n",
    );

    out.push_str("export const CHART_KIND_LABELS: Record<string, string> = {\n");
    for kind in KNOWN_KINDS {
        let _ = writeln!(out, "  {}: {:?},", kind, chart_kind_label(kind));
    }
    out.push_str("};\n\n");

    out.push_str("export const CHART_KIND_ORDER: Record<string, number> = {\n");
    for kind in KNOWN_KINDS {
        let _ = writeln!(out, "  {}: {},", kind, chart_kind_order(kind));
    }
    out.push_str("};\n\n");

    let _ = writeln!(
        out,
        "export const DEFAULT_CHART_KIND = {DEFAULT_CHART_KIND:?};"
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_kinds_get_the_short_labels_both_clients_show() {
        assert_eq!(chart_kind_label("TerminalAreaChart"), "TAC");
        assert_eq!(chart_kind_label("IfrEnrouteLow"), "IFR Low");
        assert_eq!(chart_kind_label("Sectional"), "Sectional");
    }

    #[test]
    fn an_unknown_kind_keeps_its_raw_name_and_sorts_last() {
        assert_eq!(chart_kind_label("GrandCanyonVfr"), "GrandCanyonVfr");
        assert_eq!(chart_kind_order("GrandCanyonVfr"), 99);
    }

    #[test]
    fn kinds_order_vfr_broad_to_terminal_then_ifr_then_specialty() {
        let mut kinds = vec![
            "HelicopterRoute",
            "IfrEnrouteHigh",
            "TerminalAreaChart",
            "Sectional",
            "IfrEnrouteLow",
            "VfrFlyway",
        ];
        kinds.sort_by_key(|k| chart_kind_order(k));
        assert_eq!(
            kinds,
            [
                "Sectional",
                "TerminalAreaChart",
                "VfrFlyway",
                "IfrEnrouteLow",
                "IfrEnrouteHigh",
                "HelicopterRoute"
            ]
        );
    }

    #[test]
    fn the_default_kind_is_one_the_table_can_label() {
        assert_eq!(chart_kind_label(DEFAULT_CHART_KIND), "Sectional");
    }

    #[test]
    fn an_ifr_panel_is_named_by_its_faa_designator() {
        assert_eq!(chart_sheet_label("enr_l01", "IFR Low ... enr_l01"), "L-1");
        assert_eq!(chart_sheet_label("enr_h12", "IFR High ... enr_h12"), "H-12");
    }

    #[test]
    fn the_parts_of_a_split_panel_are_told_apart() {
        // Both of these carried one identical catalogue name, so the list
        // showed a single label for two different downloads.
        assert_eq!(chart_sheet_label("enr_l06n", "same name"), "L-6 North");
        assert_eq!(chart_sheet_label("enr_l06s", "same name"), "L-6 South");
        assert_eq!(chart_sheet_label("enr_l34", "same name"), "L-34");
        assert_eq!(
            chart_sheet_label("enr_l34_bost_inset", "same name"),
            "L-34 inset (Boston)"
        );
        assert_eq!(
            chart_sheet_label("enr_l23_wilm_inset", "same name"),
            "L-23 inset (Wilmington)"
        );
    }

    #[test]
    fn an_unrecognised_inset_abbreviation_is_still_distinguishable() {
        assert_eq!(
            chart_sheet_label("enr_l09_xyz_inset", "n"),
            "L-9 inset (Xyz)"
        );
    }

    #[test]
    fn every_other_kind_keeps_the_catalogue_name() {
        assert_eq!(
            chart_sheet_label("albuquerque", "Albuquerque Sectional"),
            "Albuquerque Sectional"
        );
        assert_eq!(chart_sheet_label("boston-tac", "Boston TAC"), "Boston TAC");
    }

    #[test]
    fn ifr_sheets_order_by_panel_number_not_as_text() {
        let mut slugs = vec!["enr_l10", "enr_l2", "enr_l1", "enr_l21"];
        slugs.sort_by_key(|s| chart_sheet_sort_key(s, s));
        let labels: Vec<_> = slugs.iter().map(|s| chart_sheet_label(s, s)).collect();
        assert_eq!(labels, ["L-1", "L-2", "L-10", "L-21"]);
    }

    #[test]
    fn a_panels_parts_sort_with_the_panel() {
        let mut slugs = vec!["enr_l07", "enr_l06s", "enr_l06", "enr_l06n"];
        slugs.sort_by_key(|s| chart_sheet_sort_key(s, s));
        let labels: Vec<_> = slugs.iter().map(|s| chart_sheet_label(s, s)).collect();
        assert_eq!(labels, ["L-6", "L-6 North", "L-6 South", "L-7"]);
    }

    #[test]
    fn a_chart_id_yields_its_slug() {
        assert_eq!(chart_slug("2026-10-01-enr_l06n"), "enr_l06n");
        assert_eq!(chart_slug("2026-08-06-albuquerque"), "albuquerque");
        // Not an id of that shape — handed back untouched rather than
        // silently mangled.
        assert_eq!(chart_slug("enr_l06n"), "enr_l06n");
    }

    /// The web module is checked in, so it can go stale the moment anyone
    /// edits this file. This is what stops that being discovered by a
    /// pilot instead of by CI.
    #[test]
    fn generated_typescript_is_current() {
        let checked_in = include_str!("../../../apps/web/src/chartVocabulary.ts");
        assert_eq!(
            checked_in.replace("\r\n", "\n"),
            to_typescript(),
            "apps/web/src/chartVocabulary.ts is out of date — \
             run `cargo run -p ff-core --bin gen-web-vocabulary`"
        );
    }
}
