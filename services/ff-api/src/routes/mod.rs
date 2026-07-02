pub mod cycles;
pub mod health;
pub mod notams;
pub mod weather;

use crate::state::AppState;
use axum::routing::get;
use axum::Router;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health::health))
        .route("/weather/metar", get(weather::get_metars))
        .route("/weather/taf", get(weather::get_tafs))
        .route("/cycles/latest", get(cycles::latest))
        .route("/notams", get(notams::get_notams))
        .with_state(state)
}
