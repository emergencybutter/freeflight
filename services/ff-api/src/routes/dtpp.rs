//! Proxies a single FAA d-TPP plate PDF (SID/STAR/Approach chart or
//! Airport Diagram) so the web client's `PlateViewer` can render it itself
//! via pdf.js instead of a plain `<iframe src="https://aeronav.faa.gov/...">`.
//! The iframe approach (still how these plates were shown before
//! `PlateViewer`) can't support highlighter annotation — a cross-origin
//! iframe's rendered content is opaque to this page's JS, so there's
//! nothing to draw an overlay on top of *in registration with the actual
//! page content*. Rendering the PDF ourselves needs the raw bytes, and
//! aeronav.faa.gov sends no `Access-Control-Allow-Origin` header
//! (confirmed live), so a direct `fetch()` from the browser is blocked by
//! CORS; fetching server-side here sidesteps that (CORS is a
//! browser-enforced policy, not a property of the resource itself) and
//! re-serves the same public-domain bytes from our own origin.
use crate::state::AppState;
use axum::extract::{Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

/// Every d-TPP plate URL `ff-etl`'s `dtpp` module constructs has this
/// exact shape: `https://aeronav.faa.gov/d-tpp/<cycle>/<pdf_name>` — see
/// `ff_etl::dtpp`. `url` is caller-supplied (it rides straight through
/// from `ProcedureDetail`/`AirportDetail`'s own `chart_url`/
/// `airport_diagram_url` fields to this query param), so without an
/// allowlist this would be an open server-side-request-forgery-capable
/// proxy — a literal-prefix check is enough here (no need for a URL-
/// parsing dependency) since any string not starting with this exact
/// scheme+host+path can't be mistaken for it: a lookalike host like
/// `aeronav.faa.gov.evil.example` would need `.` immediately after
/// `aeronav.faa.gov` in the prefix position, which fails the check.
const ALLOWED_PREFIX: &str = "https://aeronav.faa.gov/d-tpp/";

fn is_allowed_plate_url(url: &str) -> bool {
    url.starts_with(ALLOWED_PREFIX)
}

#[derive(Debug, Deserialize)]
pub struct PlateQuery {
    pub url: String,
}

pub async fn plate(State(state): State<AppState>, Query(query): Query<PlateQuery>) -> Response {
    if !is_allowed_plate_url(&query.url) {
        return (
            StatusCode::BAD_REQUEST,
            "url must be an https://aeronav.faa.gov/d-tpp/ plate",
        )
            .into_response();
    }
    match state.http.get(&query.url).send().await {
        Ok(resp) if resp.status().is_success() => match resp.bytes().await {
            Ok(bytes) => (StatusCode::OK, [(header::CONTENT_TYPE, "application/pdf")], bytes).into_response(),
            Err(err) => (StatusCode::BAD_GATEWAY, err.to_string()).into_response(),
        },
        Ok(resp) => (
            StatusCode::BAD_GATEWAY,
            format!("upstream returned {}", resp.status()),
        )
            .into_response(),
        Err(err) => (StatusCode::BAD_GATEWAY, err.to_string()).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::is_allowed_plate_url;

    #[test]
    fn accepts_a_real_dtpp_plate_url() {
        assert!(is_allowed_plate_url(
            "https://aeronav.faa.gov/d-tpp/2606/00610IL4L.PDF"
        ));
    }

    #[test]
    fn rejects_a_lookalike_host() {
        assert!(!is_allowed_plate_url(
            "https://aeronav.faa.gov.evil.example/d-tpp/2606/00610IL4L.PDF"
        ));
    }

    #[test]
    fn rejects_a_different_faa_path() {
        assert!(!is_allowed_plate_url(
            "https://aeronav.faa.gov/visual/2026-07-09/sectional-files/foo.zip"
        ));
    }

    #[test]
    fn rejects_a_non_https_scheme() {
        assert!(!is_allowed_plate_url(
            "http://aeronav.faa.gov/d-tpp/2606/00610IL4L.PDF"
        ));
    }

    #[test]
    fn rejects_an_unrelated_url() {
        assert!(!is_allowed_plate_url("https://example.com/evil.pdf"));
    }
}
