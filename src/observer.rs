use std::sync::Arc;

use futures::StreamExt;
use stellar_horizon::api::Join;
use stellar_horizon::api::payments;
use stellar_horizon::client::HorizonClient;
use stellar_horizon::resources::operation::Payment as PaymentResource;

use crate::db::{sep24_transactions, sep31_transactions};
use crate::routes::AppState;
use crate::{sep24, sep31};

/// Streams incoming payments to the distribution account and drives forward
/// any SEP-24 withdrawal or SEP-31 receive transaction whose assigned memo
/// matches. This is the single long-running task that plays the role
/// anchor-platform's stellar-observer server plays, collapsed into this
/// anchor's own process since there is no callback boundary to cross.
pub async fn run(state: Arc<AppState>) {
    loop {
        if let Err(e) = stream_once(state.clone()).await {
            tracing::error!(error = %e, "payment observer stream ended, retrying in 5s");
        }
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }
}

async fn stream_once(state: Arc<AppState>) -> Result<(), String> {
    let request = payments::for_account(&state.ledger.distribution_public_key())
        .with_join(Join::Transactions);

    let mut stream = state
        .ledger
        .horizon_client()
        .stream(request)
        .map_err(|e| e.to_string())?;

    tracing::info!("payment observer stream connected");

    while let Some(item) = stream.next().await {
        let payment = match item {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(error = %e, "payment observer stream item error");
                continue;
            }
        };

        let PaymentResource::Payment(op) = payment else {
            continue;
        };

        if op.to != state.ledger.distribution_account_id() {
            continue;
        }
        let matches_asset = op.asset.asset_code.as_deref()
            == Some(state.config.asset_code.as_str())
            && op.asset.asset_issuer.as_deref()
                == Some(state.ledger.distribution_account_id().as_str());
        if !matches_asset {
            continue;
        }

        let Some(memo) = op.base.transaction.as_ref().and_then(|t| t.memo.clone()) else {
            continue;
        };

        handle_incoming_payment(&state, &memo, &op.base.transaction_hash).await;
    }

    Ok(())
}

async fn handle_incoming_payment(state: &Arc<AppState>, memo: &str, tx_hash: &str) {
    let asset_code = state.config.asset_code.clone();

    match sep24_transactions::find_by_withdraw_memo(&state.db, memo, &asset_code).await {
        Ok(Some(tx)) => {
            tracing::info!(transaction_id = %tx.id, %memo, "matched incoming payment to SEP-24 withdrawal");
            sep24::worker::complete_withdrawal(state.clone(), tx.id, tx_hash).await;
            return;
        }
        Ok(None) => {}
        Err(e) => tracing::error!(error = %e, "failed to look up sep24 withdrawal by memo"),
    }

    match sep31_transactions::find_by_stellar_memo(&state.db, memo, &asset_code).await {
        Ok(Some(tx)) => {
            tracing::info!(transaction_id = %tx.id, %memo, "matched incoming payment to SEP-31 transaction");
            sep31::worker::complete_receive(state.clone(), tx.id, tx_hash).await;
        }
        Ok(None) => {
            tracing::debug!(%memo, "incoming payment matched no pending transaction");
        }
        Err(e) => tracing::error!(error = %e, "failed to look up sep31 transaction by memo"),
    }
}
