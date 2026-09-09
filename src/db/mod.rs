use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;

pub mod customers;
pub mod sep24_transactions;
pub mod sep31_transactions;

pub async fn connect(database_url: &str) -> Result<PgPool, sqlx::Error> {
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(database_url)
        .await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}
