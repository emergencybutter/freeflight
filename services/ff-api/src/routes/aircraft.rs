//! Aircraft manager endpoints (DESIGN.md §9.5.4).
//!
//! Every route here is scoped to the signed-in user by
//! [`require_account_user`]; the `user_id` then rides into every query in
//! `ff-accounts`, which is where the actual authorization lives. An id
//! belonging to someone else returns **404, not 403**, so ids cannot be
//! probed for existence.
//!
//! Validation runs twice on purpose: here, so the client gets a readable
//! message, and again as `CHECK` constraints in the schema, so a bug in
//! this file cannot put a negative fuel burn or a NaN in front of the
//! flight planner.

use crate::routes::auth::require_account_user;
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use ff_accounts::{AircraftInput, PerformanceRow, Phase};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

/// Per-user cap on aircraft, and per-table cap on rows (§9.5.9). Neither
/// is a limit a real pilot will meet — they exist so a write endpoint
/// cannot be used to fill the volume.
const MAX_AIRCRAFT_PER_USER: i64 = 50;
const MAX_PERFORMANCE_ROWS: usize = 200;

/// Longest accepted free-text field. Registrations are ~6 characters and
/// power settings ~8; this is slack, not a real constraint.
const MAX_TEXT_LEN: usize = 64;

// ---- type templates ------------------------------------------------------

/// A seeded starting point for an aircraft of a given ICAO type
/// (§9.5.3). Product data, not aeronautical data: version-controlled with
/// the code, not on the AIRAC cycle, and **not authoritative** — book
/// figures for a new airframe on a standard day, which is not the
/// aeroplane in the hangar.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AircraftTemplate {
    pub icao_type: String,
    pub name: String,
    #[serde(flatten)]
    pub scalars: TemplateScalars,
    #[serde(default)]
    pub climb: Vec<PerformanceRow>,
    #[serde(default)]
    pub cruise: Vec<PerformanceRow>,
    #[serde(default)]
    pub descent: Vec<PerformanceRow>,
}

/// The scalar half of a template, shaped to drop straight into an
/// [`AircraftInput`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TemplateScalars {
    pub cruise_tas_kt: Option<f64>,
    pub cruise_fuel_gph: Option<f64>,
    pub climb_rate_fpm: Option<f64>,
    pub climb_tas_kt: Option<f64>,
    pub climb_fuel_gph: Option<f64>,
    pub descent_rate_fpm: Option<f64>,
    pub descent_tas_kt: Option<f64>,
    pub descent_fuel_gph: Option<f64>,
    pub taxi_fuel_gal: Option<f64>,
    pub fuel_capacity_gal: Option<f64>,
    pub reserve_minutes: Option<i32>,
    pub max_gross_weight_lb: Option<f64>,
    /// Deliberately absent from every shipped template. A CG envelope is
    /// usually not the single forward/aft pair this model stores (the
    /// forward limit typically moves aft with weight), so seeding one
    /// would put a plausible-looking wrong envelope in front of a weight
    /// & balance check. Max gross weight is a single unambiguous number
    /// and is seeded; the CG limits must come from the pilot's POH.
    #[serde(default)]
    pub forward_cg_limit_in: Option<f64>,
    #[serde(default)]
    pub aft_cg_limit_in: Option<f64>,
}

/// Parsed once from the file embedded at compile time. `include_str!`
/// rather than a runtime read so the binary has no data-file dependency
/// at all — nothing to mount, nothing to go missing in the container.
fn templates() -> &'static [AircraftTemplate] {
    static TEMPLATES: OnceLock<Vec<AircraftTemplate>> = OnceLock::new();
    TEMPLATES.get_or_init(|| {
        serde_json::from_str(include_str!("../../data/aircraft_types.json"))
            .expect("aircraft_types.json is malformed")
    })
}

/// `GET /aircraft/types` — the whole catalog. Public: there is nothing
/// user-specific about it, and the client needs it before sign-in to
/// render the "add aircraft" form.
pub async fn list_types() -> Json<&'static [AircraftTemplate]> {
    Json(templates())
}

/// `GET /aircraft/types/:icao`
pub async fn get_type(Path(icao): Path<String>) -> Response {
    let icao = icao.to_uppercase();
    match templates().iter().find(|t| t.icao_type == icao) {
        Some(template) => Json(template).into_response(),
        None => (StatusCode::NOT_FOUND, "unknown type").into_response(),
    }
}

// ---- validation ----------------------------------------------------------

/// Normalizes in place and rejects anything the planner should never see.
///
/// Note the NaN check: `NaN` survives JSON round-trips in some clients,
/// compares false against every bound, and would sail through a naive
/// `> 0.0` test — then poison every downstream calculation silently.
fn validate(input: &mut AircraftInput) -> Result<(), String> {
    input.registration = input.registration.trim().to_uppercase();
    if input.registration.is_empty() {
        return Err("registration is required".into());
    }
    if input.registration.len() > MAX_TEXT_LEN {
        return Err("registration is too long".into());
    }
    for (label, field) in [
        ("serial_number", &mut input.serial_number),
        ("name", &mut input.name),
    ] {
        if let Some(value) = field {
            *value = value.trim().to_string();
            if value.len() > MAX_TEXT_LEN {
                return Err(format!("{label} is too long"));
            }
        }
    }
    for field in [&mut input.icao_type, &mut input.template_icao] {
        if let Some(value) = field {
            *value = value.trim().to_uppercase();
            if value.len() > MAX_TEXT_LEN {
                return Err("type designator is too long".into());
            }
        }
    }

    // (label, value, must be strictly positive)
    let numbers: [(&str, Option<f64>, bool); 13] = [
        ("cruise_tas_kt", input.cruise_tas_kt, true),
        ("cruise_fuel_gph", input.cruise_fuel_gph, false),
        ("climb_rate_fpm", input.climb_rate_fpm, true),
        ("climb_tas_kt", input.climb_tas_kt, true),
        ("climb_fuel_gph", input.climb_fuel_gph, false),
        ("descent_rate_fpm", input.descent_rate_fpm, true),
        ("descent_tas_kt", input.descent_tas_kt, true),
        ("descent_fuel_gph", input.descent_fuel_gph, false),
        ("taxi_fuel_gal", input.taxi_fuel_gal, false),
        ("fuel_capacity_gal", input.fuel_capacity_gal, true),
        ("max_gross_weight_lb", input.max_gross_weight_lb, true),
        ("forward_cg_limit_in", input.forward_cg_limit_in, false),
        ("aft_cg_limit_in", input.aft_cg_limit_in, false),
    ];
    for (label, value, strictly_positive) in numbers {
        let Some(value) = value else { continue };
        if !value.is_finite() {
            return Err(format!("{label} must be a real number"));
        }
        if strictly_positive && value <= 0.0 {
            return Err(format!("{label} must be greater than zero"));
        }
        if !strictly_positive && value < 0.0 {
            return Err(format!("{label} cannot be negative"));
        }
    }
    if let Some(minutes) = input.reserve_minutes {
        if minutes < 0 {
            return Err("reserve_minutes cannot be negative".into());
        }
    }
    if let (Some(forward), Some(aft)) = (input.forward_cg_limit_in, input.aft_cg_limit_in) {
        if forward >= aft {
            return Err("the forward CG limit must be ahead of the aft limit".into());
        }
    }
    Ok(())
}

/// Validates one performance table. `phase` matters: climb and descent
/// need a vertical speed (they are the rate the vertical profile flies),
/// cruise must not carry one.
fn validate_rows(phase: Phase, rows: &mut [PerformanceRow]) -> Result<(), String> {
    if rows.len() > MAX_PERFORMANCE_ROWS {
        return Err(format!(
            "a table may have at most {MAX_PERFORMANCE_ROWS} rows"
        ));
    }
    for row in rows.iter_mut() {
        row.power_setting = row.power_setting.trim().to_string();
        if row.power_setting.len() > MAX_TEXT_LEN {
            return Err("power setting is too long".into());
        }
        if !(-2000..=60000).contains(&row.pressure_altitude_ft) {
            return Err("pressure altitude is outside -2,000..60,000 ft".into());
        }
        if !row.tas_kt.is_finite() || row.tas_kt <= 0.0 {
            return Err("TAS must be greater than zero".into());
        }
        if !row.fuel_gph.is_finite() || row.fuel_gph < 0.0 {
            return Err("fuel burn cannot be negative".into());
        }
        match (phase, row.vertical_speed_fpm) {
            (Phase::Cruise, Some(_)) => {
                return Err("cruise rows have no vertical speed".into());
            }
            (Phase::Climb | Phase::Descent, None) => {
                return Err("climb and descent rows need a vertical speed".into());
            }
            (Phase::Climb | Phase::Descent, Some(fpm)) if !fpm.is_finite() || fpm <= 0.0 => {
                // Stored as a positive magnitude; the phase supplies the
                // sign, so a "negative descent rate" is a sign error, not
                // a descent.
                return Err("vertical speed must be a positive rate".into());
            }
            _ => {}
        }
    }
    // A duplicate key would be rejected by the primary key anyway, but as
    // a 500 rather than something the user can act on.
    let mut keys: Vec<(i32, &str)> = rows
        .iter()
        .map(|r| (r.pressure_altitude_ft, r.power_setting.as_str()))
        .collect();
    keys.sort_unstable();
    let before = keys.len();
    keys.dedup();
    if keys.len() != before {
        return Err("two rows share the same altitude and power setting".into());
    }
    Ok(())
}

/// A duplicate registration is the user's problem to fix (409); anything
/// else is ours (500), and gets logged rather than described to the
/// client.
fn storage_error(err: ff_accounts::AccountsError) -> Response {
    if err.is_unique_violation() {
        return (
            StatusCode::CONFLICT,
            "you already have an aircraft with that registration",
        )
            .into_response();
    }
    tracing::error!("aircraft storage error: {err}");
    (StatusCode::INTERNAL_SERVER_ERROR, "storage error").into_response()
}

// ---- handlers ------------------------------------------------------------

/// `GET /aircraft`
pub async fn list(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let (accounts, user) = match require_account_user(&state, &headers).await {
        Ok(pair) => pair,
        Err(response) => return response,
    };
    match accounts.list_aircraft(user.id).await {
        Ok(fleet) => Json(fleet).into_response(),
        Err(err) => storage_error(err),
    }
}

/// `GET /aircraft/:id`
pub async fn get(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Response {
    let (accounts, user) = match require_account_user(&state, &headers).await {
        Ok(pair) => pair,
        Err(response) => return response,
    };
    match accounts.get_aircraft(user.id, id).await {
        Ok(Some(detail)) => Json(detail).into_response(),
        Ok(None) => not_found(),
        Err(err) => storage_error(err),
    }
}

/// What `POST /aircraft` accepts: the aircraft's own fields, plus an
/// optional type to seed anything the client left unset.
#[derive(Debug, Deserialize)]
pub struct CreateRequest {
    #[serde(flatten)]
    pub aircraft: AircraftInput,
    /// Seed from this ICAO type's template. Values the client sent
    /// explicitly win; the template only fills gaps, so "pick a type,
    /// then correct two numbers" works in a single request.
    #[serde(default)]
    pub from_type: Option<String>,
}

/// `POST /aircraft`
pub async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateRequest>,
) -> Response {
    let (accounts, user) = match require_account_user(&state, &headers).await {
        Ok(pair) => pair,
        Err(response) => return response,
    };

    let mut input = request.aircraft;
    let mut seeded: Option<&AircraftTemplate> = None;
    if let Some(type_name) = &request.from_type {
        let wanted = type_name.trim().to_uppercase();
        let Some(template) = templates().iter().find(|t| t.icao_type == wanted) else {
            return (StatusCode::BAD_REQUEST, "unknown type").into_response();
        };
        apply_template(&mut input, template);
        seeded = Some(template);
    }
    if let Err(message) = validate(&mut input) {
        return (StatusCode::BAD_REQUEST, message).into_response();
    }

    match accounts.count_aircraft(user.id).await {
        Ok(count) if count >= MAX_AIRCRAFT_PER_USER => {
            return (
                StatusCode::FORBIDDEN,
                format!("at most {MAX_AIRCRAFT_PER_USER} aircraft per account"),
            )
                .into_response();
        }
        Ok(_) => {}
        Err(err) => return storage_error(err),
    }

    let created = match accounts.create_aircraft(user.id, &input).await {
        Ok(aircraft) => aircraft,
        Err(err) => return storage_error(err),
    };

    // Seed the performance tables too. A failure here leaves a valid
    // aircraft with empty tables rather than failing the create — the
    // pilot can fill them in, and reporting failure would strand a row
    // that was in fact written.
    if let Some(template) = seeded {
        for (phase, rows) in [
            (Phase::Climb, &template.climb),
            (Phase::Cruise, &template.cruise),
            (Phase::Descent, &template.descent),
        ] {
            if rows.is_empty() {
                continue;
            }
            if let Err(err) = accounts
                .replace_performance(user.id, created.id, phase, rows)
                .await
            {
                tracing::error!(
                    "seeding the {} table for aircraft {} failed: {err}",
                    phase.as_str(),
                    created.id
                );
            }
        }
    }

    match accounts.get_aircraft(user.id, created.id).await {
        Ok(Some(detail)) => (StatusCode::CREATED, Json(detail)).into_response(),
        Ok(None) => not_found(),
        Err(err) => storage_error(err),
    }
}

/// Fills only the fields the client left unset, so an explicit value
/// always beats the book figure.
fn apply_template(input: &mut AircraftInput, template: &AircraftTemplate) {
    let s = &template.scalars;
    input.icao_type.get_or_insert(template.icao_type.clone());
    input
        .template_icao
        .get_or_insert(template.icao_type.clone());
    input.cruise_tas_kt = input.cruise_tas_kt.or(s.cruise_tas_kt);
    input.cruise_fuel_gph = input.cruise_fuel_gph.or(s.cruise_fuel_gph);
    input.climb_rate_fpm = input.climb_rate_fpm.or(s.climb_rate_fpm);
    input.climb_tas_kt = input.climb_tas_kt.or(s.climb_tas_kt);
    input.climb_fuel_gph = input.climb_fuel_gph.or(s.climb_fuel_gph);
    input.descent_rate_fpm = input.descent_rate_fpm.or(s.descent_rate_fpm);
    input.descent_tas_kt = input.descent_tas_kt.or(s.descent_tas_kt);
    input.descent_fuel_gph = input.descent_fuel_gph.or(s.descent_fuel_gph);
    input.taxi_fuel_gal = input.taxi_fuel_gal.or(s.taxi_fuel_gal);
    input.fuel_capacity_gal = input.fuel_capacity_gal.or(s.fuel_capacity_gal);
    input.reserve_minutes = input.reserve_minutes.or(s.reserve_minutes);
    input.max_gross_weight_lb = input.max_gross_weight_lb.or(s.max_gross_weight_lb);
    input.forward_cg_limit_in = input.forward_cg_limit_in.or(s.forward_cg_limit_in);
    input.aft_cg_limit_in = input.aft_cg_limit_in.or(s.aft_cg_limit_in);
    // Never inherited: numbers that came out of a book are not verified,
    // whatever the request claims.
    input.verified = false;
}

/// `PUT /aircraft/:id` — replaces the writable fields.
pub async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(mut input): Json<AircraftInput>,
) -> Response {
    let (accounts, user) = match require_account_user(&state, &headers).await {
        Ok(pair) => pair,
        Err(response) => return response,
    };
    if let Err(message) = validate(&mut input) {
        return (StatusCode::BAD_REQUEST, message).into_response();
    }
    match accounts.update_aircraft(user.id, id, &input).await {
        Ok(Some(aircraft)) => Json(aircraft).into_response(),
        Ok(None) => not_found(),
        Err(err) => storage_error(err),
    }
}

/// `DELETE /aircraft/:id`
pub async fn delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Response {
    let (accounts, user) = match require_account_user(&state, &headers).await {
        Ok(pair) => pair,
        Err(response) => return response,
    };
    match accounts.delete_aircraft(user.id, id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => not_found(),
        Err(err) => storage_error(err),
    }
}

/// `PUT /aircraft/:id/performance/:phase` — replaces that phase's whole
/// table. Whole-table rather than per-row because the client edits a
/// grid: it is idempotent, retry-safe, and needs no stable row ids.
pub async fn replace_performance(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, phase)): Path<(i64, String)>,
    Json(mut rows): Json<Vec<PerformanceRow>>,
) -> Response {
    let (accounts, user) = match require_account_user(&state, &headers).await {
        Ok(pair) => pair,
        Err(response) => return response,
    };
    let Some(phase) = Phase::parse(&phase) else {
        return (
            StatusCode::BAD_REQUEST,
            "phase must be climb, cruise, or descent",
        )
            .into_response();
    };
    if let Err(message) = validate_rows(phase, &mut rows) {
        return (StatusCode::BAD_REQUEST, message).into_response();
    }
    match accounts
        .replace_performance(user.id, id, phase, &rows)
        .await
    {
        Ok(true) => match accounts.get_aircraft(user.id, id).await {
            Ok(Some(detail)) => Json(detail).into_response(),
            Ok(None) => not_found(),
            Err(err) => storage_error(err),
        },
        Ok(false) => not_found(),
        Err(err) => storage_error(err),
    }
}

/// `DELETE /auth/me` — delete the account and everything it owns
/// (§9.5.9). One statement, thanks to the cascades.
pub async fn delete_account(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let (accounts, user) = match require_account_user(&state, &headers).await {
        Ok(pair) => pair,
        Err(response) => return response,
    };
    match accounts.delete_user(user.id).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(err) => storage_error(err),
    }
}

/// Someone else's aircraft is reported exactly as a nonexistent one, so
/// ids reveal nothing (§9.5.4).
fn not_found() -> Response {
    (StatusCode::NOT_FOUND, "no such aircraft").into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shipped_template_parses_and_is_self_consistent() {
        let all = templates();
        assert!(!all.is_empty(), "the catalog is empty");
        for template in all {
            assert_eq!(
                template.icao_type,
                template.icao_type.to_uppercase(),
                "type designators are matched uppercase"
            );
            // Templates are seeds for real flight planning, so they must
            // satisfy the same rules a hand-entered aircraft does.
            let mut input = AircraftInput {
                registration: "N0TEST".into(),
                ..Default::default()
            };
            apply_template(&mut input, template);
            validate(&mut input)
                .unwrap_or_else(|e| panic!("template {} is invalid: {e}", template.icao_type));

            for (phase, rows) in [
                (Phase::Climb, &template.climb),
                (Phase::Cruise, &template.cruise),
                (Phase::Descent, &template.descent),
            ] {
                let mut rows = rows.clone();
                validate_rows(phase, &mut rows).unwrap_or_else(|e| {
                    panic!(
                        "{} {} table is invalid: {e}",
                        template.icao_type,
                        phase.as_str()
                    )
                });
            }
            assert!(
                template.scalars.forward_cg_limit_in.is_none()
                    && template.scalars.aft_cg_limit_in.is_none(),
                "{} ships a CG envelope; those must come from the pilot's POH",
                template.icao_type
            );
        }
    }

    /// The turbocharged Bonanza exists as a separate template precisely
    /// because it keeps making power where the normally-aspirated one
    /// cannot. A table that stopped at 10,000 ft, or whose TAS tailed off
    /// with altitude, would describe the wrong aeroplane — and because
    /// lookup *clamps* rather than extrapolates (§9.5.6), a short table
    /// would silently plan an FL200 leg at its 10,000 ft figures.
    ///
    /// The designator is **BT36** (ICAO Doc 8643: BEECH A36TC/B36TC,
    /// class L1P — single *piston*). It is emphatically not `B36T`, which
    /// is the Allison **turbine** conversion (L1T): same airframe family,
    /// a different powerplant, and wholly different fuel figures. These
    /// numbers are the turbocharged piston's.
    #[test]
    fn the_turbo_bonanza_still_performs_where_the_normally_aspirated_one_stops() {
        let find = |icao: &str| {
            templates()
                .iter()
                .find(|t| t.icao_type == icao)
                .unwrap_or_else(|| panic!("{icao} template"))
        };
        let turbo = find("BT36");
        let normal = find("BE36");

        let ceiling = |t: &AircraftTemplate| {
            t.cruise
                .iter()
                .map(|r| r.pressure_altitude_ft)
                .max()
                .unwrap_or(0)
        };
        assert!(
            ceiling(turbo) >= 20000,
            "the turbo's cruise table stops at {} ft — too low to plan the altitudes it is bought for",
            ceiling(turbo)
        );
        assert!(
            ceiling(turbo) > ceiling(normal),
            "the turbo table ({} ft) should reach higher than the normally-aspirated one ({} ft)",
            ceiling(turbo),
            ceiling(normal)
        );

        // TAS rises with altitude — the defining trait of forced
        // induction. Checked *within each power setting*: the table holds
        // several, and sorting across them compares 75% at sea level
        // against 65% at sea level, which proves nothing. That is the
        // same mistake `AircraftPerformance::cruise_at` refuses to make.
        let settings: std::collections::BTreeSet<&str> = turbo
            .cruise
            .iter()
            .map(|r| r.power_setting.as_str())
            .collect();
        assert!(settings.len() > 1, "expected several power settings");
        for setting in settings {
            let mut rows: Vec<&PerformanceRow> = turbo
                .cruise
                .iter()
                .filter(|r| r.power_setting == setting)
                .collect();
            rows.sort_by_key(|r| r.pressure_altitude_ft);
            for pair in rows.windows(2) {
                assert!(
                    pair[1].tas_kt > pair[0].tas_kt,
                    "at {setting}, TAS fell from {} kt at {} ft to {} kt at {} ft",
                    pair[0].tas_kt,
                    pair[0].pressure_altitude_ft,
                    pair[1].tas_kt,
                    pair[1].pressure_altitude_ft
                );
            }
        }
    }

    #[test]
    fn a_template_never_overrides_what_the_pilot_typed() {
        let template = templates()
            .iter()
            .find(|t| t.icao_type == "C172")
            .expect("C172 template");
        let mut input = AircraftInput {
            registration: "N172SP".into(),
            // This pilot's aeroplane is slower than the book.
            cruise_tas_kt: Some(101.0),
            verified: true,
            ..Default::default()
        };
        apply_template(&mut input, template);
        assert_eq!(
            input.cruise_tas_kt,
            Some(101.0),
            "the book overrode the pilot"
        );
        assert_eq!(input.climb_rate_fpm, template.scalars.climb_rate_fpm);
        assert_eq!(input.template_icao.as_deref(), Some("C172"));
        assert!(
            !input.verified,
            "template-seeded numbers must never be marked verified"
        );
    }

    #[test]
    fn validation_rejects_what_the_planner_must_not_see() {
        let base = || AircraftInput {
            registration: "N1234A".into(),
            ..Default::default()
        };

        let mut lowercase = AircraftInput {
            registration: "  n1234a ".into(),
            ..Default::default()
        };
        validate(&mut lowercase).expect("should normalize");
        assert_eq!(lowercase.registration, "N1234A");

        let mut blank = AircraftInput {
            registration: "   ".into(),
            ..Default::default()
        };
        assert!(validate(&mut blank).is_err(), "a blank registration passed");

        // NaN compares false against every bound; without an explicit
        // finiteness check it would slip past `<= 0.0` and poison the
        // planner downstream.
        let mut nan = base();
        nan.cruise_tas_kt = Some(f64::NAN);
        assert!(validate(&mut nan).is_err(), "NaN TAS passed");

        let mut infinite = base();
        infinite.climb_rate_fpm = Some(f64::INFINITY);
        assert!(
            validate(&mut infinite).is_err(),
            "infinite climb rate passed"
        );

        let mut zero_tas = base();
        zero_tas.cruise_tas_kt = Some(0.0);
        assert!(validate(&mut zero_tas).is_err(), "zero TAS passed");

        let mut negative_burn = base();
        negative_burn.cruise_fuel_gph = Some(-1.0);
        assert!(
            validate(&mut negative_burn).is_err(),
            "negative burn passed"
        );

        // Zero fuel burn is odd but not impossible (a glider, an
        // electric trainer), so it is allowed where a rate is not.
        let mut zero_burn = base();
        zero_burn.cruise_fuel_gph = Some(0.0);
        assert!(validate(&mut zero_burn).is_ok());

        let mut inverted = base();
        inverted.forward_cg_limit_in = Some(47.3);
        inverted.aft_cg_limit_in = Some(35.0);
        assert!(
            validate(&mut inverted).is_err(),
            "inverted CG envelope passed"
        );
    }

    #[test]
    fn performance_rows_must_match_their_phase() {
        let cruise_row = |vs: Option<f64>| PerformanceRow {
            pressure_altitude_ft: 6000,
            power_setting: "65%".into(),
            vertical_speed_fpm: vs,
            tas_kt: 110.0,
            fuel_gph: 7.9,
        };

        assert!(validate_rows(Phase::Cruise, &mut [cruise_row(None)]).is_ok());
        assert!(
            validate_rows(Phase::Cruise, &mut [cruise_row(Some(500.0))]).is_err(),
            "a cruise row carried a vertical speed"
        );
        assert!(
            validate_rows(Phase::Climb, &mut [cruise_row(None)]).is_err(),
            "a climb row without a rate passed"
        );
        // Descent rates are magnitudes; a negative one is a sign error.
        assert!(
            validate_rows(Phase::Descent, &mut [cruise_row(Some(-500.0))]).is_err(),
            "a negative descent rate passed"
        );
        assert!(validate_rows(Phase::Descent, &mut [cruise_row(Some(500.0))]).is_ok());

        // Duplicate keys would otherwise surface as a 500 from the
        // primary key rather than something the user can fix.
        let mut duplicates = [cruise_row(None), cruise_row(None)];
        assert!(validate_rows(Phase::Cruise, &mut duplicates).is_err());

        let mut too_many: Vec<PerformanceRow> = (0..=MAX_PERFORMANCE_ROWS)
            .map(|i| PerformanceRow {
                pressure_altitude_ft: i as i32,
                power_setting: String::new(),
                vertical_speed_fpm: None,
                tas_kt: 110.0,
                fuel_gph: 7.9,
            })
            .collect();
        assert!(validate_rows(Phase::Cruise, &mut too_many).is_err());
    }
}
