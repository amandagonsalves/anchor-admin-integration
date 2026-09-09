use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

use askama::Template;
use axum::extract::{Form, Query, State};
use axum::response::{Html, IntoResponse, Response};
use rust_decimal::Decimal;
use serde::Deserialize;

use crate::db::customers::{self, UpsertCustomer};
use crate::db::sep24_transactions;
use crate::error::{AppError, AppResult};
use crate::fee;
use crate::jwt::{self, InteractiveClaims};
use crate::kyc;
use crate::routes::{AppState, SharedState};

use super::worker;

#[derive(Template)]
#[template(path = "sep24_form.html")]
struct FormTemplate {
    action_title: String,
    submit_label: &'static str,
    transaction_id: String,
    token: String,
    asset_code: String,
    amount: String,
    missing_fields: Vec<MissingField>,
}

struct MissingField {
    name: String,
    description: String,
}

#[derive(Template)]
#[template(path = "sep24_message.html")]
struct MessageTemplate {
    heading: String,
    body: String,
    transaction_id: String,
    status: String,
    post_message: bool,
}

#[derive(Debug, Deserialize)]
pub struct InteractiveQuery {
    /// The anchor platform's own transaction id — this is *not* our internal
    /// row id. The platform mints this URL (and its `token`) itself when it
    /// answers the wallet's `POST /sep24/transactions/.../interactive`.
    transaction_id: String,
    token: String,
}

/// Verifies a token the platform signed with `SECRET_SEP24_INTERACTIVE_URL_JWT_SECRET`.
/// We never mint these ourselves in this architecture.
fn verify_interactive_token(
    state: &SharedState,
    transaction_id: &str,
    token: &str,
) -> AppResult<()> {
    let claims =
        jwt::decode_jwt::<InteractiveClaims>(token, &state.config.sep24_interactive_jwt_secret)
            .map_err(|e| AppError::NotAuthorized(format!("invalid interactive token: {e}")))?;
    if claims.transaction_id != transaction_id {
        return Err(AppError::NotAuthorized(
            "token does not match transaction_id".to_string(),
        ));
    }
    Ok(())
}

pub async fn show_form(
    State(state): State<SharedState>,
    Query(query): Query<InteractiveQuery>,
) -> AppResult<Response> {
    verify_interactive_token(&state, &query.transaction_id, &query.token)?;

    let tx = sep24_transactions::find_by_platform_id(&state.db, &query.transaction_id)
        .await
        .map_err(AppError::Db)?
        .ok_or_else(|| AppError::NotFound("transaction not found".to_string()))?;

    if tx.status != "received" {
        return Ok(render_message(
            "Already submitted",
            "This deposit/withdrawal has already been submitted.",
            &query.transaction_id,
            &tx.status,
            false,
        ));
    }

    let customer = customers::find_by_identity(&state.db, &tx.account, tx.memo.as_deref(), "sep24")
        .await
        .map_err(AppError::Db)?;
    let provided = customer
        .as_ref()
        .and_then(|c| c.fields.as_object().cloned())
        .unwrap_or_default();

    let missing_fields = kyc::required_fields("sep24")
        .into_iter()
        .filter(|spec| provided.get(spec.name).is_none_or(|v| v.is_null()))
        .map(|spec| MissingField {
            name: spec.name.to_string(),
            description: spec.description.to_string(),
        })
        .collect();

    let action_title = if tx.kind == "deposit" {
        "Complete your deposit"
    } else {
        "Complete your withdrawal"
    };

    let template = FormTemplate {
        action_title: action_title.to_string(),
        submit_label: if tx.kind == "deposit" {
            "I have sent the funds"
        } else {
            "Continue"
        },
        transaction_id: query.transaction_id,
        token: query.token,
        asset_code: tx.asset_code,
        amount: tx.amount_in.map(|a| a.to_string()).unwrap_or_default(),
        missing_fields,
    };

    Ok(Html(
        template
            .render()
            .map_err(|e| AppError::Internal(e.to_string()))?,
    )
    .into_response())
}

pub async fn submit_form(
    State(state): State<SharedState>,
    Form(params): Form<HashMap<String, String>>,
) -> AppResult<Response> {
    let transaction_id = params
        .get("transaction_id")
        .cloned()
        .ok_or_else(|| AppError::BadRequest("missing transaction_id".to_string()))?;
    let token = params
        .get("token")
        .ok_or_else(|| AppError::BadRequest("missing token".to_string()))?;

    verify_interactive_token(&state, &transaction_id, token)?;

    let tx = sep24_transactions::find_by_platform_id(&state.db, &transaction_id)
        .await
        .map_err(AppError::Db)?
        .ok_or_else(|| AppError::NotFound("transaction not found".to_string()))?;
    if tx.status != "received" {
        return Ok(render_message(
            "Already submitted",
            "This deposit/withdrawal has already been submitted.",
            &transaction_id,
            &tx.status,
            false,
        ));
    }

    let amount = params
        .get("amount")
        .and_then(|a| Decimal::from_str(a).ok())
        .ok_or_else(|| AppError::BadRequest("invalid amount".to_string()))?;

    let mut kyc_fields = serde_json::Map::new();
    for spec in kyc::required_fields("sep24") {
        if let Some(value) = params.get(spec.name) {
            kyc_fields.insert(
                spec.name.to_string(),
                serde_json::Value::String(value.clone()),
            );
        }
    }
    customers::upsert(
        &state.db,
        UpsertCustomer {
            id: None,
            account: &tx.account,
            memo: tx.memo.as_deref(),
            memo_type: tx.memo_type.as_deref(),
            customer_type: "sep24",
            owner_account: &tx.account,
            owner_memo: tx.memo.as_deref(),
            fields: serde_json::Value::Object(kyc_fields),
        },
    )
    .await
    .map_err(AppError::Db)?;

    let fee_amount = fee::flat_fee(amount);
    let amount_out = amount - fee_amount;
    sep24_transactions::submit_amount(&state.db, tx.id, amount, amount_out, fee_amount)
        .await
        .map_err(AppError::Db)?;

    if tx.kind == "deposit" {
        let app_state: Arc<AppState> = Arc::clone(&state);
        tokio::spawn(worker::process_deposit(app_state, tx.id));
        Ok(render_message(
            "Deposit submitted",
            "Your deposit is being processed. You can close this window; check your wallet for the incoming payment.",
            &transaction_id,
            "pending_user_transfer_start",
            true,
        ))
    } else {
        let body = format!(
            "Send exactly {amount} {asset} to account {account} with memo (id) {memo}. \
             Once received, your withdrawal will be completed automatically.",
            amount = amount,
            asset = tx.asset_code,
            account = state.ledger.distribution_account_id(),
            memo = tx.withdraw_memo.unwrap_or_default(),
        );
        Ok(render_message(
            "Send your funds",
            &body,
            &transaction_id,
            "pending_user_transfer_start",
            true,
        ))
    }
}

fn render_message(
    heading: &str,
    body: &str,
    transaction_id: &str,
    status: &str,
    post_message: bool,
) -> Response {
    let template = MessageTemplate {
        heading: heading.to_string(),
        body: body.to_string(),
        transaction_id: transaction_id.to_string(),
        status: status.to_string(),
        post_message,
    };
    match template.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => AppError::Internal(e.to_string()).into_response(),
    }
}
