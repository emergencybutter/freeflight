//! Parser for **AIXM 4.5** aeronautical data — the ICAO/EUROCONTROL XML
//! exchange format published by national AIS providers. The first target
//! is France's SIA export (`AIXM4.5_all_FR_OM_*.xml`), the non-US analog
//! of `ff-cifp`/`ff-nasr` (DESIGN.md §3.1).
//!
//! # Licensing (France / SIA)
//!
//! The SIA AIXM 4.5 export is published under the French **Licence
//! Ouverte** (Etalab open licence): free, worldwide, **redistribution and
//! commercial use permitted**, with two obligations that consumers of
//! this crate's output must honor:
//! - **Attribution:** cite the source — at minimum "Service de
//!   l'Information Aéronautique (SIA)" and the data's last-update date.
//! - **No distortion:** don't alter the data in a way that changes its
//!   meaning (freeflight's existing "not for navigation" disclaimer and
//!   faithful ingestion already satisfy this).
//!
//! Unlike OpenAIP (the reverted prior choice), this is redistributable, so
//! it needs no §12 public-domain-rule exception — only attribution.
//! **Per-country note:** this licence is France's; other states' AIP terms
//! differ and must be re-checked before their AIXM is bundled.
//!
//! # AIXM 4.5 shape
//!
//! An AIXM 4.5 file is a flat `<AIXM-Snapshot>` of feature elements with
//! terse names and a typed naming convention (`code*`/`txt*`/`geo*`/
//! `val*`/`uom*`). Each feature carries a nested `<XxxUid>` identity block,
//! plus *referenced* Uid blocks (e.g. `<OrgUid>`) the parser must not
//! confuse for the feature's own fields (see [`parser`]). France's export
//! is UTF-8; other states' may be ISO-8859-1 (handled — see `Cargo.toml`).
//! This is structurally much simpler than AIXM 5.1 (no GML geometry, no
//! timeslices, no xlink) — deliberately why France-4.5 was chosen over the
//! frozen France-5.1 demo (DESIGN.md §3.1).
//!
//! Features handled today:
//! - `Ahp` (AirportHeliport) → [`ff_core::Airport`]
//! - `Vor`/`Ndb`/`Dme`/`Tcn` → [`ff_core::Navaid`]
//! - `Dpn` (DesignatedPoint) → [`ff_core::Waypoint`]
//! - `Rwy` + `Rdn` (runway + directions) → [`ff_core::Runway`], joined
//!   across the two feature types (see [`runway`])
//! - `Rte` + `Rsg` (route + segments) → [`ff_core::Airway`] +
//!   [`ff_core::AirwayLeg`], with segment order reconstructed by chaining
//!   (see [`airway`])
//! - `Ase` + `Abd` (airspace + border) → [`ff_core::AirspaceVolume`], with
//!   arc/circle borders expanded to point rings (see [`airspace`])
//!
//! Deferred:
//! - VOR/DME co-location merge (a `Vor` + linked `Dme` → a single
//!   `VorDme`); today each maps to its own base type.
//!
//! # Verification status
//!
//! Validated against the **real SIA export** `AIXM4.5_all_FR_OM_2026-07-09`
//! (AIRAC 07/26): the full 43 MB file parses cleanly — 878 airports, 394
//! navaids, 4285 waypoints, 779 runways (each matching the raw element
//! count), 425 airways / 1895 legs (from 429 `Rte`; 4 idents published
//! twice across regions collapsed), and 1541 airspaces (of ~3765 with a
//! mappable class — the rest are single-*point* "activity" zones with no
//! polygon border, correctly skipped). `tests/real_sia.rs` is a golden
//! test over verbatim records (the `ff-nasr`/`tests/real_nasr.rs` analog),
//! covering the LFRC 10/28 Rwy+Rdn join, the L615 Rte+Rsg chain, and the
//! LFR92 Ase+Abd polygon. Confirmed against real data: DMS `geo*` encoding
//! ([`coord`]), `valFreq`/`uomFreq` units, `codeComposition` surfaces, FL
//! limits, arc/circle borders, the polymorphic segment endpoints, the
//! airspace `mid` join key ([`airspace`]), and the `<OrgUid>`
//! name-shadowing trap ([`parser`]). See `examples/parse_file.rs` to run
//! it over any export.
//!
//! Remaining `// VERIFY:` items are refinements, not blockers: the full
//! `Ahp` `codeType` enum (only `HP`→heliport is special-cased today), and
//! that a single-`region` stamp is a **simplification** — the SIA `FR_OM`
//! file spans several ICAO regions (metropolitan `LF`, New Caledonia,
//! Antilles, …), so a per-feature region is a follow-up.

mod airspace;
mod airway;
pub mod convert;
pub mod coord;
pub mod parser;
mod runway;

pub use convert::{airport_from_fields, navaid_from_fields, waypoint_from_fields};
pub use coord::{parse_lat, parse_lon};
pub use parser::{parse_snapshot, AixmData, AixmError};
