//! Outbound JSON-RPC client to the anchor platform's Platform API (port 8085
//! by default). This is the "you -> platform" surface: every time our custody
//! layer (`ledger`/`observer`/the sep24-sep31 workers) moves money or detects
//! money moving, it reports the fact here so the platform can advance its own
//! transaction state machine. Mirrors `anchor-ms`'s `PlatformApiClient.kt` /
//! `PlatformApiModels.kt`.
//!
//! `notify_onchain_funds_sent`, `notify_offchain_funds_received`, and
//! `notify_transaction_error` param shapes are confirmed against akupay's
//! working `anchor-ms` integration. `notify_onchain_funds_received` and
//! `notify_offchain_funds_sent` follow the same shape by inference from the
//! platform's JSON-RPC methods reference and are NOT yet confirmed against a
//! running `stellar/anchor-platform` image — verify both before relying on
//! them (see the plan's verification section).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum PlatformApiError {
    #[error("http error calling platform api: {0}")]
    Http(#[from] reqwest::Error),
    #[error("platform api rejected {method}: {code} {message}")]
    Rejected {
        method: &'static str,
        code: i64,
        message: String,
    },
}

pub type PlatformApiResult<T> = Result<T, PlatformApiError>;

#[derive(Debug, Serialize)]
struct RpcRequest {
    id: String,
    jsonrpc: &'static str,
    method: &'static str,
    params: Value,
}

#[derive(Debug, Deserialize)]
struct RpcError {
    code: i64,
    message: String,
}

#[derive(Debug, Deserialize)]
struct RpcResponse {
    #[allow(dead_code)]
    id: Option<String>,
    error: Option<RpcError>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RpcAmount {
    pub amount: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asset: Option<String>,
}

pub struct PlatformApiClient {
    http: reqwest::Client,
    base_url: String,
    auth_secret: String,
}

impl PlatformApiClient {
    pub fn new(base_url: String, auth_secret: String) -> Self {
        PlatformApiClient {
            http: reqwest::Client::new(),
            base_url,
            auth_secret,
        }
    }

    /// Deposit: our custody layer sent the Stellar asset to the user and
    /// Horizon confirmed. The platform advances the transaction to `completed`.
    pub async fn notify_onchain_funds_sent(
        &self,
        transaction_id: &str,
        stellar_transaction_id: &str,
        message: Option<&str>,
    ) -> PlatformApiResult<()> {
        self.call(
            "notify_onchain_funds_sent",
            serde_json::json!({
                "transactionId": transaction_id,
                "stellarTransactionId": stellar_transaction_id,
                "message": message,
            }),
        )
        .await
    }

    /// Withdrawal: the user's Stellar payment hit our distribution account.
    /// The platform moves the transaction to `pending_anchor`.
    ///
    /// NOT YET CONFIRMED against a running platform image, see module docs.
    pub async fn notify_onchain_funds_received(
        &self,
        transaction_id: &str,
        stellar_transaction_id: &str,
        amount_in: RpcAmount,
    ) -> PlatformApiResult<()> {
        self.call(
            "notify_onchain_funds_received",
            serde_json::json!({
                "transactionId": transaction_id,
                "stellarTransactionId": stellar_transaction_id,
                "amountIn": amount_in,
            }),
        )
        .await
    }

    /// Deposit: our banking integration (simulated here) confirmed the
    /// user's fiat arrived. The platform moves the transaction to
    /// `pending_anchor`; we then pay out on-chain.
    pub async fn notify_offchain_funds_received(
        &self,
        transaction_id: &str,
        external_transaction_id: &str,
        funds_received_at: DateTime<Utc>,
        amount_in: RpcAmount,
        amount_out: RpcAmount,
        message: Option<&str>,
    ) -> PlatformApiResult<()> {
        self.call(
            "notify_offchain_funds_received",
            serde_json::json!({
                "transactionId": transaction_id,
                "externalTransactionId": external_transaction_id,
                "fundsReceivedAt": funds_received_at.to_rfc3339(),
                "amountIn": amount_in,
                "amountOut": amount_out,
                "message": message,
            }),
        )
        .await
    }

    /// Withdrawal: we executed the (simulated) off-chain payout. The
    /// platform advances the transaction to `completed`.
    ///
    /// NOT YET CONFIRMED against a running platform image, see module docs.
    pub async fn notify_offchain_funds_sent(
        &self,
        transaction_id: &str,
        external_transaction_id: &str,
        message: Option<&str>,
    ) -> PlatformApiResult<()> {
        self.call(
            "notify_offchain_funds_sent",
            serde_json::json!({
                "transactionId": transaction_id,
                "externalTransactionId": external_transaction_id,
                "message": message,
            }),
        )
        .await
    }

    /// A flow failed in a way we can't recover from automatically.
    pub async fn notify_transaction_error(
        &self,
        transaction_id: &str,
        message: &str,
    ) -> PlatformApiResult<()> {
        self.call(
            "notify_transaction_error",
            serde_json::json!({
                "transactionId": transaction_id,
                "message": message,
            }),
        )
        .await
    }

    /// Assigns the on-chain destination (account/memo) for a pending SEP-24
    /// withdrawal, moving it to `pending_user_transfer_start` so the wallet
    /// knows where to send funds. Uses the classic `PATCH /transactions/{id}`
    /// shape shown in the Anchor Platform admin guide's business-server
    /// walkthrough rather than a JSON-RPC method name, since that's the one
    /// concretely documented shape available at time of writing — confirm
    /// against the installed platform version whether your release still
    /// accepts this or requires an equivalent JSON-RPC call instead.
    pub async fn assign_withdrawal_destination(
        &self,
        transaction_id: &str,
        destination_account: &str,
        memo: &str,
        memo_type: &str,
        amount_in: &str,
    ) -> PlatformApiResult<()> {
        let url = format!(
            "{}/transactions/{}",
            self.base_url.trim_end_matches('/'),
            transaction_id
        );
        let body = serde_json::json!({
            "status": "pending_user_transfer_start",
            "destination_account": destination_account,
            "memo": memo,
            "memo_type": memo_type,
            "amount_in": amount_in,
        });

        let response = self
            .http
            .patch(&url)
            .bearer_auth(&self.auth_secret)
            .json(&body)
            .send()
            .await?;

        if let Err(e) = response.error_for_status_ref() {
            let body_text = response.text().await.unwrap_or_default();
            return Err(PlatformApiError::Rejected {
                method: "PATCH /transactions/{id}",
                code: e.status().map(|s| s.as_u16() as i64).unwrap_or(0),
                message: body_text,
            });
        }

        Ok(())
    }

    async fn call(&self, method: &'static str, params: Value) -> PlatformApiResult<()> {
        let request = RpcRequest {
            id: Uuid::new_v4().to_string(),
            jsonrpc: "2.0",
            method,
            params,
        };

        let responses: Vec<RpcResponse> = self
            .http
            .post(&self.base_url)
            .bearer_auth(&self.auth_secret)
            .json(&[request])
            .send()
            .await?
            .json()
            .await?;

        if let Some(error) = responses.into_iter().find_map(|r| r.error) {
            return Err(PlatformApiError::Rejected {
                method,
                code: error.code,
                message: error.message,
            });
        }

        Ok(())
    }
}
