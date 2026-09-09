use std::sync::Arc;

use rust_decimal::Decimal;
use stellar_base::memo::Memo;
use uuid::Uuid;

use crate::db::sep24_transactions;
use crate::event::amount_in_stellar;
use crate::ledger::Ledger;
use crate::routes::AppState;

pub async fn process_deposit(state: Arc<AppState>, transaction_id: Uuid) {
    if let Err(e) = try_process_deposit(&state, transaction_id).await {
        tracing::error!(%transaction_id, error = %e, "deposit processing failed");
        let _ = sep24_transactions::set_error(&state.db, transaction_id, &e).await;
        if let Ok(Some(tx)) = sep24_transactions::find_by_id(&state.db, transaction_id).await {
            let _ = state
                .platform_api
                .notify_transaction_error(&tx.platform_transaction_id, &e)
                .await;
        }
    }
}

async fn try_process_deposit(state: &AppState, transaction_id: Uuid) -> Result<(), String> {
    let tx = sep24_transactions::find_by_id(&state.db, transaction_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "transaction disappeared".to_string())?;

    let amount_out: Decimal = tx
        .amount_out
        .ok_or_else(|| "missing amount_out".to_string())?;
    let destination = Ledger::parse_account(&tx.account).map_err(|e| e.to_string())?;
    let asset = state.ledger.asset().map_err(|e| e.to_string())?;
    let memo = tx
        .memo_type
        .as_deref()
        .filter(|t| *t == "id")
        .and(tx.memo.as_deref())
        .and_then(|m| m.parse::<u64>().ok())
        .map(Memo::new_id);

    let hash = state
        .ledger
        .send_payment(destination, asset, amount_out, memo)
        .await
        .map_err(|e| e.to_string())?;

    sep24_transactions::set_completed(&state.db, transaction_id, &hash)
        .await
        .map_err(|e| e.to_string())?;

    state
        .platform_api
        .notify_onchain_funds_sent(&tx.platform_transaction_id, &hash, None)
        .await
        .map_err(|e| e.to_string())?;

    tracing::info!(%transaction_id, %hash, "deposit completed");
    Ok(())
}

/// Called by the shared payment observer when an incoming payment matches a
/// pending SEP-24 withdrawal's memo. Reports the on-chain receipt to the
/// platform, then — since this anchor has no real banking integration —
/// simulates/logs the fiat leg and reports that too.
pub async fn complete_withdrawal(
    state: Arc<AppState>,
    transaction_id: Uuid,
    incoming_tx_hash: &str,
) {
    let tx = match sep24_transactions::find_by_id(&state.db, transaction_id).await {
        Ok(Some(tx)) => tx,
        _ => return,
    };

    let amount_in = tx.amount_in.unwrap_or_default();
    if let Err(e) = state
        .platform_api
        .notify_onchain_funds_received(
            &tx.platform_transaction_id,
            incoming_tx_hash,
            amount_in_stellar(&state.ledger, amount_in),
        )
        .await
    {
        tracing::error!(%transaction_id, error = %e, "failed to notify onchain funds received");
        return;
    }

    if let Err(e) =
        sep24_transactions::set_status(&state.db, transaction_id, "awaiting_offchain_payout").await
    {
        tracing::error!(%transaction_id, error = %e, "failed to mark withdrawal awaiting_offchain_payout");
        return;
    }

    let external_reference = format!("SIMULATED-{}", Uuid::new_v4());
    tracing::info!(
        %transaction_id,
        account = %tx.account,
        amount = %amount_in,
        external_reference = %external_reference,
        "simulated: sending fiat payout to customer's bank details on file"
    );

    if let Err(e) =
        sep24_transactions::set_completed(&state.db, transaction_id, incoming_tx_hash).await
    {
        tracing::error!(%transaction_id, error = %e, "failed to mark withdrawal completed");
        return;
    }

    if let Err(e) = state
        .platform_api
        .notify_offchain_funds_sent(&tx.platform_transaction_id, &external_reference, None)
        .await
    {
        tracing::error!(%transaction_id, error = %e, "failed to notify offchain funds sent");
    }
}
