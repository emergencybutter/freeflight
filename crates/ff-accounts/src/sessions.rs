//! Users and sessions (DESIGN.md §9.5.5).
//!
//! Replaces `ff-api`'s in-memory session map, whose two problems were
//! that a restart signed everyone out and that there was no user row for
//! anything else to hang off. Aircraft records (step 3) reference
//! `app_user.id`, so identity has to be durable before they can exist.
//!
//! Session tokens are stored **only as a SHA-256 hash**. The token itself
//! is a bearer credential — anyone holding it is signed in — so a leaked
//! dump of this table must not be usable. Lookup hashes the presented
//! token and matches on that, exactly like a password check.

use crate::{Accounts, AccountsError};
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};

/// A signed-in identity as stored, normalized across providers. Mirrors
/// `ff-api`'s `routes::auth::User` plus the row id that owns the data.
#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct StoredUser {
    pub id: i64,
    /// `"google"` / `"discord"` — the provider that vouched for this
    /// identity, and its provider-local stable id.
    pub provider: String,
    pub subject: String,
    pub display_name: String,
    pub email: Option<String>,
    pub avatar_url: Option<String>,
}

/// SHA-256 of the bearer token. The database column is `BYTEA` with a
/// 32-byte length constraint, so storing a raw token by mistake is
/// rejected by the schema rather than silently persisted in plaintext.
fn token_hash(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

impl Accounts {
    /// Record a sign-in, creating the user on first login and refreshing
    /// their profile on every subsequent one.
    ///
    /// Keyed on `(provider, subject)` rather than email: email is
    /// mutable at the provider and is not even guaranteed to be present,
    /// whereas the subject is the provider's own stable id. Signing in
    /// with Google and with Discord therefore gives two distinct
    /// accounts, which is the honest behaviour — nothing here can prove
    /// the two are the same person.
    pub async fn upsert_user(
        &self,
        provider: &str,
        subject: &str,
        display_name: &str,
        email: Option<&str>,
        avatar_url: Option<&str>,
    ) -> Result<i64, AccountsError> {
        let id = sqlx::query_scalar(
            "INSERT INTO app_user (provider, subject, display_name, email, avatar_url, last_seen_at)
             VALUES ($1, $2, $3, $4, $5, now())
             ON CONFLICT (provider, subject) DO UPDATE
                SET display_name = EXCLUDED.display_name,
                    email        = EXCLUDED.email,
                    avatar_url   = EXCLUDED.avatar_url,
                    last_seen_at = now()
             RETURNING id",
        )
        .bind(provider)
        .bind(subject)
        .bind(display_name)
        .bind(email)
        .bind(avatar_url)
        .fetch_one(&self.pool)
        .await?;
        Ok(id)
    }

    /// Store a newly minted session token for `user_id`.
    pub async fn create_session(
        &self,
        token: &str,
        user_id: i64,
        expires_at: DateTime<Utc>,
    ) -> Result<(), AccountsError> {
        sqlx::query("INSERT INTO session (token_hash, user_id, expires_at) VALUES ($1, $2, $3)")
            .bind(token_hash(token))
            .bind(user_id)
            .bind(expires_at)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// The user a token currently signs in as, or `None` if the token is
    /// unknown or expired. Expiry is evaluated by the database (`now()`),
    /// not the caller, so a wrong clock in the app cannot resurrect a
    /// dead session.
    pub async fn session_user(&self, token: &str) -> Result<Option<StoredUser>, AccountsError> {
        let user = sqlx::query_as::<_, StoredUser>(
            "SELECT u.id, u.provider, u.subject, u.display_name, u.email, u.avatar_url
             FROM session s
             JOIN app_user u ON u.id = s.user_id
             WHERE s.token_hash = $1 AND s.expires_at > now()",
        )
        .bind(token_hash(token))
        .fetch_optional(&self.pool)
        .await?;
        Ok(user)
    }

    /// Sign out: drop just this session, leaving the user's other
    /// devices signed in.
    pub async fn delete_session(&self, token: &str) -> Result<(), AccountsError> {
        sqlx::query("DELETE FROM session WHERE token_hash = $1")
            .bind(token_hash(token))
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Drop every expired session, returning how many went. In-memory
    /// sessions used to be swept opportunistically on each write and
    /// otherwise died with the process; durable ones need someone to
    /// actually do it, so `ff-api` runs this periodically.
    pub async fn sweep_expired_sessions(&self) -> Result<u64, AccountsError> {
        let result = sqlx::query("DELETE FROM session WHERE expires_at <= now()")
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

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

    /// Unlike the schema tests in lib.rs, these exercise the public API,
    /// which uses the pool directly and so cannot run inside a rolled-back
    /// transaction. Each test therefore uses its own subject and cleans up
    /// after itself.
    async fn cleanup(accounts: &Accounts, subject: &str) {
        sqlx::query("DELETE FROM app_user WHERE provider = 'google' AND subject = $1")
            .bind(subject)
            .execute(accounts.pool())
            .await
            .expect("cleanup");
    }

    #[tokio::test]
    async fn signing_in_twice_updates_rather_than_duplicates() {
        let Some(accounts) = accounts().await else {
            return;
        };
        let subject = "sessions-upsert";
        cleanup(&accounts, subject).await;

        let first = accounts
            .upsert_user("google", subject, "Amelia", Some("a@example.com"), None)
            .await
            .expect("first sign-in");
        // Second sign-in with a changed display name and avatar: same
        // row, refreshed profile.
        let second = accounts
            .upsert_user(
                "google",
                subject,
                "Amelia Earhart",
                Some("amelia@example.com"),
                Some("https://example.com/a.png"),
            )
            .await
            .expect("second sign-in");
        assert_eq!(first, second, "a second sign-in created a second user");

        let (name, email): (String, Option<String>) =
            sqlx::query_as("SELECT display_name, email FROM app_user WHERE id = $1")
                .bind(first)
                .fetch_one(accounts.pool())
                .await
                .expect("read back");
        assert_eq!(name, "Amelia Earhart");
        assert_eq!(email.as_deref(), Some("amelia@example.com"));

        cleanup(&accounts, subject).await;
    }

    #[tokio::test]
    async fn a_session_round_trips_and_can_be_signed_out() {
        let Some(accounts) = accounts().await else {
            return;
        };
        let subject = "sessions-roundtrip";
        cleanup(&accounts, subject).await;
        let user_id = accounts
            .upsert_user("google", subject, "Pilot", None, None)
            .await
            .expect("upsert");

        let token = "opaque-token-round-trip";
        accounts
            .create_session(token, user_id, Utc::now() + Duration::days(30))
            .await
            .expect("create session");

        let found = accounts.session_user(token).await.expect("lookup");
        let found = found.expect("session should resolve to a user");
        assert_eq!(found.id, user_id);
        assert_eq!(found.provider, "google");
        assert_eq!(found.display_name, "Pilot");

        // An unrelated token is not signed in.
        assert!(accounts
            .session_user("some-other-token")
            .await
            .expect("lookup")
            .is_none());

        accounts.delete_session(token).await.expect("sign out");
        assert!(
            accounts
                .session_user(token)
                .await
                .expect("lookup")
                .is_none(),
            "session survived sign-out"
        );

        cleanup(&accounts, subject).await;
    }

    #[tokio::test]
    async fn the_raw_token_is_never_stored() {
        let Some(accounts) = accounts().await else {
            return;
        };
        let subject = "sessions-hash";
        cleanup(&accounts, subject).await;
        let user_id = accounts
            .upsert_user("google", subject, "Pilot", None, None)
            .await
            .expect("upsert");

        let token = "a-secret-bearer-token";
        accounts
            .create_session(token, user_id, Utc::now() + Duration::days(1))
            .await
            .expect("create session");

        // What is on disk must be the 32-byte digest, not the token — a
        // dump of this table should be useless to whoever reads it.
        let stored: Vec<u8> =
            sqlx::query_scalar("SELECT token_hash FROM session WHERE user_id = $1")
                .bind(user_id)
                .fetch_one(accounts.pool())
                .await
                .expect("read back");
        assert_eq!(stored.len(), 32);
        assert_ne!(stored, token.as_bytes(), "the raw token was stored");
        assert_eq!(stored, token_hash(token));

        cleanup(&accounts, subject).await;
    }

    #[tokio::test]
    async fn expired_sessions_do_not_authenticate_and_get_swept() {
        let Some(accounts) = accounts().await else {
            return;
        };
        let subject = "sessions-expiry";
        cleanup(&accounts, subject).await;
        let user_id = accounts
            .upsert_user("google", subject, "Pilot", None, None)
            .await
            .expect("upsert");

        let token = "already-expired-token";
        accounts
            .create_session(token, user_id, Utc::now() - Duration::minutes(1))
            .await
            .expect("create session");

        assert!(
            accounts
                .session_user(token)
                .await
                .expect("lookup")
                .is_none(),
            "an expired session still authenticated"
        );

        let swept = accounts.sweep_expired_sessions().await.expect("sweep");
        assert!(swept >= 1, "the expired session was not swept");
        let left: i64 = sqlx::query_scalar("SELECT count(*) FROM session WHERE user_id = $1")
            .bind(user_id)
            .fetch_one(accounts.pool())
            .await
            .expect("count");
        assert_eq!(left, 0);

        cleanup(&accounts, subject).await;
    }
}
