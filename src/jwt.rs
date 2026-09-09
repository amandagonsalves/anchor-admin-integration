use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};

/// Claims for the short-lived token embedded in a SEP-24 interactive session
/// URL we hand back to the platform. Nothing to do with SEP-10 (the platform
/// owns that entirely) — this is purely how `sep24::interactive` recognizes a
/// legitimate visit to its own hosted form.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct InteractiveClaims {
    pub transaction_id: String,
    pub iat: i64,
    pub exp: i64,
}

pub fn encode_jwt<T: Serialize>(
    claims: &T,
    secret: &str,
) -> Result<String, jsonwebtoken::errors::Error> {
    encode(
        &Header::new(Algorithm::HS256),
        claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
}

pub fn decode_jwt<T: for<'de> Deserialize<'de>>(
    token: &str,
    secret: &str,
) -> Result<T, jsonwebtoken::errors::Error> {
    let mut validation = Validation::new(Algorithm::HS256);
    validation.validate_exp = true;
    // Only the interactive-session token uses this codec now, and it carries
    // no audience claim, so validation stays at the library default plus exp.
    validation.required_spec_claims.clear();
    let data = decode::<T>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    )?;
    Ok(data.claims)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_round_trip() {
        let claims = InteractiveClaims {
            transaction_id: "tx-1".into(),
            iat: 1_000,
            exp: 2_000_000_000,
        };
        let token = encode_jwt(&claims, "secret").unwrap();
        let decoded: InteractiveClaims = decode_jwt(&token, "secret").unwrap();
        assert_eq!(decoded.transaction_id, "tx-1");
    }

    #[test]
    fn decode_rejects_wrong_secret() {
        let claims = InteractiveClaims {
            transaction_id: "tx-1".into(),
            iat: 0,
            exp: 2_000_000_000,
        };
        let token = encode_jwt(&claims, "secret").unwrap();
        assert!(decode_jwt::<InteractiveClaims>(&token, "wrong").is_err());
    }
}
