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

/// ARINC 424 path-and-terminator leg types. Phase 1 fully supports the
/// common enroute/terminal leg types (see DESIGN.md §12, open question 3);
/// the rest are modeled so a procedure using them can be recognized and
/// flagged rather than silently mis-rendered, not so it can be flown.
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
    /// Course to an altitude.
    CA,
    /// Course to a DME distance.
    CD,
    /// Heading to an altitude.
    VA,
    /// Heading to an intercept.
    VI,
    /// Heading to a DME distance.
    VD,
    /// Heading to a manual termination.
    VM,
    /// Radius to a fix (constant-radius turn), used e.g. by RNP procedures.
    RF,
    /// Course/heading to a radial intercept.
    CI,
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
