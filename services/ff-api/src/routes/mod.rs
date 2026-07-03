pub mod cycles;
pub mod data;
pub mod health;
pub mod notams;
pub mod weather;

use crate::state::AppState;
use axum::routing::get;
use axum::Router;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

pub fn router(state: AppState) -> Router {
    // Published cycle artifacts (cycle.sqlite, chart.pmtiles) are served
    // as plain static files with HTTP Range support — PMTiles is fetched
    // via range requests by MapLibre's pmtiles protocol, so a
    // read-whole-file handler wouldn't work. ServeDir handles Range,
    // ETags, and content types for free.
    let bundles = ServeDir::new(state.data_dir.join("cycles"));

    Router::new()
        .route("/health", get(health::health))
        .route("/weather/metar", get(weather::get_metars))
        .route("/weather/taf", get(weather::get_tafs))
        .route("/weather/gairmet", get(weather::get_gairmets))
        .route("/weather/sigmet", get(weather::get_sigmets))
        .route("/weather/isigmet", get(weather::get_intl_sigmets))
        .route("/weather/windtemp", get(weather::get_winds_aloft))
        .route("/cycles/latest", get(cycles::latest))
        .route("/data/airports", get(data::airports))
        .route("/data/airports/:icao", get(data::airport_detail))
        .route("/data/airports/:icao/procedures", get(data::airport_procedures))
        .route("/data/procedures/:id", get(data::procedure_detail))
        .route("/data/charts", get(data::charts))
        .route("/notams", get(notams::get_notams))
        .nest_service("/bundles", bundles)
        .with_state(state)
        // Permissive: this proxies only public FAA/NOAA data and takes no
        // credentials from the browser, so there's no cross-origin risk
        // worth restricting during Phase 1 (DESIGN.md §11 revisits this
        // once `ff-sync` carries account state).
        .layer(CorsLayer::permissive())
}
