use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Customer {
    pub id: Uuid,
    pub account: String,
    pub memo: Option<String>,
    pub memo_type: Option<String>,
    pub customer_type: String,
    pub owner_account: String,
    pub owner_memo: Option<String>,
    pub fields: Value,
}

pub async fn find_by_id(pool: &PgPool, id: Uuid) -> Result<Option<Customer>, sqlx::Error> {
    sqlx::query_as::<_, Customer>(
        "SELECT id, account, memo, memo_type, customer_type, owner_account, owner_memo, fields \
         FROM customers WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
}

pub async fn find_by_identity(
    pool: &PgPool,
    account: &str,
    memo: Option<&str>,
    customer_type: &str,
) -> Result<Option<Customer>, sqlx::Error> {
    sqlx::query_as::<_, Customer>(
        "SELECT id, account, memo, memo_type, customer_type, owner_account, owner_memo, fields \
         FROM customers WHERE account = $1 AND memo IS NOT DISTINCT FROM $2 AND customer_type = $3",
    )
    .bind(account)
    .bind(memo)
    .bind(customer_type)
    .fetch_optional(pool)
    .await
}

pub struct UpsertCustomer<'a> {
    pub id: Option<Uuid>,
    pub account: &'a str,
    pub memo: Option<&'a str>,
    pub memo_type: Option<&'a str>,
    pub customer_type: &'a str,
    pub owner_account: &'a str,
    pub owner_memo: Option<&'a str>,
    pub fields: Value,
}

pub async fn upsert(pool: &PgPool, request: UpsertCustomer<'_>) -> Result<Uuid, sqlx::Error> {
    if let Some(id) = request.id {
        sqlx::query("UPDATE customers SET fields = fields || $2, updated_at = now() WHERE id = $1")
            .bind(id)
            .bind(&request.fields)
            .execute(pool)
            .await?;
        return Ok(id);
    }

    if let Some(existing) =
        find_by_identity(pool, request.account, request.memo, request.customer_type).await?
    {
        sqlx::query("UPDATE customers SET fields = fields || $2, updated_at = now() WHERE id = $1")
            .bind(existing.id)
            .bind(&request.fields)
            .execute(pool)
            .await?;
        return Ok(existing.id);
    }

    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO customers (id, account, memo, memo_type, customer_type, owner_account, owner_memo, fields) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(id)
    .bind(request.account)
    .bind(request.memo)
    .bind(request.memo_type)
    .bind(request.customer_type)
    .bind(request.owner_account)
    .bind(request.owner_memo)
    .bind(&request.fields)
    .execute(pool)
    .await?;

    Ok(id)
}

pub async fn delete(pool: &PgPool, account: &str, memo: Option<&str>) -> Result<u64, sqlx::Error> {
    let result =
        sqlx::query("DELETE FROM customers WHERE account = $1 AND memo IS NOT DISTINCT FROM $2")
            .bind(account)
            .bind(memo)
            .execute(pool)
            .await?;
    Ok(result.rows_affected())
}
