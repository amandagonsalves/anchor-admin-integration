use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

/// Our own mirror of a SEP-24 transaction the platform told us about via a
/// `transaction_created` event. The platform's own store is the
/// externally-visible source of truth (wallets poll it, not us); this row
/// only carries what our custody workers and the interactive form need.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Sep24Transaction {
    pub id: Uuid,
    pub platform_transaction_id: String,
    pub kind: String,
    pub status: String,
    pub account: String,
    pub memo: Option<String>,
    pub memo_type: Option<String>,
    pub asset_code: String,
    pub amount_in: Option<Decimal>,
    pub amount_out: Option<Decimal>,
    pub amount_fee: Option<Decimal>,
    pub stellar_transaction_id: Option<String>,
    pub message: Option<String>,
    pub withdraw_memo: Option<String>,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

pub struct NewTransaction<'a> {
    pub platform_transaction_id: &'a str,
    pub kind: &'a str,
    pub account: &'a str,
    pub memo: Option<&'a str>,
    pub memo_type: Option<&'a str>,
    pub asset_code: &'a str,
    pub amount_in: Option<Decimal>,
    pub withdraw_memo: Option<&'a str>,
}

pub async fn create(pool: &PgPool, tx: NewTransaction<'_>) -> Result<Uuid, sqlx::Error> {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO sep24_transactions \
         (id, platform_transaction_id, kind, status, account, memo, memo_type, asset_code, \
          amount_in, withdraw_memo) \
         VALUES ($1, $2, $3, 'received', $4, $5, $6, $7, $8, $9)",
    )
    .bind(id)
    .bind(tx.platform_transaction_id)
    .bind(tx.kind)
    .bind(tx.account)
    .bind(tx.memo)
    .bind(tx.memo_type)
    .bind(tx.asset_code)
    .bind(tx.amount_in)
    .bind(tx.withdraw_memo)
    .execute(pool)
    .await?;
    Ok(id)
}

pub async fn find_by_id(pool: &PgPool, id: Uuid) -> Result<Option<Sep24Transaction>, sqlx::Error> {
    sqlx::query_as::<_, Sep24Transaction>(
        "SELECT id, platform_transaction_id, kind, status, account, memo, memo_type, asset_code, \
         amount_in, amount_out, amount_fee, stellar_transaction_id, message, withdraw_memo, \
         started_at, completed_at \
         FROM sep24_transactions WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
}

pub async fn find_by_platform_id(
    pool: &PgPool,
    platform_transaction_id: &str,
) -> Result<Option<Sep24Transaction>, sqlx::Error> {
    sqlx::query_as::<_, Sep24Transaction>(
        "SELECT id, platform_transaction_id, kind, status, account, memo, memo_type, asset_code, \
         amount_in, amount_out, amount_fee, stellar_transaction_id, message, withdraw_memo, \
         started_at, completed_at \
         FROM sep24_transactions WHERE platform_transaction_id = $1",
    )
    .bind(platform_transaction_id)
    .fetch_optional(pool)
    .await
}

pub async fn find_by_withdraw_memo(
    pool: &PgPool,
    memo: &str,
    asset_code: &str,
) -> Result<Option<Sep24Transaction>, sqlx::Error> {
    sqlx::query_as::<_, Sep24Transaction>(
        "SELECT id, platform_transaction_id, kind, status, account, memo, memo_type, asset_code, \
         amount_in, amount_out, amount_fee, stellar_transaction_id, message, withdraw_memo, \
         started_at, completed_at \
         FROM sep24_transactions \
         WHERE withdraw_memo = $1 AND asset_code = $2 AND status = 'awaiting_stellar_payment'",
    )
    .bind(memo)
    .bind(asset_code)
    .fetch_optional(pool)
    .await
}

pub async fn submit_amount(
    pool: &PgPool,
    id: Uuid,
    amount_in: Decimal,
    amount_out: Decimal,
    amount_fee: Decimal,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE sep24_transactions SET amount_in = $2, amount_out = $3, amount_fee = $4, \
         status = 'awaiting_stellar_payment' WHERE id = $1",
    )
    .bind(id)
    .bind(amount_in)
    .bind(amount_out)
    .bind(amount_fee)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn set_status(pool: &PgPool, id: Uuid, status: &str) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE sep24_transactions SET status = $2 WHERE id = $1")
        .bind(id)
        .bind(status)
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
        "UPDATE sep24_transactions SET status = 'completed', stellar_transaction_id = $2, \
         completed_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(stellar_transaction_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn set_error(pool: &PgPool, id: Uuid, message: &str) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE sep24_transactions SET status = 'error', message = $2 WHERE id = $1")
        .bind(id)
        .bind(message)
        .execute(pool)
        .await?;
    Ok(())
}
