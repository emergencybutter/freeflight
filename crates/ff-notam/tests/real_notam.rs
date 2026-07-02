//! Opt-in integration test confirming `NotamClient` reaches the real FAA
//! NMS API (`api-nms.aim.faa.gov`) — the endpoint the old, now-retired
//! FAA NOTAM Search API client pointed at instead (see client.rs docs).
//! Not run by default (needs network):
//!
//! ```sh
//! cargo test -p ff-notam --test real_notam -- --ignored --nocapture
//! ```
//!
//! With bogus credentials this proves the token endpoint is live and
//! speaking the expected OAuth2 `client_credentials` protocol (a
//! structured 401, not a connection failure or a generic gateway 404
//! like the old endpoint gives). It does not prove the NOTAM record
//! shape is correct — that needs a real `client_id`/`client_secret`
//! (requested via NOTAMS@faa.gov), which this crate does not have.
use ff_notam::{NotamClient, NotamError};

#[tokio::test]
#[ignore]
async fn token_endpoint_is_live_and_rejects_bogus_credentials() {
    let client = NotamClient::new("not-a-real-client-id", "not-a-real-secret");
    let result = client.fetch_notams_raw("KSFO").await;
    match result {
        Err(NotamError::TokenRequestFailed { status, .. }) => {
            assert!(
                status.is_client_error(),
                "expected a 4xx from the token endpoint, got {status}"
            );
        }
        other => panic!("expected NotamError::TokenRequestFailed, got {other:?}"),
    }
}
