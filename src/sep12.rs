//! SEP-12 customer callbacks (`Platform -> Anchor`). The platform already
//! authenticated the wallet via SEP-10 on its own side before ever calling
//! us; `callback_auth::require_platform_auth` only proves the caller is the
//! platform itself, so the `account`/`memo`/`id` parameters below are
//! trusted as-is, same as `anchor-ms`'s `CustomerCallbackController`.

use std::collections::BTreeMap;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::callback_auth::require_platform_auth;
use crate::db::customers::{self, UpsertCustomer};
use crate::db::sep31_transactions;
use crate::error::{AppError, AppResult};
use crate::kyc;
use crate::routes::SharedState;

#[derive(Debug, Deserialize)]
pub struct GetCustomerQuery {
    id: Option<Uuid>,
    account: Option<String>,
    memo: Option<String>,
    #[serde(rename = "type")]
    customer_type: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct GetCustomerResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    status: &'static str,
    fields: BTreeMap<String, FieldOut>,
    provided_fields: BTreeMap<String, ProvidedFieldOut>,
}

#[derive(Debug, Serialize)]
struct FieldOut {
    #[serde(rename = "type")]
    field_type: &'static str,
    description: &'static str,
    optional: bool,
}

#[derive(Debug, Serialize)]
struct ProvidedFieldOut {
    #[serde(rename = "type")]
    field_type: &'static str,
    description: &'static str,
    status: &'static str,
}

pub async fn get_customer(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Query(query): Query<GetCustomerQuery>,
) -> AppResult<Json<GetCustomerResponse>> {
    require_platform_auth(&state, &headers)?;

    let customer_type = query
        .customer_type
        .clone()
        .unwrap_or_else(|| "sep24".to_string());

    let customer = if let Some(id) = query.id {
        customers::find_by_id(&state.db, id)
            .await
            .map_err(AppError::Db)?
    } else {
        let account = query
            .account
            .as_deref()
            .ok_or_else(|| AppError::BadRequest("missing 'account' parameter".to_string()))?;
        customers::find_by_identity(&state.db, account, query.memo.as_deref(), &customer_type)
            .await
            .map_err(AppError::Db)?
    };

    let required = kyc::required_fields(&customer_type);

    let (id, provided_values) = match &customer {
        Some(c) => (
            Some(c.id.to_string()),
            c.fields.as_object().cloned().unwrap_or_default(),
        ),
        None => (None, Map::new()),
    };

    let mut fields = BTreeMap::new();
    let mut provided_fields = BTreeMap::new();
    let mut needs_info = false;

    for spec in &required {
        if provided_values
            .get(spec.name)
            .filter(|v| !v.is_null())
            .is_some()
        {
            provided_fields.insert(
                spec.name.to_string(),
                ProvidedFieldOut {
                    field_type: spec.field_type,
                    description: spec.description,
                    status: "ACCEPTED",
                },
            );
        } else {
            needs_info = true;
            fields.insert(
                spec.name.to_string(),
                FieldOut {
                    field_type: spec.field_type,
                    description: spec.description,
                    optional: false,
                },
            );
        }
    }

    let status = if needs_info { "NEEDS_INFO" } else { "ACCEPTED" };

    Ok(Json(GetCustomerResponse {
        id,
        status,
        fields,
        provided_fields,
    }))
}

#[derive(Debug, Deserialize)]
pub struct PutCustomerRequest {
    id: Option<Uuid>,
    account: Option<String>,
    memo: Option<String>,
    memo_type: Option<String>,
    #[serde(rename = "type")]
    customer_type: Option<String>,
    #[serde(flatten)]
    kyc_fields: Map<String, Value>,
}

#[derive(Debug, Serialize)]
pub struct PutCustomerResponse {
    id: String,
}

pub async fn put_customer(
    State(state): State<SharedState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> AppResult<(StatusCode, Json<PutCustomerResponse>)> {
    require_platform_auth(&state, &headers)?;

    let request: PutCustomerRequest = serde_json::from_slice(&body)
        .map_err(|e| AppError::BadRequest(format!("invalid request body: {e}")))?;

    if let Some(id) = request.id {
        customers::find_by_id(&state.db, id)
            .await
            .map_err(AppError::Db)?
            .ok_or_else(|| AppError::NotFound("customer not found".to_string()))?;
    }

    let customer_type = request
        .customer_type
        .clone()
        .unwrap_or_else(|| "sep24".to_string());
    let account = request
        .account
        .clone()
        .ok_or_else(|| AppError::BadRequest("missing 'account' parameter".to_string()))?;

    let id = customers::upsert(
        &state.db,
        UpsertCustomer {
            id: request.id,
            account: &account,
            memo: request.memo.as_deref(),
            memo_type: request.memo_type.as_deref(),
            customer_type: &customer_type,
            owner_account: &account,
            owner_memo: request.memo.as_deref(),
            fields: json!(request.kyc_fields),
        },
    )
    .await
    .map_err(AppError::Db)?;

    if customer_type == "sep31-receiver" {
        let app_state = std::sync::Arc::clone(&state);
        let receiver_id = id.to_string();
        tokio::spawn(async move {
            resume_pending_sep31_transactions(app_state, receiver_id).await;
        });
    }

    Ok((
        StatusCode::ACCEPTED,
        Json(PutCustomerResponse { id: id.to_string() }),
    ))
}

/// Re-checks any `pending_customer_info_update` SEP-31 transaction for
/// `receiver_id`, resuming settlement if KYC is now complete. Called in
/// place of a full event bus, same pattern the original single-process
/// anchor used.
async fn resume_pending_sep31_transactions(state: SharedState, receiver_id: String) {
    let pending = match sep31_transactions::find_pending_info_by_receiver(&state.db, &receiver_id)
        .await
    {
        Ok(rows) => rows,
        Err(e) => {
            tracing::error!(error = %e, %receiver_id, "failed to look up pending sep31 transactions");
            return;
        }
    };

    for tx in pending {
        crate::sep31::worker::resume_if_ready(state.clone(), tx.id).await;
    }
}

#[derive(Debug, Deserialize)]
pub struct DeleteCustomerQuery {
    memo: Option<String>,
}

pub async fn delete_customer(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path(account): Path<String>,
    Query(query): Query<DeleteCustomerQuery>,
) -> AppResult<StatusCode> {
    require_platform_auth(&state, &headers)?;

    let deleted = customers::delete(&state.db, &account, query.memo.as_deref())
        .await
        .map_err(AppError::Db)?;

    if deleted == 0 {
        return Err(AppError::NotFound("customer not found".to_string()));
    }

    Ok(StatusCode::OK)
}
