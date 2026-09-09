//! `POST /event` — the platform's event-processor bridges its internal
//! Kafka stream (`transaction_created`, `transaction_status_changed`, ...) to
//! this single HTTP webhook, same shape as `anchor-ms`'s
//! `EventCallbackController`. Event field names below (`sepTransactionId`,
//! `amountIn`, `feeTotal`, ...) are drawn directly from `anchor-ms`'s
//! `AnchorEventModels.kt`, which is a confirmed-working shape against the
//! real platform.
//!
//! This anchor allocates a withdrawal/receive destination + memo eagerly,
//! right here on `transaction_created`, and reports it back via
//! `platform_api::assign_withdrawal_destination` (a `PATCH` call). The skill
//! reference for business servers also documents a pull-based
//! `GET /unique-address` callback for the same purpose — confirm which
//! mechanism your installed platform version actually expects before
//! shipping this for real; this project picked the push-based event flow to
//! keep one consistent mechanism across SEP-24 and SEP-31 rather than
//! implementing both without a live platform to verify either against.

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::Deserialize;

use crate::callback_auth::require_platform_auth;
use crate::db::{sep24_transactions, sep31_transactions};
use crate::error::{AppError, AppResult};
use crate::ledger::Ledger;
use crate::platform_api::RpcAmount;
use crate::routes::SharedState;

#[derive(Debug, Deserialize)]
pub struct EventRequest {
    #[serde(rename = "type")]
    pub event_type: String,
    pub payload: EventPayload,
}

#[derive(Debug, Deserialize)]
pub struct EventPayload {
    pub transaction: Option<EventTransaction>,
}

#[derive(Debug, Deserialize)]
pub struct EventAmount {
    pub amount: Decimal,
    #[allow(dead_code)]
    pub asset: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct EventTransaction {
    #[serde(rename = "sepTransactionId")]
    pub platform_transaction_id: String,
    pub sep: u8,
    pub kind: String,
    #[allow(dead_code)]
    pub status: String,
    pub account: Option<String>,
    pub memo: Option<String>,
    pub memo_type: Option<String>,
    pub asset_code: Option<String>,
    #[serde(rename = "amountIn")]
    pub amount_in: Option<EventAmount>,
    #[serde(rename = "amountOut")]
    pub amount_out: Option<EventAmount>,
    #[serde(rename = "feeTotal")]
    pub fee_total: Option<Decimal>,
    pub sender_id: Option<String>,
    pub receiver_id: Option<String>,
    #[allow(dead_code)]
    #[serde(rename = "startedAt")]
    pub started_at: Option<DateTime<Utc>>,
}

pub async fn handle_event(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(request): Json<EventRequest>,
) -> AppResult<StatusCode> {
    require_platform_auth(&state, &headers)?;

    match request.event_type.as_str() {
        "transaction_created" => {
            let Some(tx) = request.payload.transaction else {
                return Ok(StatusCode::OK);
            };
            match tx.sep {
                24 => handle_sep24_created(&state, tx).await?,
                31 => handle_sep31_created(&state, tx).await?,
                other => {
                    tracing::debug!(
                        sep = other,
                        "ignoring transaction_created for unhandled sep"
                    )
                }
            }
        }
        "transaction_status_changed" | "customer_status_changed" => {
            // Status mirroring and KYC re-checks are driven by our own
            // workers (the observer, and the SEP-12 PUT /customer handler)
            // rather than this event, per the "do not run a second source of
            // truth" guidance for the platform's own Stellar Observer.
        }
        other => tracing::debug!(event_type = other, "ignoring unrecognized event type"),
    }

    Ok(StatusCode::OK)
}

async fn handle_sep24_created(state: &SharedState, tx: EventTransaction) -> AppResult<()> {
    let asset_code = tx
        .asset_code
        .unwrap_or_else(|| state.config.asset_code.clone());
    let account = tx
        .account
        .ok_or_else(|| AppError::BadRequest("transaction_created missing account".to_string()))?;

    if tx.kind == "withdrawal" {
        let withdraw_memo: u64 = rand::random::<u32>() as u64 + 1;
        let withdraw_memo = withdraw_memo.to_string();
        let amount_in = tx.amount_in.as_ref().map(|a| a.amount);
        let amount_in_str = amount_in.map(|a| a.to_string()).unwrap_or_default();

        sep24_transactions::create(
            &state.db,
            sep24_transactions::NewTransaction {
                platform_transaction_id: &tx.platform_transaction_id,
                kind: "withdrawal",
                account: &account,
                memo: None,
                memo_type: None,
                asset_code: &asset_code,
                amount_in,
                withdraw_memo: Some(&withdraw_memo),
            },
        )
        .await
        .map_err(AppError::Db)?;

        if let Err(e) = state
            .platform_api
            .assign_withdrawal_destination(
                &tx.platform_transaction_id,
                &state.ledger.distribution_account_id(),
                &withdraw_memo,
                "id",
                &amount_in_str,
            )
            .await
        {
            tracing::error!(error = %e, transaction_id = %tx.platform_transaction_id, "failed to assign withdrawal destination");
        }
    } else {
        sep24_transactions::create(
            &state.db,
            sep24_transactions::NewTransaction {
                platform_transaction_id: &tx.platform_transaction_id,
                kind: "deposit",
                account: &account,
                memo: tx.memo.as_deref(),
                memo_type: tx.memo_type.as_deref(),
                asset_code: &asset_code,
                amount_in: tx.amount_in.map(|a| a.amount),
                withdraw_memo: None,
            },
        )
        .await
        .map_err(AppError::Db)?;
    }

    Ok(())
}

async fn handle_sep31_created(state: &SharedState, tx: EventTransaction) -> AppResult<()> {
    let asset_code = tx
        .asset_code
        .unwrap_or_else(|| state.config.asset_code.clone());
    let account = tx
        .account
        .ok_or_else(|| AppError::BadRequest("transaction_created missing account".to_string()))?;
    let amount_in = tx
        .amount_in
        .ok_or_else(|| AppError::BadRequest("transaction_created missing amount_in".to_string()))?
        .amount;

    let fee = tx.fee_total.unwrap_or_default();
    let amount_out = tx.amount_out.map(|a| a.amount).unwrap_or(amount_in - fee);

    let stellar_memo: u64 = rand::random::<u32>() as u64 + 1;
    let stellar_memo = stellar_memo.to_string();

    sep31_transactions::create(
        &state.db,
        sep31_transactions::NewTransaction {
            platform_transaction_id: &tx.platform_transaction_id,
            creator_account: &account,
            creator_memo: tx.memo.as_deref(),
            asset_code: &asset_code,
            amount_in,
            amount_out: Some(amount_out),
            fee: Some(fee),
            sender_id: tx.sender_id.as_deref(),
            receiver_id: tx.receiver_id.as_deref(),
            stellar_memo: &stellar_memo,
        },
    )
    .await
    .map_err(AppError::Db)?;

    if let Err(e) = state
        .platform_api
        .assign_withdrawal_destination(
            &tx.platform_transaction_id,
            &state.ledger.distribution_account_id(),
            &stellar_memo,
            "id",
            &amount_in.to_string(),
        )
        .await
    {
        tracing::error!(error = %e, transaction_id = %tx.platform_transaction_id, "failed to assign sep-31 receive destination");
    }

    Ok(())
}

/// Convenience used by the observer to build the `RpcAmount` the
/// `notify_onchain_funds_received` call expects.
pub fn amount_in_stellar(ledger: &Ledger, amount: Decimal) -> RpcAmount {
    RpcAmount {
        amount: amount.to_string(),
        asset: Some(format!(
            "stellar:{}:{}",
            ledger.asset_code,
            ledger.distribution_account_id()
        )),
    }
}
