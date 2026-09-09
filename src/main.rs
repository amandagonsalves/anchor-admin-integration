use std::sync::Arc;

use anchor_rust::config::Config;
use anchor_rust::db;
use anchor_rust::ledger::Ledger;
use anchor_rust::platform_api::PlatformApiClient;
use anchor_rust::routes::{self, AppState};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let config = Config::from_env();

    let db = db::connect(&config.database_url)
        .await
        .expect("failed to connect to database / run migrations");

    let ledger = Ledger::new(
        &config.horizon_url,
        &config.network_passphrase,
        &config.distribution_seed,
        config.asset_code.clone(),
    )
    .expect("failed to initialize ledger client");

    let platform_api = PlatformApiClient::new(
        config.platform_api_base_url.clone(),
        config.platform_api_auth_secret.clone(),
    );

    tracing::info!(
        distribution_account = %ledger.distribution_account_id(),
        asset_code = %config.asset_code,
        platform_api_base_url = %config.platform_api_base_url,
        "anchor business server starting"
    );

    let addr = config.server_addr.clone();
    let state = Arc::new(AppState {
        config,
        db,
        ledger,
        platform_api,
    });

    tokio::spawn(anchor_rust::observer::run(state.clone()));

    let app = routes::build_router(state);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| panic!("failed to bind {addr}: {e}"));
    tracing::info!(%addr, "listening");
    axum::serve(listener, app).await.expect("server error");
}
