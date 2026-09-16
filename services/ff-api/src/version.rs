//! API version negotiation (DESIGN.md §4.1).
//!
//! §4.1 carried "Versioning: none yet — the web client and `ff-api` deploy
//! together in Phase 1, so breaking changes are coordinated, not
//! negotiated; revisit before Android ships, since app-store clients can't
//! be force-updated in lockstep." Android has shipped, onto a phone that
//! talks to production, so the assumption that both ends deploy together
//! no longer holds.
//!
//! What actually goes wrong without this is not that a client gets an
//! error — it is that it doesn't. Change a field's type and an installed
//! APK deserialises garbage, or silently drops a value, and reports
//! something plausible and wrong to a pilot. The point of negotiating is
//! to convert that into a refusal someone can act on.
//!
//! ## Why a header and not `/v1/`
//!
//! §4.1 suggests either a URL prefix or media-type versioning. The prefix
//! is the more conventional answer and is not available here: nginx on
//! vya2 proxies to this service on
//! `^/(data|bundles|weather|notams|cycles|health|dtpp|auth|aircraft)`, so
//! `/v1/data/...` would miss that rule, fall through to the SPA's
//! `try_files` and return `index.html` — a client asking for JSON would
//! get a page of HTML. Fixing that means a change in the separate
//! `vya-ws/nginx` repo *and* moving every already-deployed client onto new
//! paths, which is a migration to solve a problem we do not have yet.
//!
//! A header needs no routing change, leaves every existing URL working,
//! and is what the clients need anyway: the server has to learn what a
//! client speaks, not the other way round.
//!
//! ## The contract
//!
//! - A request may send `X-Freeflight-Api-Version: <n>`.
//! - **Absent means 1**, frozen ([`ASSUMED_WHEN_ABSENT`]) — clients that
//!   send nothing are the builds that predate negotiation, and those speak
//!   v1. Absence is then bounds-checked like any stated version, so the day
//!   v1 is dropped a silent old client is refused rather than quietly
//!   handed shapes it cannot read.
//! - Every response states the version it was served as, so a client can
//!   notice drift instead of guessing.
//! - A version this server no longer serves gets `426 Upgrade Required`;
//!   one from the future gets `400`. Both with a body saying so.
//!
//! Today [`MIN_SUPPORTED`] and [`CURRENT`] are both 1, so nothing is ever
//! refused. That is the point: the mechanism has to exist *before* the
//! first breaking change, because afterwards is too late for every client
//! already in someone's pocket.

use axum::body::Body;
use axum::http::{HeaderName, HeaderValue, Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

/// The header clients send and this service answers with.
pub const HEADER: &str = "x-freeflight-api-version";

/// Oldest contract still served. Raising this is the breaking change —
/// it is the moment old installs start being turned away, so it wants a
/// deliberate decision and a note in DESIGN.md §4.1, not a quiet bump.
pub const MIN_SUPPORTED: u32 = 1;

/// Newest contract this build speaks.
pub const CURRENT: u32 = 1;

/// What a request with no version header is taken to be speaking.
///
/// **Frozen at 1 forever.** Clients that send nothing are the builds that
/// existed before negotiation did, and those speak v1 — that is a fact
/// about software already in the world, not a policy this server gets to
/// revise. Deriving it from [`MIN_SUPPORTED`] instead would mean that the
/// day v1 is dropped, a silent old client stops being refused and starts
/// being served v2 shapes it cannot parse, which is the exact failure
/// this module exists to prevent.
pub const ASSUMED_WHEN_ABSENT: u32 = 1;

/// What a request's version header amounts to.
#[derive(Debug, PartialEq)]
pub enum Negotiated {
    /// Serve it; the client speaks this version.
    Serve(u32),
    /// Older than anything this build still serves.
    TooOld(u32),
    /// Newer than this build knows — the client is ahead of the server.
    TooNew(u32),
    /// Present but not a version number.
    Malformed,
}

/// Interpret the version header. `None` means the client sent nothing,
/// which is [`ASSUMED_WHEN_ABSENT`] — and is then bounds-checked like any
/// other version, so a silent old client is refused rather than served
/// something it cannot read.
pub fn negotiate(raw: Option<&str>) -> Negotiated {
    let requested = match raw {
        None => ASSUMED_WHEN_ABSENT,
        Some(raw) => match raw.trim().parse::<u32>() {
            Ok(requested) => requested,
            Err(_) => return Negotiated::Malformed,
        },
    };
    match requested {
        v if v < MIN_SUPPORTED => Negotiated::TooOld(v),
        v if v > CURRENT => Negotiated::TooNew(v),
        v => Negotiated::Serve(v),
    }
}

/// Middleware over every route. Stamps the served version on responses and
/// turns an unservable request into an explicit refusal.
pub async fn negotiate_version(request: Request<Body>, next: Next) -> Response {
    let requested = request
        .headers()
        .get(HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);

    let served = match negotiate(requested.as_deref()) {
        Negotiated::Serve(version) => version,
        Negotiated::TooOld(version) => {
            return refusal(
                StatusCode::UPGRADE_REQUIRED,
                format!(
                    "this client speaks API v{version}, which this server no longer serves \
                     (supported: v{MIN_SUPPORTED}-v{CURRENT}). Update the app.\n"
                ),
            );
        }
        Negotiated::TooNew(version) => {
            return refusal(
                StatusCode::BAD_REQUEST,
                format!(
                    "this client asked for API v{version}, which is newer than this server \
                     serves (supported: v{MIN_SUPPORTED}-v{CURRENT}).\n"
                ),
            );
        }
        Negotiated::Malformed => {
            return refusal(
                StatusCode::BAD_REQUEST,
                format!("{HEADER} must be a version number, e.g. {CURRENT}\n"),
            );
        }
    };

    let mut response = next.run(request).await;
    stamp(&mut response, served);
    response
}

fn refusal(status: StatusCode, message: String) -> Response {
    let mut response = (status, message).into_response();
    // Stamped too, so a refused client can still see what this server
    // speaks without a second request.
    stamp(&mut response, CURRENT);
    response
}

fn stamp(response: &mut Response, version: u32) {
    if let Ok(value) = HeaderValue::from_str(&version.to_string()) {
        response
            .headers_mut()
            .insert(HeaderName::from_static(HEADER), value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The case that covers every client deployed today: they send
    /// nothing, and they speak v1.
    #[test]
    fn no_header_means_version_one() {
        assert_eq!(negotiate(None), Negotiated::Serve(ASSUMED_WHEN_ABSENT));
        assert_eq!(ASSUMED_WHEN_ABSENT, 1);
    }

    #[test]
    fn a_supported_version_is_served() {
        assert_eq!(negotiate(Some("1")), Negotiated::Serve(1));
        // Whitespace is a client formatting quirk, not a protocol error.
        assert_eq!(negotiate(Some(" 1 ")), Negotiated::Serve(1));
    }

    #[test]
    fn a_version_from_the_future_is_refused_rather_than_guessed_at() {
        assert_eq!(negotiate(Some("2")), Negotiated::TooNew(2));
        assert_eq!(negotiate(Some("99")), Negotiated::TooNew(99));
    }

    #[test]
    fn nonsense_is_refused_rather_than_treated_as_absent() {
        // Falling back to "absent" here would serve a client that thinks it
        // negotiated something, which is the failure this exists to stop.
        assert_eq!(negotiate(Some("v1")), Negotiated::Malformed);
        assert_eq!(negotiate(Some("")), Negotiated::Malformed);
        assert_eq!(negotiate(Some("1.0")), Negotiated::Malformed);
        assert_eq!(negotiate(Some("-1")), Negotiated::Malformed);
    }

    /// Nothing is refused while both bounds are 1 — every client, header or
    /// not, gets served. Pins that today's behaviour is "no visible change".
    #[test]
    fn today_the_only_refusals_are_future_versions_and_nonsense() {
        assert_eq!(MIN_SUPPORTED, 1);
        assert_eq!(CURRENT, 1);
        assert!(matches!(negotiate(None), Negotiated::Serve(_)));
        assert!(matches!(negotiate(Some("1")), Negotiated::Serve(_)));
    }

    /// A copy of [`negotiate`] with the bounds as parameters, so the
    /// branches that only fire in a future release can be exercised now.
    /// It must stay in step with the real one — every line of it is
    /// asserted against `negotiate` by the test below.
    fn negotiate_with(raw: Option<&str>, min: u32, current: u32) -> Negotiated {
        let requested = match raw {
            None => ASSUMED_WHEN_ABSENT,
            Some(raw) => match raw.trim().parse::<u32>() {
                Ok(requested) => requested,
                Err(_) => return Negotiated::Malformed,
            },
        };
        match requested {
            v if v < min => Negotiated::TooOld(v),
            v if v > current => Negotiated::TooNew(v),
            v => Negotiated::Serve(v),
        }
    }

    #[test]
    fn the_stand_in_agrees_with_the_real_one_at_todays_bounds() {
        for raw in [None, Some("1"), Some("2"), Some("nonsense"), Some("")] {
            assert_eq!(
                negotiate(raw),
                negotiate_with(raw, MIN_SUPPORTED, CURRENT),
                "disagreed on {raw:?}"
            );
        }
    }

    /// The branch this whole module exists for, and the one that stays
    /// dormant until the first breaking change: once v1 is dropped, a v1
    /// client must be turned away.
    #[test]
    fn once_v1_is_dropped_a_v1_client_is_told_to_upgrade() {
        assert_eq!(negotiate_with(Some("1"), 2, 3), Negotiated::TooOld(1));
        assert_eq!(negotiate_with(Some("2"), 2, 3), Negotiated::Serve(2));
    }

    /// The bug a hand-run against a simulated v2 server caught: a client
    /// that sends *nothing* is a pre-negotiation build speaking v1, so it
    /// has to be refused alongside the ones that say so. Reading the
    /// absent case as "the oldest version we still serve" silently handed
    /// it v2 shapes instead — the precise misparse this module exists to
    /// prevent, and a green unit test had asserted it was correct.
    #[test]
    fn once_v1_is_dropped_a_silent_client_is_refused_too() {
        assert_eq!(negotiate_with(None, 2, 3), Negotiated::TooOld(1));
    }
}
