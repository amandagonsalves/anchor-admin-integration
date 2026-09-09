use std::sync::Arc;

use axum::Router;
use axum::routing::{get, post};
use sqlx::PgPool;

use crate::config::Config;
use crate::ledger::Ledger;
use crate::platform_api::PlatformApiClient;

pub struct AppState {
    pub config: Config,
    pub db: PgPool,
    pub ledger: Ledger,
    pub platform_api: PlatformApiClient,
}

pub type SharedState = Arc<AppState>;

/// Everything wallet-facing (SEP-1/10/24/31 wire endpoints) is served by the
/// anchor platform now, not here. This process only answers the platform's
/// own callbacks, plus hosts the SEP-24 interactive UI the platform links out
/// to.
pub fn build_router(state: SharedState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route(
            "/customer",
            get(crate::sep12::get_customer).put(crate::sep12::put_customer),
        )
        .route(
            "/customer/{account}",
            axum::routing::delete(crate::sep12::delete_customer),
        )
        .route("/rate", get(crate::sep38::get_rate))
        .route("/event", post(crate::event::handle_event))
        .route(
            "/sep24/interactive",
            get(crate::sep24::interactive::show_form),
        )
        .route(
            "/sep24/interactive/submit",
            post(crate::sep24::interactive::submit_form),
        )
        .with_state(state)
}

async fn health() -> &'static str {
    "ok"
}
