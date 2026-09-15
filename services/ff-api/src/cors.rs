//! Cross-origin policy (DESIGN.md §11).
//!
//! This used to be `CorsLayer::permissive()`, justified by a comment
//! saying the server "proxies only public FAA/NOAA data and takes no
//! credentials from the browser". The first half is still true; the second
//! stopped being true when OAuth sign-in and the aircraft manager landed
//! (§9.5) — `/auth/me`, `/auth/logout` and `/aircraft/*` all read an
//! `Authorization: Bearer` token now.
//!
//! Bearer tokens are not cookies, so a permissive policy was never a
//! session-hijacking hole the way `credentials: include` would have been —
//! a browser does not attach them cross-origin on its own. What it *did*
//! leave open is the thing §11 actually names: any page on the internet
//! could use this as a free METAR relay from its visitors' browsers,
//! spending our upstream quota under their addresses.
//!
//! Restricting it costs the deployed web client nothing, because nginx
//! serves the SPA and proxies the API under one hostname — those requests
//! are same-origin, which CORS does not govern. It only bites a client
//! served from somewhere else, which in practice means `npm run dev`.
//!
//! The allowlist is deliberately **the same one the OAuth flow already
//! uses** (`FF_WEB_ORIGINS`, plus localhost for free). Both answer the
//! identical question — "is this origin one of ours?" — and giving them
//! separate variables would mean an operator can set one, miss the other,
//! and get a half-configured deployment whose symptom appears in an
//! unrelated place.

use crate::routes::auth::AuthState;
use axum::http::{HeaderValue, Method};
use std::sync::Arc;
use tower_http::cors::{AllowOrigin, CorsLayer};

/// Builds the CORS layer, allowing exactly the origins that
/// [`AuthState::origin_allowed`] accepts.
pub fn layer_for(auth: Arc<AuthState>) -> CorsLayer {
    CorsLayer::new()
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
        // Mirrors whatever the request asks for, which covers the
        // `Authorization` and `Content-Type` the SPA sends without pinning
        // a list here that drifts from the routes.
        .allow_headers(tower_http::cors::Any)
        .allow_origin(AllowOrigin::predicate(
            move |origin: &HeaderValue, _request| {
                origin
                    .to_str()
                    .map(|origin| auth.origin_allowed(origin))
                    .unwrap_or(false)
            },
        ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routes::auth::AuthState;

    /// The predicate is the whole policy, so it is what gets tested —
    /// building a `CorsLayer` and driving a preflight through tower would
    /// test tower-http, not this.
    fn allows(auth: &AuthState, origin: &str) -> bool {
        auth.origin_allowed(origin)
    }

    #[test]
    fn localhost_is_allowed_so_a_fresh_checkout_needs_no_config() {
        let auth = AuthState::from_env();
        assert!(allows(&auth, "http://localhost:5173"));
        assert!(allows(&auth, "http://127.0.0.1:5173"));
    }

    #[test]
    fn an_unrelated_site_is_refused() {
        let auth = AuthState::from_env();
        // The §11 case: someone else's page using this as a METAR relay
        // from their visitors' browsers.
        assert!(!allows(&auth, "https://example.com"));
        // A lookalike that merely contains the real host must not pass.
        assert!(!allows(&auth, "https://freeflight.flyvoyager.net.evil.test"));
    }
}
