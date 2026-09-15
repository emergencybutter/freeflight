pub mod aircraft;
pub mod auth;
pub mod butterlog;
pub mod cycles;
pub mod data;
pub mod dtpp;
pub mod health;
pub mod notams;
pub mod weather;

use crate::ratelimit::RateLimiter;
use crate::state::AppState;
use axum::routing::{get, post, put};
use axum::Router;
use tower_http::services::ServeDir;

pub fn router(state: AppState) -> Router {
    router_with(state, RateLimiter::from_env())
}

/// Split out so tests can build a router with a known limiter (or none)
/// instead of reaching through the environment.
pub fn router_with(state: AppState, limiter: Option<RateLimiter>) -> Router {
    // Published cycle artifacts (cycle.sqlite, chart.pmtiles) are served
    // as plain static files with HTTP Range support — PMTiles is fetched
    // via range requests by MapLibre's pmtiles protocol, so a
    // read-whole-file handler wouldn't work. ServeDir handles Range,
    // ETags, and content types for free.
    let bundles = ServeDir::new(state.data_dir.join("cycles"));

    Router::new()
        .route("/health", get(health::health))
        .route("/data/butterlog/user/:user_id/current", get(butterlog::get_current))
        .route("/data/butterlog/by-discord/:discord_id/current", get(butterlog::get_current_by_discord))
        .route("/cycles/latest", get(cycles::latest))
        .route("/data/airports", get(data::airports))
        .route("/data/attributions", get(data::attributions))
        .route("/data/search", get(data::search))
        .route("/data/airports/:icao", get(data::airport_detail))
        .route(
            "/data/airports/:icao/procedures",
            get(data::airport_procedures),
        )
        .route("/data/procedures/:id", get(data::procedure_detail))
        .route("/data/airways/:ident", get(data::airway_detail))
        .route("/data/search_idents", get(data::search_idents))
        .route("/data/preferred_routes", get(data::preferred_routes))
        .route("/data/charts", get(data::charts))
        .route("/data/airspace", get(data::airspace))
        .route("/data/nearest_fix", get(data::nearest_fix))
        .route("/dtpp/plate", get(dtpp::plate))
        // OAuth sign-in (Google/Discord). `login`/`callback` are top-level
        // browser redirects; `me`/`logout` are bearer-token XHR from the SPA.
        .route("/auth/providers", get(auth::providers))
        .route("/auth/login/:provider", get(auth::login))
        .route("/auth/callback/:provider", get(auth::callback))
        .route("/auth/me", get(auth::me).delete(aircraft::delete_account))
        .route("/auth/logout", post(auth::logout))
        // Aircraft manager (DESIGN.md §9.5). The /types catalog is public
        // (product data, no user in it); everything else is scoped to the
        // signed-in user and 404s on someone else's id.
        .route("/aircraft/types", get(aircraft::list_types))
        .route("/aircraft/types/:icao", get(aircraft::get_type))
        .route("/aircraft", get(aircraft::list).post(aircraft::create))
        .route(
            "/aircraft/:id",
            get(aircraft::get).put(aircraft::update).delete(aircraft::delete),
        )
        .route(
            "/aircraft/:id/performance/:phase",
            put(aircraft::replace_performance),
        )
        .nest_service("/bundles", bundles)
        .merge(proxy_routes(state.clone(), limiter))
        .layer(crate::cors::layer_for(state.auth.clone()))
        .with_state(state)
}

/// The routes that spend an upstream budget rather than serving our own
/// data: `/weather/*` fans out to aviationweather.gov, `/notams` spends
/// the FAA NMS quota attached to our credentials. These are what §11 asks
/// to rate limit, and they are grouped here so the limiter covers exactly
/// them — a pilot pulling chart tiles or querying the local bundle is not
/// costing anyone else anything and should never be throttled for it.
fn proxy_routes(state: AppState, limiter: Option<RateLimiter>) -> Router<AppState> {
    let routes = Router::new()
        .route("/weather/metar", get(weather::get_metars))
        .route("/weather/flightcat", get(weather::get_flight_categories))
        .route("/weather/taf", get(weather::get_tafs))
        .route("/weather/atis", get(weather::get_datis))
        .route("/weather/gairmet", get(weather::get_gairmets))
        .route("/weather/sigmet", get(weather::get_sigmets))
        .route("/weather/isigmet", get(weather::get_intl_sigmets))
        .route("/weather/cwa", get(weather::get_cwas))
        .route("/weather/pirep", get(weather::get_pireps))
        .route("/weather/windtemp", get(weather::get_winds_aloft))
        .route("/notams", get(notams::get_notams));

    match limiter {
        Some(limiter) => routes.layer(axum::middleware::from_fn_with_state(
            limiter,
            crate::ratelimit::enforce,
        )),
        None => routes,
    }
    .with_state(state)
}
