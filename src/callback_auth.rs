use axum::http::HeaderMap;
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};
use serde::Deserialize;

use crate::error::AppError;
use crate::routes::SharedState;

const CALLBACK_API_AUDIENCE: &str = "callback_api";

#[derive(Debug, Deserialize)]
struct CallbackClaims {
    #[serde(default)]
    aud: Option<AudienceClaim>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum AudienceClaim {
    One(String),
    Many(Vec<String>),
}

impl AudienceClaim {
    fn contains(&self, value: &str) -> bool {
        match self {
            AudienceClaim::One(s) => s == value,
            AudienceClaim::Many(v) => v.iter().any(|s| s == value),
        }
    }
}

/// Verifies the anchor platform's own inbound HS256 JWT on every `/customer`,
/// `/rate`, and `/event` call — a separate trust domain from SEP-10 identity.
/// The platform signs these with `auth.platformToAnchorSecret` /
/// `SECRET_CALLBACK_API_AUTH_SECRET` on its side; this anchor holds the same
/// shared secret via `SECRET_CALLBACK_API_AUTH_SECRET` in its own config.
/// Mirrors `anchor-ms`'s `CallbackApiSecurityConfig`.
pub fn require_platform_auth(state: &SharedState, headers: &HeaderMap) -> Result<(), AppError> {
    let header = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| AppError::NotAuthorized("missing Authorization header".to_string()))?;

    let token = header.strip_prefix("Bearer ").ok_or_else(|| {
        AppError::NotAuthorized("Authorization header must be a Bearer token".to_string())
    })?;

    let mut validation = Validation::new(Algorithm::HS256);
    validation.set_audience(&[CALLBACK_API_AUDIENCE]);
    validation.required_spec_claims.insert("aud".to_string());

    let claims = decode::<CallbackClaims>(
        token,
        &DecodingKey::from_secret(state.config.callback_api_auth_secret.as_bytes()),
        &validation,
    )
    .map_err(|e| AppError::NotAuthorized(format!("invalid platform callback token: {e}")))?
    .claims;

    let audience_ok = claims
        .aud
        .as_ref()
        .is_some_and(|aud| aud.contains(CALLBACK_API_AUDIENCE));
    if !audience_ok {
        return Err(AppError::NotAuthorized(
            "platform callback token missing expected audience".to_string(),
        ));
    }

    Ok(())
}
