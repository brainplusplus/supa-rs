pub mod config;
pub mod api;
pub mod db;
pub mod parser;
pub mod sql;

use config::Config;
use db::{embed::EmbeddedPostgres, pool::create_pool};
use axum::Router;
use tracing_subscriber;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let cfg = Config::from_env();

    // Resolve connection string: external DB atau embedded
    let (conn_str, _embedded) = match cfg.database_url {
        Some(url) => {
            tracing::info!("Using external PostgreSQL: {}", url);
            (url.clone(), None)
        }
        None => {
            tracing::info!("Starting embedded PostgreSQL in {}", cfg.data_dir);
            let embedded = EmbeddedPostgres::start(&cfg.data_dir).await?;
            let cs = embedded.connection_string.clone();
            (cs, Some(embedded))
        }
    };

    // Build connection pool
    let pool = create_pool(&conn_str).await?;
    tracing::info!("Database pool established");

    // Run migrations
    sqlx::migrate!("./migrations").run(&pool).await?;
    tracing::info!("Migrations complete");

    // Build Axum router (placeholder routes untuk Task 2+)
    let app = Router::new()
        .nest("/rest/v1",    api::rest::router(pool.clone()))
        .nest("/auth/v1",    api::auth::router(pool.clone(), cfg.jwt_secret.clone()))
        .nest("/storage/v1", api::storage::router(
            pool.clone(),
            cfg.storage_root.clone(),
            cfg.jwt_secret.clone(),
        ));

    let addr = format!("0.0.0.0:{}", cfg.port);
    tracing::info!("SupaRust listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
