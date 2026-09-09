use std::sync::Arc;

use uuid::Uuid;

use crate::db::{customers, sep31_transactions};
use crate::event::amount_in_stellar;
use crate::kyc;
use crate::routes::AppState;

/// Returns `Ok(true)` when every required `sep31-receiver` KYC field has been
/// provided for `receiver_id`, `Ok(false)` (with the missing-fields message)
/// otherwise.
async fn receiver_kyc_complete(
    state: &AppState,
    receiver_id: &str,
) -> Result<(bool, String), String> {
    let Ok(id) = receiver_id.parse() else {
        return Ok((false, "invalid receiver_id".to_string()));
    };
    let customer = customers::find_by_id(&state.db, id)
        .await
        .map_err(|e| e.to_string())?;

    let provided = customer
        .as_ref()
        .and_then(|c| c.fields.as_object().cloned())
        .unwrap_or_default();

    let missing: Vec<&str> = kyc::required_fields("sep31-receiver")
        .into_iter()
        .filter(|spec| provided.get(spec.name).is_none_or(|v| v.is_null()))
        .map(|spec| spec.name)
        .collect();

    if missing.is_empty() {
        Ok((true, String::new()))
    } else {
        Ok((
            false,
            format!(
                "missing required receiver KYC fields: {}",
                missing.join(", ")
            ),
        ))
    }
}

async fn settle(
    state: &Arc<AppState>,
    transaction_id: Uuid,
    incoming_tx_hash: &str,
) -> Result<(), String> {
    let tx = sep31_transactions::find_by_id(&state.db, transaction_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "transaction disappeared".to_string())?;

    let external_reference = format!("SIMULATED-{}", Uuid::new_v4());
    tracing::info!(
        %transaction_id,
        receiver_id = ?tx.receiver_id,
        amount = ?tx.amount_out,
        external_reference = %external_reference,
        "simulated: sending fiat payout to receiver's bank details on file"
    );

    sep31_transactions::set_completed(&state.db, transaction_id, incoming_tx_hash)
        .await
        .map_err(|e| e.to_string())?;

    state
        .platform_api
        .notify_offchain_funds_sent(&tx.platform_transaction_id, &external_reference, None)
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}

/// Called by the shared payment observer when an incoming payment matches a
/// pending SEP-31 transaction's assigned memo.
pub async fn complete_receive(state: Arc<AppState>, transaction_id: Uuid, incoming_tx_hash: &str) {
    let tx = match sep31_transactions::find_by_id(&state.db, transaction_id).await {
        Ok(Some(tx)) => tx,
        _ => return,
    };

    if let Err(e) = state
        .platform_api
        .notify_onchain_funds_received(
            &tx.platform_transaction_id,
            incoming_tx_hash,
            amount_in_stellar(&state.ledger, tx.amount_in),
        )
        .await
    {
        tracing::error!(%transaction_id, error = %e, "failed to notify onchain funds received");
        return;
    }

    let Some(receiver_id) = &tx.receiver_id else {
        // No receiver KYC was configured for this asset; settle unconditionally.
        if let Err(e) = settle(&state, transaction_id, incoming_tx_hash).await {
            tracing::error!(%transaction_id, error = %e, "failed to settle sep31 transaction");
        }
        return;
    };

    match receiver_kyc_complete(&state, receiver_id).await {
        Ok((true, _)) => {
            if let Err(e) = settle(&state, transaction_id, incoming_tx_hash).await {
                tracing::error!(%transaction_id, error = %e, "failed to settle sep31 transaction");
            }
        }
        Ok((false, message)) => {
            if let Err(e) = sep31_transactions::set_required_info(
                &state.db,
                transaction_id,
                &message,
                incoming_tx_hash,
            )
            .await
            {
                tracing::error!(%transaction_id, error = %e, "failed to mark sep31 transaction needs-info");
            }
        }
        Err(e) => tracing::error!(%transaction_id, error = %e, "failed to check receiver kyc"),
    }
}

/// Re-checks one `pending_customer_info_update` SEP-31 transaction, resuming
/// settlement if its receiver's KYC is now complete. Called from the SEP-12
/// `PUT /customer` handler in place of a full event bus.
pub async fn resume_if_ready(state: Arc<AppState>, transaction_id: Uuid) {
    let tx = match sep31_transactions::find_by_id(&state.db, transaction_id).await {
        Ok(Some(tx)) => tx,
        _ => return,
    };
    let Some(receiver_id) = &tx.receiver_id else {
        return;
    };
    let Some(incoming_tx_hash) = &tx.stellar_transaction_id else {
        return;
    };

    match receiver_kyc_complete(&state, receiver_id).await {
        Ok((true, _)) => {
            if let Err(e) = settle(&state, transaction_id, incoming_tx_hash).await {
                tracing::error!(%transaction_id, error = %e, "failed to resume sep31 settlement");
            }
        }
        Ok((false, _)) => {}
        Err(e) => {
            tracing::error!(%transaction_id, error = %e, "failed to re-check receiver kyc")
        }
    }
}
