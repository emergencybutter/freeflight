//! OAuth 2.0 sign-in (Google / Discord) for the web client.
//!
//! DESIGN.md lists accounts as an optional Phase 4 item; this is the first
//! slice of it — just *identity*, not yet server-side sync of plans or
//! profiles. The flow is the standard authorization-code grant, driven by
//! hand with the shared `reqwest` client rather than pulling in an
//! `oauth2`/`openidconnect` crate: two providers, one exchange, no PKCE
//! needed (we hold a client secret server-side).
//!
//! ## Why a bearer token in the redirect fragment, not a cookie
//! In production the web client and this API are the same origin (nginx
//! path-proxies `/auth`, `/data`, … to ff-api), but in local dev the SPA
//! runs on Vite's `:5173` while ff-api is on `:8080` — cross-origin. A
//! `SameSite=Lax` cookie set on the API origin would not ride cross-site
//! XHR back to `/auth/me`, and `SameSite=None` needs HTTPS. Rather than
//! special-case dev, the callback hands the SPA an opaque session token in
//! the URL *fragment* (`#ff_auth=…`, never sent to a server, not in access
//! logs) which the client stores and replays as `Authorization: Bearer`.
//! This works identically in dev and prod and needs no CORS-credentials
//! dance against the existing permissive layer.
//!
//! ## Persistence
//! Sessions are stored in PostgreSQL via `ff-accounts` when
//! `FF_DATABASE_URL` is configured (DESIGN.md §9.5.5): a restart no
//! longer signs everyone out, and there is an `app_user` row for aircraft
//! records to belong to.
//!
//! Without a database it falls back to the original in-memory map, which
//! keeps sign-in working on a fresh checkout, in local dev, and if the
//! database is unreachable at boot — at the cost of sessions dying with
//! the process, exactly as before. The three `*_session` helpers below
//! are the only places that know which of the two is in play.
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::Json;
use chrono::{Duration as ChronoDuration, Utc};
use ff_accounts::{Accounts, StoredUser};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, SystemTime};

/// How long a completed session stays valid. In-memory, so also bounded by
/// server uptime; 30 days keeps a regular user signed in across visits.
const SESSION_TTL: Duration = Duration::from_secs(30 * 24 * 60 * 60);
/// How long a login may sit between the redirect to the provider and the
/// callback — long enough to actually authenticate, short enough that the
/// pending-state map self-cleans. Stale entries are also swept lazily.
const PENDING_TTL: Duration = Duration::from_secs(10 * 60);

/// One OAuth provider's fixed endpoints plus the credentials read from the
/// environment. Only providers with both a client id and secret set are
/// present in `AuthState::providers`.
#[derive(Clone)]
pub struct ProviderConfig {
    pub id: &'static str,
    pub display_name: &'static str,
    pub client_id: String,
    pub client_secret: String,
    pub authorize_url: &'static str,
    pub token_url: &'static str,
    pub userinfo_url: &'static str,
    pub scope: &'static str,
}

/// A signed-in identity, normalized across providers. This is all we keep —
/// no tokens from the provider are stored past the one-shot userinfo fetch.
#[derive(Clone, Serialize)]
pub struct User {
    /// Which provider vouched for this identity (`"google"`/`"discord"`).
    pub provider: &'static str,
    /// Provider-local stable user id (Google `sub`, Discord `id`).
    pub subject: String,
    pub name: String,
    pub email: Option<String>,
    pub avatar_url: Option<String>,
}

struct Session {
    user: User,
    expires: SystemTime,
}

struct Pending {
    provider: &'static str,
    /// Where to send the browser (with the session token) once the
    /// callback completes — the SPA origin that started the login.
    return_origin: String,
    expires: SystemTime,
}

/// All auth-related shared state, built once at startup and hung off
/// `AppState`. Empty `providers` means sign-in is simply off (no env
/// configured) and the routes report as much.
pub struct AuthState {
    providers: HashMap<&'static str, ProviderConfig>,
    /// Public base URL the provider redirects back to, e.g.
    /// `https://freeflight.flyvoyager.net` (prod) or `http://localhost:8080`
    /// (dev). The per-provider redirect URI is `{base}/auth/callback/{id}`
    /// and must match what's registered in the provider's console.
    redirect_base: String,
    /// Allowlist of SPA origins we may hand a session token to via the
    /// post-login fragment redirect — guards against open-redirect / token
    /// exfiltration. From `FF_WEB_ORIGINS`; localhost is always allowed so
    /// dev needs no config.
    web_origins: Vec<String>,
    sessions: tokio::sync::RwLock<HashMap<String, Session>>,
    pending: tokio::sync::RwLock<HashMap<String, Pending>>,
}

impl AuthState {
    /// Reads provider credentials and redirect config from the environment.
    /// Any provider missing either half of its id/secret pair is silently
    /// left out (that button just won't appear in the client).
    pub fn from_env() -> Self {
        let mut providers = HashMap::new();
        if let Some(cfg) = google_from_env() {
            providers.insert(cfg.id, cfg);
        }
        if let Some(cfg) = discord_from_env() {
            providers.insert(cfg.id, cfg);
        }

        let redirect_base = std::env::var("FF_OAUTH_REDIRECT_BASE")
            .unwrap_or_default()
            .trim_end_matches('/')
            .to_string();
        if !providers.is_empty() && redirect_base.is_empty() {
            tracing::warn!(
                "OAuth provider credentials are set but FF_OAUTH_REDIRECT_BASE is not — sign-in will fail until it points at this API's public origin"
            );
        }

        let web_origins = std::env::var("FF_WEB_ORIGINS")
            .unwrap_or_default()
            .split(',')
            .map(|s| s.trim().trim_end_matches('/').to_string())
            .filter(|s| !s.is_empty())
            .collect();

        Self {
            providers,
            redirect_base,
            web_origins,
            sessions: tokio::sync::RwLock::new(HashMap::new()),
            pending: tokio::sync::RwLock::new(HashMap::new()),
        }
    }

    /// True if a browser-supplied SPA origin is allowed to receive a session
    /// token. Localhost (any port/scheme) is always permitted so dev needs
    /// no `FF_WEB_ORIGINS`; everything else must be explicitly allowlisted.
    fn origin_allowed(&self, origin: &str) -> bool {
        if is_localhost_origin(origin) {
            return true;
        }
        self.web_origins.iter().any(|o| o == origin)
    }
}

fn google_from_env() -> Option<ProviderConfig> {
    let client_id = std::env::var("FF_OAUTH_GOOGLE_CLIENT_ID").ok()?;
    let client_secret = std::env::var("FF_OAUTH_GOOGLE_CLIENT_SECRET").ok()?;
    Some(ProviderConfig {
        id: "google",
        display_name: "Google",
        client_id,
        client_secret,
        authorize_url: "https://accounts.google.com/o/oauth2/v2/auth",
        token_url: "https://oauth2.googleapis.com/token",
        userinfo_url: "https://openidconnect.googleapis.com/v1/userinfo",
        scope: "openid email profile",
    })
}

fn discord_from_env() -> Option<ProviderConfig> {
    let client_id = std::env::var("FF_OAUTH_DISCORD_CLIENT_ID").ok()?;
    let client_secret = std::env::var("FF_OAUTH_DISCORD_CLIENT_SECRET").ok()?;
    Some(ProviderConfig {
        id: "discord",
        display_name: "Discord",
        client_id,
        client_secret,
        authorize_url: "https://discord.com/oauth2/authorize",
        token_url: "https://discord.com/api/oauth2/token",
        userinfo_url: "https://discord.com/api/users/@me",
        scope: "identify email",
    })
}

fn is_localhost_origin(origin: &str) -> bool {
    let host = origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"))
        .unwrap_or(origin);
    let host = host.split(&['/', ':'][..]).next().unwrap_or("");
    host == "localhost" || host == "127.0.0.1" || host == "[::1]"
}

// ---- session storage: database when configured, memory otherwise --------

/// Maps a provider name read back out of the database to the `&'static
/// str` the rest of this module uses. A row naming a provider this build
/// doesn't know is treated as no session rather than trusted — the value
/// is echoed to the client and compared against elsewhere (the web app
/// checks `provider == "discord"` to auto-link Butterlog), so it should
/// only ever be one of the values we mint.
fn known_provider(name: &str) -> Option<&'static str> {
    match name {
        "google" => Some("google"),
        "discord" => Some("discord"),
        _ => None,
    }
}

/// Persist a new session for `user`, returning false if it could not be
/// stored (the caller then fails the login rather than handing out a
/// token that will not work).
async fn store_session(state: &AppState, token: &str, user: &User) -> bool {
    let Some(accounts) = &state.accounts else {
        let mut sessions = state.auth.sessions.write().await;
        sweep_expired(&mut sessions, |s| s.expires);
        sessions.insert(
            token.to_string(),
            Session {
                user: user.clone(),
                expires: SystemTime::now() + SESSION_TTL,
            },
        );
        return true;
    };
    let user_id = match accounts
        .upsert_user(
            user.provider,
            &user.subject,
            &user.name,
            user.email.as_deref(),
            user.avatar_url.as_deref(),
        )
        .await
    {
        Ok(id) => id,
        Err(err) => {
            tracing::error!("could not record the signed-in user: {err}");
            return false;
        }
    };
    let expires = Utc::now() + ChronoDuration::seconds(SESSION_TTL.as_secs() as i64);
    if let Err(err) = accounts.create_session(token, user_id, expires).await {
        tracing::error!("could not store the session: {err}");
        return false;
    }
    true
}

/// The user a bearer token is currently signed in as, if any.
async fn load_session(state: &AppState, token: &str) -> Option<User> {
    let Some(accounts) = &state.accounts else {
        let sessions = state.auth.sessions.read().await;
        return sessions
            .get(token)
            .filter(|s| s.expires > SystemTime::now())
            .map(|s| s.user.clone());
    };
    match accounts.session_user(token).await {
        Ok(Some(stored)) => Some(User {
            provider: known_provider(&stored.provider)?,
            subject: stored.subject,
            name: stored.display_name,
            email: stored.email,
            avatar_url: stored.avatar_url,
        }),
        Ok(None) => None,
        Err(err) => {
            // A database blip must read as "not signed in" for this
            // request, not as a 500 — the client already handles an
            // unauthenticated /auth/me by showing the sign-in button.
            tracing::warn!("session lookup failed: {err}");
            None
        }
    }
}

/// Sign out of this one session, leaving the user's other devices alone.
async fn drop_session(state: &AppState, token: &str) {
    let Some(accounts) = &state.accounts else {
        state.auth.sessions.write().await.remove(token);
        return;
    };
    if let Err(err) = accounts.delete_session(token).await {
        tracing::warn!("could not delete the session: {err}");
    }
}

/// The signed-in user for an endpoint that needs the account database,
/// together with a handle on it.
///
/// Distinguishes the two ways such a request can fail, because they mean
/// different things to a client: **503** if this deployment has no
/// account database at all (the feature is off — no amount of signing in
/// will help), **401** if it does but you are not signed in.
///
/// Returns `StoredUser` rather than `User` because everything owned by an
/// account is keyed on `app_user.id`, which the public shape omits.
pub async fn require_account_user(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<(Accounts, StoredUser), Response> {
    let Some(accounts) = state.accounts.clone() else {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "accounts are not configured on this server",
        )
            .into_response());
    };
    let Some(token) = bearer_token(headers) else {
        return Err((StatusCode::UNAUTHORIZED, "not signed in").into_response());
    };
    match accounts.session_user(&token).await {
        Ok(Some(user)) => Ok((accounts, user)),
        Ok(None) => Err((StatusCode::UNAUTHORIZED, "not signed in").into_response()),
        Err(err) => {
            tracing::warn!("session lookup failed: {err}");
            Err((StatusCode::SERVICE_UNAVAILABLE, "account storage unavailable").into_response())
        }
    }
}

/// Drop every expired session. Called periodically from `main` — the
/// in-memory map self-swept on each write, but rows do not.
pub async fn sweep_expired_sessions(state: &AppState) {
    let Some(accounts) = &state.accounts else {
        let mut sessions = state.auth.sessions.write().await;
        sweep_expired(&mut sessions, |s| s.expires);
        return;
    };
    match accounts.sweep_expired_sessions().await {
        Ok(0) => {}
        Ok(n) => tracing::info!("swept {n} expired session(s)"),
        Err(err) => tracing::warn!("could not sweep expired sessions: {err}"),
    }
}

/// 32 bytes of OS randomness, hex-encoded — used for both opaque session
/// tokens and single-use CSRF `state` values. 256 bits is well beyond
/// guessable.
fn random_token() -> String {
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes).expect("OS RNG unavailable");
    let mut out = String::with_capacity(64);
    for b in bytes {
        out.push(char::from_digit((b >> 4) as u32, 16).unwrap());
        out.push(char::from_digit((b & 0xf) as u32, 16).unwrap());
    }
    out
}

// ---- GET /auth/providers -------------------------------------------------

#[derive(Serialize)]
pub struct ProviderInfo {
    pub id: &'static str,
    pub display_name: &'static str,
}

/// Lists the sign-in providers this deployment actually has configured, so
/// the client only renders buttons that will work.
pub async fn providers(State(state): State<AppState>) -> Json<Vec<ProviderInfo>> {
    let mut list: Vec<ProviderInfo> = state
        .auth
        .providers
        .values()
        .map(|p| ProviderInfo {
            id: p.id,
            display_name: p.display_name,
        })
        .collect();
    // Stable order (map iteration isn't) so the buttons don't reshuffle.
    list.sort_by_key(|p| p.id);
    Json(list)
}

// ---- GET /auth/login/:provider ------------------------------------------

#[derive(Deserialize)]
pub struct LoginQuery {
    /// SPA origin to return the browser to after login (defaults to the
    /// redirect base's origin — i.e. same-origin prod).
    #[serde(rename = "return")]
    pub return_origin: Option<String>,
}

pub async fn login(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    Query(query): Query<LoginQuery>,
) -> Response {
    let auth = &state.auth;
    let Some(cfg) = auth.providers.get(provider.as_str()) else {
        return (StatusCode::NOT_FOUND, "unknown or unconfigured provider").into_response();
    };
    if auth.redirect_base.is_empty() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "sign-in is not configured (FF_OAUTH_REDIRECT_BASE unset)",
        )
            .into_response();
    }

    let return_origin = query
        .return_origin
        .map(|o| o.trim_end_matches('/').to_string())
        .unwrap_or_else(|| auth.redirect_base.clone());
    if !auth.origin_allowed(&return_origin) {
        return (StatusCode::BAD_REQUEST, "return origin not allowed").into_response();
    }

    let csrf = random_token();
    {
        let mut pending = auth.pending.write().await;
        sweep_expired(&mut pending, |p| p.expires);
        pending.insert(
            csrf.clone(),
            Pending {
                provider: cfg.id,
                return_origin,
                expires: SystemTime::now() + PENDING_TTL,
            },
        );
    }

    let redirect_uri = format!("{}/auth/callback/{}", auth.redirect_base, cfg.id);
    let authorize = match reqwest::Url::parse_with_params(
        cfg.authorize_url,
        &[
            ("client_id", cfg.client_id.as_str()),
            ("redirect_uri", redirect_uri.as_str()),
            ("response_type", "code"),
            ("scope", cfg.scope),
            ("state", csrf.as_str()),
            // Google only returns an id/userinfo consistently when asked to
            // prompt; harmless to Discord (ignored).
            ("prompt", "consent"),
        ],
    ) {
        Ok(url) => url,
        Err(err) => {
            tracing::error!("failed to build authorize URL: {err}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "bad provider config").into_response();
        }
    };
    Redirect::to(authorize.as_str()).into_response()
}

// ---- GET /auth/callback/:provider ---------------------------------------

#[derive(Deserialize)]
pub struct CallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
}

pub async fn callback(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    Query(query): Query<CallbackQuery>,
) -> Response {
    let auth = &state.auth;

    // Consume the pending CSRF state first — it also tells us where to send
    // the user back to on either success or failure.
    let csrf = query.state.clone().unwrap_or_default();
    let pending = {
        let mut pending = auth.pending.write().await;
        sweep_expired(&mut pending, |p| p.expires);
        pending.remove(&csrf)
    };
    let Some(pending) = pending else {
        return (StatusCode::BAD_REQUEST, "invalid or expired login state").into_response();
    };
    if pending.provider != provider {
        return (StatusCode::BAD_REQUEST, "provider mismatch").into_response();
    }
    let return_origin = pending.return_origin;

    if let Some(err) = query.error {
        tracing::info!("provider {provider} returned OAuth error: {err}");
        return Redirect::to(&format!("{return_origin}/#ff_auth_error=denied")).into_response();
    }
    let Some(code) = query.code else {
        return Redirect::to(&format!("{return_origin}/#ff_auth_error=no_code")).into_response();
    };

    let Some(cfg) = auth.providers.get(provider.as_str()) else {
        return (StatusCode::NOT_FOUND, "unknown provider").into_response();
    };
    let redirect_uri = format!("{}/auth/callback/{}", auth.redirect_base, cfg.id);

    let user = match exchange_and_fetch_user(&state.http, cfg, &code, &redirect_uri).await {
        Ok(user) => user,
        Err(err) => {
            tracing::warn!("OAuth callback for {provider} failed: {err}");
            return Redirect::to(&format!("{return_origin}/#ff_auth_error=exchange"))
                .into_response();
        }
    };

    let token = random_token();
    if !store_session(&state, &token, &user).await {
        // The identity checked out but we could not persist the session,
        // so the token would be dead on arrival. Report it as a failed
        // login rather than handing over one that silently doesn't work.
        return Redirect::to(&format!("{return_origin}/#ff_auth_error=storage")).into_response();
    }

    // Hand the token to the SPA via the fragment (never leaves the browser).
    Redirect::to(&format!("{return_origin}/#ff_auth={token}")).into_response()
}

/// Trades the authorization `code` for an access token, then fetches and
/// normalizes the provider's userinfo into a `User`.
async fn exchange_and_fetch_user(
    http: &reqwest::Client,
    cfg: &ProviderConfig,
    code: &str,
    redirect_uri: &str,
) -> Result<User, String> {
    let token_resp = http
        .post(cfg.token_url)
        .header(header::ACCEPT, "application/json")
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("client_id", cfg.client_id.as_str()),
            ("client_secret", cfg.client_secret.as_str()),
        ])
        .send()
        .await
        .map_err(|e| format!("token request failed: {e}"))?;
    if !token_resp.status().is_success() {
        let status = token_resp.status();
        let body = token_resp.text().await.unwrap_or_default();
        return Err(format!("token endpoint returned {status}: {body}"));
    }
    let token_json: TokenResponse = token_resp
        .json()
        .await
        .map_err(|e| format!("decoding token response: {e}"))?;

    let userinfo = http
        .get(cfg.userinfo_url)
        .bearer_auth(&token_json.access_token)
        .header(header::ACCEPT, "application/json")
        .send()
        .await
        .map_err(|e| format!("userinfo request failed: {e}"))?;
    if !userinfo.status().is_success() {
        let status = userinfo.status();
        return Err(format!("userinfo endpoint returned {status}"));
    }
    let raw: serde_json::Value = userinfo
        .json()
        .await
        .map_err(|e| format!("decoding userinfo: {e}"))?;

    Ok(normalize_user(cfg.id, &raw))
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
}

/// Maps a provider's userinfo JSON into our uniform `User`. Kept provider-
/// aware (rather than one hopeful field set) because Google and Discord name
/// these fields differently.
fn normalize_user(provider: &'static str, raw: &serde_json::Value) -> User {
    let s = |key: &str| raw.get(key).and_then(|v| v.as_str()).map(str::to_string);
    match provider {
        "discord" => {
            let subject = s("id").unwrap_or_default();
            let name = s("global_name")
                .or_else(|| s("username"))
                .unwrap_or_else(|| "Discord user".to_string());
            let avatar_url = match (s("avatar"), subject.is_empty()) {
                (Some(hash), false) => Some(format!(
                    "https://cdn.discordapp.com/avatars/{subject}/{hash}.png"
                )),
                _ => None,
            };
            User {
                provider,
                subject,
                name,
                email: s("email"),
                avatar_url,
            }
        }
        // Google (OpenID Connect userinfo) and any future OIDC provider.
        _ => User {
            provider,
            subject: s("sub").unwrap_or_default(),
            name: s("name")
                .or_else(|| s("email"))
                .unwrap_or_else(|| "User".to_string()),
            email: s("email"),
            avatar_url: s("picture"),
        },
    }
}

// ---- GET /auth/me  &  POST /auth/logout ---------------------------------

/// Pulls the bearer token out of the `Authorization` header, if present.
fn bearer_token(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    value
        .strip_prefix("Bearer ")
        .or_else(|| value.strip_prefix("bearer "))
        .map(str::to_string)
}

pub async fn me(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let Some(token) = bearer_token(&headers) else {
        return (StatusCode::UNAUTHORIZED, "not signed in").into_response();
    };
    match load_session(&state, &token).await {
        Some(user) => Json(user).into_response(),
        None => (StatusCode::UNAUTHORIZED, "not signed in").into_response(),
    }
}

pub async fn logout(State(state): State<AppState>, headers: HeaderMap) -> StatusCode {
    if let Some(token) = bearer_token(&headers) {
        drop_session(&state, &token).await;
    }
    StatusCode::NO_CONTENT
}

/// Drops expired entries from a token map while we already hold the write
/// lock — cheap opportunistic GC so the in-memory maps don't grow without
/// bound over a long-running server.
fn sweep_expired<V>(map: &mut HashMap<String, V>, expires: impl Fn(&V) -> SystemTime) {
    let now = SystemTime::now();
    map.retain(|_, v| expires(v) > now);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn localhost_origins_are_allowed_without_config() {
        assert!(is_localhost_origin("http://localhost:5173"));
        assert!(is_localhost_origin("http://127.0.0.1:8080"));
        assert!(is_localhost_origin("https://localhost"));
        assert!(!is_localhost_origin("https://freeflight.flyvoyager.net"));
        assert!(!is_localhost_origin("http://localhost.evil.example"));
    }

    #[test]
    fn random_tokens_are_64_hex_chars_and_distinct() {
        let a = random_token();
        let b = random_token();
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }

    #[test]
    fn normalizes_google_userinfo() {
        let raw = serde_json::json!({
            "sub": "12345",
            "name": "Ada Lovelace",
            "email": "ada@example.com",
            "picture": "https://example.com/a.png",
        });
        let u = normalize_user("google", &raw);
        assert_eq!(u.subject, "12345");
        assert_eq!(u.name, "Ada Lovelace");
        assert_eq!(u.email.as_deref(), Some("ada@example.com"));
        assert_eq!(u.avatar_url.as_deref(), Some("https://example.com/a.png"));
    }

    #[test]
    fn normalizes_discord_userinfo_and_builds_avatar_url() {
        let raw = serde_json::json!({
            "id": "999",
            "username": "ada",
            "global_name": "Ada",
            "email": "ada@example.com",
            "avatar": "abc",
        });
        let u = normalize_user("discord", &raw);
        assert_eq!(u.subject, "999");
        assert_eq!(u.name, "Ada");
        assert_eq!(
            u.avatar_url.as_deref(),
            Some("https://cdn.discordapp.com/avatars/999/abc.png")
        );
    }

    #[test]
    fn discord_without_avatar_has_none() {
        let raw = serde_json::json!({ "id": "999", "username": "ada" });
        let u = normalize_user("discord", &raw);
        assert_eq!(u.name, "ada");
        assert!(u.avatar_url.is_none());
    }
}
