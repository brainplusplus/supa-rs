use crate::config::Config;
use crate::db::{embed::EmbeddedPostgres, pool::create_pool};
use axum::Router;

const PID_FILE: &str = ".suparust.pid";

pub async fn cmd_start_foreground() {
    tracing_subscriber::fmt::init();

    let pid = std::process::id();
    std::fs::write(PID_FILE, pid.to_string()).ok();
    tracing::info!("PID {} written to {}", pid, PID_FILE);

    if let Err(e) = run_server().await {
        tracing::error!("Server error: {}", e);
    }

    std::fs::remove_file(PID_FILE).ok();
}

pub async fn cmd_start_daemon() {
    todo!("daemon start")
}

pub async fn cmd_start_daemon_child() {
    todo!("daemon child")
}

async fn run_server() -> Result<(), Box<dyn std::error::Error>> {
    let cfg = Config::from_env();

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

    let pool = create_pool(&conn_str).await?;
    tracing::info!("Database pool established");

    sqlx::migrate!("./migrations").run(&pool).await?;
    tracing::info!("Migrations complete");

    let app = Router::new()
        .nest("/rest/v1",    crate::api::rest::router(pool.clone(), cfg.jwt_secret.clone()))
        .nest("/auth/v1",    crate::api::auth::router(pool.clone(), cfg.jwt_secret.clone()))
        .nest("/storage/v1", crate::api::storage::router(
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
