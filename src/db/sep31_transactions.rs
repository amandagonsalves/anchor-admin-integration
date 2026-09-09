use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

/// Our own mirror of a SEP-31 transaction the platform told us about via a
/// `transaction_created` event. See the comment on `sep24_transactions` for
/// why this only carries what our workers need, not the full wallet-visible
/// record (the platform owns that).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Sep31Transaction {
    pub id: Uuid,
    pub platform_transaction_id: String,
    pub status: String,
    pub creator_account: String,
    pub creator_memo: Option<String>,
    pub asset_code: String,
    pub amount_in: Decimal,
    pub amount_out: Option<Decimal>,
    pub fee: Option<Decimal>,
    pub sender_id: Option<String>,
    pub receiver_id: Option<String>,
    pub stellar_memo: String,
    pub stellar_memo_type: String,
    pub required_info_message: Option<String>,
    pub stellar_transaction_id: Option<String>,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

pub struct NewTransaction<'a> {
    pub platform_transaction_id: &'a str,
    pub creator_account: &'a str,
    pub creator_memo: Option<&'a str>,
    pub asset_code: &'a str,
    pub amount_in: Decimal,
    pub amount_out: Option<Decimal>,
    pub fee: Option<Decimal>,
    pub sender_id: Option<&'a str>,
    pub receiver_id: Option<&'a str>,
    pub stellar_memo: &'a str,
}

pub async fn create(pool: &PgPool, tx: NewTransaction<'_>) -> Result<Uuid, sqlx::Error> {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO sep31_transactions \
         (id, platform_transaction_id, status, creator_account, creator_memo, asset_code, \
          amount_in, amount_out, fee, sender_id, receiver_id, stellar_memo, stellar_memo_type) \
         VALUES ($1, $2, 'pending_receiver', $3, $4, $5, $6, $7, $8, $9, $10, $11, 'id')",
    )
    .bind(id)
    .bind(tx.platform_transaction_id)
    .bind(tx.creator_account)
    .bind(tx.creator_memo)
    .bind(tx.asset_code)
    .bind(tx.amount_in)
    .bind(tx.amount_out)
    .bind(tx.fee)
    .bind(tx.sender_id)
    .bind(tx.receiver_id)
    .bind(tx.stellar_memo)
    .execute(pool)
    .await?;
    Ok(id)
}

pub async fn find_by_id(pool: &PgPool, id: Uuid) -> Result<Option<Sep31Transaction>, sqlx::Error> {
    sqlx::query_as::<_, Sep31Transaction>(
        "SELECT id, platform_transaction_id, status, creator_account, creator_memo, asset_code, \
         amount_in, amount_out, fee, sender_id, receiver_id, stellar_memo, stellar_memo_type, \
         required_info_message, stellar_transaction_id, started_at, completed_at \
         FROM sep31_transactions WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
}

pub async fn find_by_stellar_memo(
    pool: &PgPool,
    memo: &str,
    asset_code: &str,
) -> Result<Option<Sep31Transaction>, sqlx::Error> {
    sqlx::query_as::<_, Sep31Transaction>(
        "SELECT id, platform_transaction_id, status, creator_account, creator_memo, asset_code, \
         amount_in, amount_out, fee, sender_id, receiver_id, stellar_memo, stellar_memo_type, \
         required_info_message, stellar_transaction_id, started_at, completed_at \
         FROM sep31_transactions \
         WHERE stellar_memo = $1 AND asset_code = $2 AND status = 'pending_receiver'",
    )
    .bind(memo)
    .bind(asset_code)
    .fetch_optional(pool)
    .await
}

pub async fn set_status(pool: &PgPool, id: Uuid, status: &str) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE sep31_transactions SET status = $2 WHERE id = $1")
        .bind(id)
        .bind(status)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn set_required_info(
    pool: &PgPool,
    id: Uuid,
    message: &str,
    incoming_tx_hash: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE sep31_transactions SET status = 'pending_customer_info_update', \
         required_info_message = $2, stellar_transaction_id = $3 WHERE id = $1",
    )
    .bind(id)
    .bind(message)
    .bind(incoming_tx_hash)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn set_completed(
    pool: &PgPool,
    id: Uuid,
    stellar_transaction_id: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE sep31_transactions SET status = 'completed', stellar_transaction_id = $2, \
         completed_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(stellar_transaction_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Transactions still waiting on receiver KYC, for a given `receiver_id`.
/// Used to resume settlement from the SEP-12 `PUT /customer` handler.
pub async fn find_pending_info_by_receiver(
    pool: &PgPool,
    receiver_id: &str,
) -> Result<Vec<Sep31Transaction>, sqlx::Error> {
    sqlx::query_as::<_, Sep31Transaction>(
        "SELECT id, platform_transaction_id, status, creator_account, creator_memo, asset_code, \
         amount_in, amount_out, fee, sender_id, receiver_id, stellar_memo, stellar_memo_type, \
         required_info_message, stellar_transaction_id, started_at, completed_at \
         FROM sep31_transactions \
         WHERE receiver_id = $1 AND status = 'pending_customer_info_update'",
    )
    .bind(receiver_id)
    .fetch_all(pool)
    .await
}
