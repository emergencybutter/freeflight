use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProcedureKind {
    Sid,
    Star,
    Approach,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Procedure {
    pub id: String,
    pub airport_icao: String,
    pub kind: ProcedureKind,
    /// e.g. "SHORE2", "ILS 28L".
    pub ident: String,
    /// Runway this procedure serves, if runway-specific.
    pub runway_ident: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransitionKind {
    Enroute,
    Common,
    Approach,
    Missed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProcedureTransition {
    pub id: String,
    pub procedure_id: String,
    pub ident: String,
    pub kind: TransitionKind,
}

/// ARINC 424 path-and-terminator leg types (spec §5.21). Covers all 23
/// standard leg types except the three holding-pattern termination
/// flavors (`HA`/`HF`/`HM`), which collapse into [`Self::HoldingPattern`]
/// since freeflight doesn't yet distinguish them; `Unsupported` is a
/// fallback for any future/malformed code rather than a real leg type
/// (see DESIGN.md §12, open question 3 — now closed for coverage, not
/// for full leg-geometry rendering).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PathAndTerm {
    /// Initial fix.
    IF,
    /// Track to a fix.
    TF,
    /// Course to a fix.
    CF,
    /// Direct to a fix (from an unspecified position).
    DF,
    /// Fix to an altitude.
    FA,
    /// Track from a fix for a distance.
    FC,
    /// Track from a fix to a DME distance.
    FD,
    /// From a fix to a manual termination.
    FM,
    /// Course to an altitude.
    CA,
    /// Course to a DME distance.
    CD,
    /// Course to a radial termination.
    CR,
    /// Radius to a fix (constant-radius turn), used e.g. by RNP procedures.
    RF,
    /// Arc to a fix (DME arc).
    AF,
    /// Heading to an altitude.
    VA,
    /// Heading to a DME distance.
    VD,
    /// Heading to an intercept.
    VI,
    /// Heading to a manual termination.
    VM,
    /// Heading to a radial termination.
    VR,
    /// Course to an intercept (of the next leg's course).
    CI,
    /// 045/180 procedure turn.
    PI,
    /// Holding pattern, various termination flavors collapsed for now.
    HoldingPattern,
    /// Any leg type not yet modeled explicitly.
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TurnDirection {
    Left,
    Right,
    Either,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum AltitudeConstraint {
    At(u32),
    AtOrAbove(u32),
    AtOrBelow(u32),
    Between { lower: u32, upper: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum SpeedConstraint {
    AtOrBelow(u32),
    At(u32),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProcedureLeg {
    pub transition_id: String,
    /// 1-based position within the transition.
    pub seq: u32,
    pub path_and_term: PathAndTerm,
    /// Fix this leg terminates at, when the leg type has one.
    pub fix_ident: Option<String>,
    /// Magnetic course in degrees, when the leg type specifies one.
    pub course_deg: Option<f64>,
    pub altitude: Option<AltitudeConstraint>,
    pub speed: Option<SpeedConstraint>,
    pub turn_direction: Option<TurnDirection>,
}
