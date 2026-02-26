use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

pub async fn create_pool(connection_string: &str) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(20)
        .min_connections(2)
        .acquire_timeout(std::time::Duration::from_secs(5))
        .connect(connection_string)
        .await
}
