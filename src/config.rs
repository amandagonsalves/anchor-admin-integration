use std::env;

#[derive(Clone)]
pub struct Config {
    pub server_addr: String,
    pub base_url: String,
    pub database_url: String,
    pub horizon_url: String,
    pub network_passphrase: String,
    pub distribution_seed: String,
    pub asset_code: String,
    /// Shared secret the platform mints SEP-24 interactive session URLs with
    /// (`SECRET_SEP24_INTERACTIVE_URL_JWT_SECRET` on the platform side). The
    /// platform builds and signs the interactive URL itself when it answers
    /// a wallet's `POST /sep24/transactions/.../interactive`; we only verify
    /// the token when the browser follows that link to our hosted form.
    pub sep24_interactive_jwt_secret: String,
    /// Base URL of the anchor platform's Platform API (port 8085 by default),
    /// used for outbound `notify_*` JSON-RPC calls. See `platform_api`.
    pub platform_api_base_url: String,
    /// Shared secret the platform signs its inbound callback requests with
    /// (`SECRET_CALLBACK_API_AUTH_SECRET` on the platform side). Verified in
    /// `callback_auth`.
    pub callback_api_auth_secret: String,
    /// Shared secret this anchor signs its outbound Platform API requests
    /// with (`SECRET_PLATFORM_API_AUTH_SECRET` on the platform side).
    pub platform_api_auth_secret: String,
}

impl Config {
    pub fn from_env() -> Self {
        dotenvy::dotenv().ok();

        Config {
            server_addr: env_or("SERVER_ADDR", "0.0.0.0:8080"),
            base_url: env_or("BASE_URL", "http://localhost:8080"),
            database_url: require_env("DATABASE_URL"),
            horizon_url: env_or("HORIZON_URL", "https://horizon-testnet.stellar.org"),
            network_passphrase: env_or("NETWORK_PASSPHRASE", "Test SDF Network ; September 2015"),
            distribution_seed: require_env("DISTRIBUTION_SEED"),
            asset_code: env_or("ASSET_CODE", "TEST"),
            sep24_interactive_jwt_secret: require_env("SECRET_SEP24_INTERACTIVE_URL_JWT_SECRET"),
            platform_api_base_url: env_or("PLATFORM_API_BASE_URL", "http://localhost:8085"),
            callback_api_auth_secret: require_env("SECRET_CALLBACK_API_AUTH_SECRET"),
            platform_api_auth_secret: require_env("SECRET_PLATFORM_API_AUTH_SECRET"),
        }
    }
}

fn env_or(key: &str, default: &str) -> String {
    env::var(key).unwrap_or_else(|_| default.to_string())
}

fn require_env(key: &str) -> String {
    env::var(key).unwrap_or_else(|_| panic!("missing required environment variable: {key}"))
}
