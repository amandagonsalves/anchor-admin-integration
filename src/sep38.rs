//! `GET /rate` — the SEP-38 rate callback the platform calls to price a
//! SEP-6/24/31 flow before or during a transaction (`context=sep6|sep24|sep31`).
//! This anchor only ever issues one asset at a fixed 1:1 price minus a flat
//! fee (see `fee::flat_fee`), so "indicative" and "firm" collapse to the same
//! computation — there's no real market rate to hedge or let expire. A
//! multi-asset anchor would source `price` from a real pricing engine here
//! and persist firm quotes it must honor until `expires_at`.

use axum::Json;
use axum::extract::{Query, State};
use axum::http::HeaderMap;
use chrono::{Duration, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::callback_auth::require_platform_auth;
use crate::error::{AppError, AppResult};
use crate::fee;
use crate::routes::SharedState;

#[derive(Debug, Deserialize)]
pub struct RateQuery {
    #[serde(rename = "type")]
    rate_type: String,
    sell_asset: String,
    #[allow(dead_code)]
    buy_asset: String,
    sell_amount: Option<Decimal>,
    buy_amount: Option<Decimal>,
    #[allow(dead_code)]
    context: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct FeeOut {
    total: String,
    asset: String,
}

#[derive(Debug, Serialize)]
pub struct RateOut {
    price: String,
    sell_amount: String,
    buy_amount: String,
    fee: FeeOut,
    #[serde(skip_serializing_if = "Option::is_none")]
    expires_at: Option<String>,
}

pub async fn get_rate(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Query(query): Query<RateQuery>,
) -> AppResult<Json<RateOut>> {
    require_platform_auth(&state, &headers)?;

    let (sell_amount, buy_amount) = match (query.sell_amount, query.buy_amount) {
        (Some(sell), None) => {
            let fee_amount = fee::flat_fee(sell);
            (sell, sell - fee_amount)
        }
        (None, Some(buy)) => {
            // Flat percentage fee taken out of the sell side: buy = sell * (1 - rate).
            let fee_rate = fee::flat_fee(Decimal::ONE);
            let sell = (buy / (Decimal::ONE - fee_rate)).round_dp(7);
            (sell, buy)
        }
        _ => {
            return Err(AppError::BadRequest(
                "exactly one of sell_amount or buy_amount is required".to_string(),
            ));
        }
    };

    let fee_amount = sell_amount - buy_amount;
    let expires_at = match query.rate_type.as_str() {
        "firm" => Some((Utc::now() + Duration::minutes(15)).to_rfc3339()),
        "indicative" => None,
        other => {
            return Err(AppError::BadRequest(format!(
                "unsupported rate type '{other}'"
            )));
        }
    };

    Ok(Json(RateOut {
        price: "1.0000000".to_string(),
        sell_amount: sell_amount.to_string(),
        buy_amount: buy_amount.to_string(),
        fee: FeeOut {
            total: fee_amount.to_string(),
            asset: query.sell_asset,
        },
        expires_at,
    }))
}
