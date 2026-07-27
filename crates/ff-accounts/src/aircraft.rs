//! Aircraft records and their per-phase performance tables
//! (DESIGN.md §9.5).
//!
//! Every query here is scoped by `user_id`. That is not a convenience —
//! it is the authorization boundary, and it lives in the SQL rather than
//! in a check the caller might forget. A row belonging to someone else is
//! indistinguishable from one that does not exist, which is also what the
//! HTTP layer reports (404, never 403, so ids are not enumerable).

use crate::{Accounts, AccountsError};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Phase of flight a performance table describes. Serialized lowercase to
/// match the `CHECK (phase IN ('climb','cruise','descent'))` constraint
/// and the URL segment in `PUT /aircraft/:id/performance/:phase`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Climb,
    Cruise,
    Descent,
}

impl Phase {
    pub fn as_str(self) -> &'static str {
        match self {
            Phase::Climb => "climb",
            Phase::Cruise => "cruise",
            Phase::Descent => "descent",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "climb" => Some(Phase::Climb),
            "cruise" => Some(Phase::Cruise),
            "descent" => Some(Phase::Descent),
            _ => None,
        }
    }
}

/// One aircraft's identity and scalar performance. The performance
/// *tables* are fetched separately (see [`AircraftDetail`]) so listing a
/// fleet does not drag every row along with it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, sqlx::FromRow)]
pub struct Aircraft {
    pub id: i64,
    pub registration: String,
    pub serial_number: Option<String>,
    pub icao_type: Option<String>,
    pub name: Option<String>,
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
    pub forward_cg_limit_in: Option<f64>,
    pub aft_cg_limit_in: Option<f64>,
    /// Which type template seeded this record, if any.
    pub template_icao: Option<String>,
    /// Null until the pilot confirms the numbers against their own POH.
    /// Surfaced wherever this feeds a plan (§9.5.3).
    pub verified_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// One row of a performance table. The phase is not repeated here — it is
/// the table the row belongs to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, sqlx::FromRow)]
pub struct PerformanceRow {
    pub pressure_altitude_ft: i32,
    /// Free text (`"65%"`, `"2400 RPM"`); empty for climb/descent, which
    /// have no power dimension.
    #[serde(default)]
    pub power_setting: String,
    /// Positive magnitude; the phase supplies the sign. None for cruise.
    pub vertical_speed_fpm: Option<f64>,
    pub tas_kt: f64,
    pub fuel_gph: f64,
}

/// An aircraft plus all three of its performance tables, each sorted by
/// pressure altitude.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AircraftDetail {
    #[serde(flatten)]
    pub aircraft: Aircraft,
    pub climb: Vec<PerformanceRow>,
    pub cruise: Vec<PerformanceRow>,
    pub descent: Vec<PerformanceRow>,
}

/// The writable fields of an aircraft. Used for both create and replace:
/// the UI edits a whole form, so a full replace avoids PATCH's
/// "absent vs. explicitly null" ambiguity entirely.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AircraftInput {
    pub registration: String,
    #[serde(default)]
    pub serial_number: Option<String>,
    #[serde(default)]
    pub icao_type: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub cruise_tas_kt: Option<f64>,
    #[serde(default)]
    pub cruise_fuel_gph: Option<f64>,
    #[serde(default)]
    pub climb_rate_fpm: Option<f64>,
    #[serde(default)]
    pub climb_tas_kt: Option<f64>,
    #[serde(default)]
    pub climb_fuel_gph: Option<f64>,
    #[serde(default)]
    pub descent_rate_fpm: Option<f64>,
    #[serde(default)]
    pub descent_tas_kt: Option<f64>,
    #[serde(default)]
    pub descent_fuel_gph: Option<f64>,
    #[serde(default)]
    pub taxi_fuel_gal: Option<f64>,
    #[serde(default)]
    pub fuel_capacity_gal: Option<f64>,
    #[serde(default)]
    pub reserve_minutes: Option<i32>,
    #[serde(default)]
    pub max_gross_weight_lb: Option<f64>,
    #[serde(default)]
    pub forward_cg_limit_in: Option<f64>,
    #[serde(default)]
    pub aft_cg_limit_in: Option<f64>,
    #[serde(default)]
    pub template_icao: Option<String>,
    /// True once the pilot has confirmed these numbers against their POH.
    /// Set on create when they typed everything in themselves; set later
    /// via the "verified" button after starting from a template.
    #[serde(default)]
    pub verified: bool,
}

/// Everything an aircraft row can be selected as, in `Aircraft`'s field
/// order — kept in one place so the list/get/create/update queries cannot
/// drift apart.
const AIRCRAFT_COLUMNS: &str = "id, registration, serial_number, icao_type, name,
     cruise_tas_kt, cruise_fuel_gph,
     climb_rate_fpm, climb_tas_kt, climb_fuel_gph,
     descent_rate_fpm, descent_tas_kt, descent_fuel_gph,
     taxi_fuel_gal, fuel_capacity_gal, reserve_minutes,
     max_gross_weight_lb, forward_cg_limit_in, aft_cg_limit_in,
     template_icao, verified_at, created_at, updated_at";

impl Accounts {
    /// How many aircraft this user already has, for the per-user cap
    /// (§9.5.9) — a write endpoint must not be usable to fill the disk.
    pub async fn count_aircraft(&self, user_id: i64) -> Result<i64, AccountsError> {
        let count = sqlx::query_scalar("SELECT count(*) FROM aircraft WHERE user_id = $1")
            .bind(user_id)
            .fetch_one(&self.pool)
            .await?;
        Ok(count)
    }

    /// The user's fleet, scalars only, newest registration order.
    pub async fn list_aircraft(&self, user_id: i64) -> Result<Vec<Aircraft>, AccountsError> {
        let rows = sqlx::query_as::<_, Aircraft>(&format!(
            "SELECT {AIRCRAFT_COLUMNS} FROM aircraft WHERE user_id = $1 ORDER BY registration"
        ))
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// One aircraft with all three performance tables, or `None` if it
    /// does not exist *or* belongs to someone else — deliberately the
    /// same answer.
    pub async fn get_aircraft(
        &self,
        user_id: i64,
        aircraft_id: i64,
    ) -> Result<Option<AircraftDetail>, AccountsError> {
        let Some(aircraft) = sqlx::query_as::<_, Aircraft>(&format!(
            "SELECT {AIRCRAFT_COLUMNS} FROM aircraft WHERE user_id = $1 AND id = $2"
        ))
        .bind(user_id)
        .bind(aircraft_id)
        .fetch_optional(&self.pool)
        .await?
        else {
            return Ok(None);
        };

        let mut detail = AircraftDetail {
            aircraft,
            climb: Vec::new(),
            cruise: Vec::new(),
            descent: Vec::new(),
        };
        for phase in [Phase::Climb, Phase::Cruise, Phase::Descent] {
            let rows = self.performance_rows(aircraft_id, phase).await?;
            match phase {
                Phase::Climb => detail.climb = rows,
                Phase::Cruise => detail.cruise = rows,
                Phase::Descent => detail.descent = rows,
            }
        }
        Ok(Some(detail))
    }

    async fn performance_rows(
        &self,
        aircraft_id: i64,
        phase: Phase,
    ) -> Result<Vec<PerformanceRow>, AccountsError> {
        let rows = sqlx::query_as::<_, PerformanceRow>(
            "SELECT pressure_altitude_ft, power_setting, vertical_speed_fpm, tas_kt, fuel_gph
             FROM aircraft_performance
             WHERE aircraft_id = $1 AND phase = $2
             ORDER BY pressure_altitude_ft, power_setting",
        )
        .bind(aircraft_id)
        .bind(phase.as_str())
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// Create an aircraft owned by `user_id`. The unique index on
    /// `(user_id, registration)` is what rejects a duplicate — checking
    /// first would race.
    pub async fn create_aircraft(
        &self,
        user_id: i64,
        input: &AircraftInput,
    ) -> Result<Aircraft, AccountsError> {
        let aircraft = sqlx::query_as::<_, Aircraft>(&format!(
            "INSERT INTO aircraft (
                 user_id, registration, serial_number, icao_type, name,
                 cruise_tas_kt, cruise_fuel_gph,
                 climb_rate_fpm, climb_tas_kt, climb_fuel_gph,
                 descent_rate_fpm, descent_tas_kt, descent_fuel_gph,
                 taxi_fuel_gal, fuel_capacity_gal, reserve_minutes,
                 max_gross_weight_lb, forward_cg_limit_in, aft_cg_limit_in,
                 template_icao, verified_at)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,
                     CASE WHEN $21 THEN now() ELSE NULL END)
             RETURNING {AIRCRAFT_COLUMNS}"
        ))
        .bind(user_id)
        .bind(&input.registration)
        .bind(&input.serial_number)
        .bind(&input.icao_type)
        .bind(&input.name)
        .bind(input.cruise_tas_kt)
        .bind(input.cruise_fuel_gph)
        .bind(input.climb_rate_fpm)
        .bind(input.climb_tas_kt)
        .bind(input.climb_fuel_gph)
        .bind(input.descent_rate_fpm)
        .bind(input.descent_tas_kt)
        .bind(input.descent_fuel_gph)
        .bind(input.taxi_fuel_gal)
        .bind(input.fuel_capacity_gal)
        .bind(input.reserve_minutes)
        .bind(input.max_gross_weight_lb)
        .bind(input.forward_cg_limit_in)
        .bind(input.aft_cg_limit_in)
        .bind(&input.template_icao)
        .bind(input.verified)
        .fetch_one(&self.pool)
        .await?;
        Ok(aircraft)
    }

    /// Replace an aircraft's writable fields. `None` if it is not theirs.
    ///
    /// `verified_at` is only *cleared* or *set*, never advanced:
    /// re-confirming an already-verified aircraft keeps the original
    /// timestamp, so "verified on" means what it says.
    pub async fn update_aircraft(
        &self,
        user_id: i64,
        aircraft_id: i64,
        input: &AircraftInput,
    ) -> Result<Option<Aircraft>, AccountsError> {
        let aircraft = sqlx::query_as::<_, Aircraft>(&format!(
            "UPDATE aircraft SET
                 registration = $3, serial_number = $4, icao_type = $5, name = $6,
                 cruise_tas_kt = $7, cruise_fuel_gph = $8,
                 climb_rate_fpm = $9, climb_tas_kt = $10, climb_fuel_gph = $11,
                 descent_rate_fpm = $12, descent_tas_kt = $13, descent_fuel_gph = $14,
                 taxi_fuel_gal = $15, fuel_capacity_gal = $16, reserve_minutes = $17,
                 max_gross_weight_lb = $18, forward_cg_limit_in = $19, aft_cg_limit_in = $20,
                 template_icao = $21,
                 verified_at = CASE WHEN $22 THEN COALESCE(verified_at, now()) ELSE NULL END,
                 updated_at = now()
             WHERE user_id = $1 AND id = $2
             RETURNING {AIRCRAFT_COLUMNS}"
        ))
        .bind(user_id)
        .bind(aircraft_id)
        .bind(&input.registration)
        .bind(&input.serial_number)
        .bind(&input.icao_type)
        .bind(&input.name)
        .bind(input.cruise_tas_kt)
        .bind(input.cruise_fuel_gph)
        .bind(input.climb_rate_fpm)
        .bind(input.climb_tas_kt)
        .bind(input.climb_fuel_gph)
        .bind(input.descent_rate_fpm)
        .bind(input.descent_tas_kt)
        .bind(input.descent_fuel_gph)
        .bind(input.taxi_fuel_gal)
        .bind(input.fuel_capacity_gal)
        .bind(input.reserve_minutes)
        .bind(input.max_gross_weight_lb)
        .bind(input.forward_cg_limit_in)
        .bind(input.aft_cg_limit_in)
        .bind(&input.template_icao)
        .bind(input.verified)
        .fetch_optional(&self.pool)
        .await?;
        Ok(aircraft)
    }

    /// Delete an aircraft and its performance rows (cascade). False if it
    /// was not theirs to delete.
    pub async fn delete_aircraft(
        &self,
        user_id: i64,
        aircraft_id: i64,
    ) -> Result<bool, AccountsError> {
        let result = sqlx::query("DELETE FROM aircraft WHERE user_id = $1 AND id = $2")
            .bind(user_id)
            .bind(aircraft_id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Replace one phase's entire performance table in a single
    /// transaction, so a failed write leaves the previous table intact
    /// rather than a half-applied one. False if the aircraft is not
    /// theirs.
    pub async fn replace_performance(
        &self,
        user_id: i64,
        aircraft_id: i64,
        phase: Phase,
        rows: &[PerformanceRow],
    ) -> Result<bool, AccountsError> {
        let mut tx = self.pool.begin().await?;

        // Ownership check inside the transaction, so it cannot be
        // deleted out from under the insert.
        let owned: Option<i64> =
            sqlx::query_scalar("SELECT id FROM aircraft WHERE user_id = $1 AND id = $2 FOR UPDATE")
                .bind(user_id)
                .bind(aircraft_id)
                .fetch_optional(&mut *tx)
                .await?;
        if owned.is_none() {
            return Ok(false);
        }

        sqlx::query("DELETE FROM aircraft_performance WHERE aircraft_id = $1 AND phase = $2")
            .bind(aircraft_id)
            .bind(phase.as_str())
            .execute(&mut *tx)
            .await?;

        for row in rows {
            sqlx::query(
                "INSERT INTO aircraft_performance
                     (aircraft_id, phase, pressure_altitude_ft, power_setting,
                      vertical_speed_fpm, tas_kt, fuel_gph)
                 VALUES ($1, $2, $3, $4, $5, $6, $7)",
            )
            .bind(aircraft_id)
            .bind(phase.as_str())
            .bind(row.pressure_altitude_ft)
            .bind(&row.power_setting)
            .bind(row.vertical_speed_fpm)
            .bind(row.tas_kt)
            .bind(row.fuel_gph)
            .execute(&mut *tx)
            .await?;
        }

        sqlx::query("UPDATE aircraft SET updated_at = now() WHERE id = $1")
            .bind(aircraft_id)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        Ok(true)
    }

    /// Delete a user and everything they own (§9.5.9's "delete my data").
    pub async fn delete_user(&self, user_id: i64) -> Result<bool, AccountsError> {
        let result = sqlx::query("DELETE FROM app_user WHERE id = $1")
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn accounts() -> Option<Accounts> {
        let url = match std::env::var("FF_TEST_DATABASE_URL") {
            Ok(url) if !url.is_empty() => url,
            _ => {
                eprintln!("skipping: FF_TEST_DATABASE_URL not set");
                return None;
            }
        };
        Some(Accounts::open(&url).await.expect("open test database"))
    }

    async fn fresh_user(accounts: &Accounts, subject: &str) -> i64 {
        sqlx::query("DELETE FROM app_user WHERE provider = 'google' AND subject = $1")
            .bind(subject)
            .execute(accounts.pool())
            .await
            .expect("cleanup");
        accounts
            .upsert_user("google", subject, "Pilot", None, None)
            .await
            .expect("create user")
    }

    fn c172(registration: &str) -> AircraftInput {
        AircraftInput {
            registration: registration.to_string(),
            icao_type: Some("C172".into()),
            cruise_tas_kt: Some(110.0),
            cruise_fuel_gph: Some(8.5),
            climb_rate_fpm: Some(700.0),
            climb_tas_kt: Some(75.0),
            template_icao: Some("C172".into()),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn aircraft_round_trip_with_performance_tables() {
        let Some(accounts) = accounts().await else {
            return;
        };
        let user = fresh_user(&accounts, "aircraft-roundtrip").await;

        let created = accounts
            .create_aircraft(user, &c172("N172SP"))
            .await
            .expect("create");
        assert_eq!(created.registration, "N172SP");
        assert_eq!(created.template_icao.as_deref(), Some("C172"));
        assert!(
            created.verified_at.is_none(),
            "a template-seeded aircraft must start unverified"
        );

        let rows = vec![
            PerformanceRow {
                pressure_altitude_ft: 6000,
                power_setting: "65%".into(),
                vertical_speed_fpm: None,
                tas_kt: 110.0,
                fuel_gph: 7.8,
            },
            PerformanceRow {
                pressure_altitude_ft: 2000,
                power_setting: "65%".into(),
                vertical_speed_fpm: None,
                tas_kt: 105.0,
                fuel_gph: 8.4,
            },
        ];
        assert!(accounts
            .replace_performance(user, created.id, Phase::Cruise, &rows)
            .await
            .expect("replace"));

        let detail = accounts
            .get_aircraft(user, created.id)
            .await
            .expect("get")
            .expect("should exist");
        assert_eq!(detail.cruise.len(), 2);
        // Sorted by altitude regardless of the order they were sent in.
        assert_eq!(detail.cruise[0].pressure_altitude_ft, 2000);
        assert_eq!(detail.cruise[1].pressure_altitude_ft, 6000);
        assert!(detail.climb.is_empty() && detail.descent.is_empty());

        // Replacing is a replace, not an append.
        assert!(accounts
            .replace_performance(user, created.id, Phase::Cruise, &rows[..1])
            .await
            .expect("replace again"));
        let detail = accounts
            .get_aircraft(user, created.id)
            .await
            .expect("get")
            .expect("exists");
        assert_eq!(detail.cruise.len(), 1);

        accounts.delete_user(user).await.expect("cleanup");
    }

    #[tokio::test]
    async fn one_users_aircraft_is_invisible_to_another() {
        let Some(accounts) = accounts().await else {
            return;
        };
        let owner = fresh_user(&accounts, "aircraft-owner").await;
        let stranger = fresh_user(&accounts, "aircraft-stranger").await;

        let mine = accounts
            .create_aircraft(owner, &c172("N999AA"))
            .await
            .expect("create");

        // Every accessor must behave as though it simply is not there.
        assert!(accounts
            .get_aircraft(stranger, mine.id)
            .await
            .expect("get")
            .is_none());
        assert!(accounts
            .update_aircraft(stranger, mine.id, &c172("N999AA"))
            .await
            .expect("update")
            .is_none());
        assert!(!accounts
            .delete_aircraft(stranger, mine.id)
            .await
            .expect("delete"));
        assert!(!accounts
            .replace_performance(stranger, mine.id, Phase::Cruise, &[])
            .await
            .expect("replace"));
        assert!(accounts
            .list_aircraft(stranger)
            .await
            .expect("list")
            .is_empty());

        // ...and none of that touched the real owner's copy.
        assert!(accounts
            .get_aircraft(owner, mine.id)
            .await
            .expect("get")
            .is_some());

        accounts.delete_user(owner).await.expect("cleanup");
        accounts.delete_user(stranger).await.expect("cleanup");
    }

    #[tokio::test]
    async fn verifying_records_a_timestamp_that_does_not_drift() {
        let Some(accounts) = accounts().await else {
            return;
        };
        let user = fresh_user(&accounts, "aircraft-verify").await;
        let created = accounts
            .create_aircraft(user, &c172("N55VER"))
            .await
            .expect("create");
        assert!(created.verified_at.is_none());

        let mut input = c172("N55VER");
        input.verified = true;
        let verified = accounts
            .update_aircraft(user, created.id, &input)
            .await
            .expect("update")
            .expect("exists");
        let first_verified_at = verified.verified_at.expect("should be verified");

        // Saving again keeps the original confirmation time rather than
        // silently refreshing it.
        let again = accounts
            .update_aircraft(user, created.id, &input)
            .await
            .expect("update")
            .expect("exists");
        assert_eq!(again.verified_at, Some(first_verified_at));

        // Un-verifying clears it outright.
        input.verified = false;
        let cleared = accounts
            .update_aircraft(user, created.id, &input)
            .await
            .expect("update")
            .expect("exists");
        assert!(cleared.verified_at.is_none());

        accounts.delete_user(user).await.expect("cleanup");
    }

    #[tokio::test]
    async fn a_failed_row_leaves_the_previous_table_intact() {
        let Some(accounts) = accounts().await else {
            return;
        };
        let user = fresh_user(&accounts, "aircraft-atomic").await;
        let created = accounts
            .create_aircraft(user, &c172("N77ATM"))
            .await
            .expect("create");

        let good = PerformanceRow {
            pressure_altitude_ft: 4000,
            power_setting: "65%".into(),
            vertical_speed_fpm: None,
            tas_kt: 108.0,
            fuel_gph: 8.1,
        };
        accounts
            .replace_performance(user, created.id, Phase::Cruise, std::slice::from_ref(&good))
            .await
            .expect("replace");

        // Second row violates CHECK (tas_kt > 0): the whole replace must
        // roll back, not delete the good table and then fail.
        let bad = PerformanceRow {
            pressure_altitude_ft: 8000,
            power_setting: "65%".into(),
            vertical_speed_fpm: None,
            tas_kt: 0.0,
            fuel_gph: 7.5,
        };
        let result = accounts
            .replace_performance(user, created.id, Phase::Cruise, &[good.clone(), bad])
            .await;
        assert!(result.is_err(), "an invalid row was accepted");

        let detail = accounts
            .get_aircraft(user, created.id)
            .await
            .expect("get")
            .expect("exists");
        assert_eq!(
            detail.cruise,
            vec![good],
            "the previous table did not survive a failed replace"
        );

        accounts.delete_user(user).await.expect("cleanup");
    }
}
