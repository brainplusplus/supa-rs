use crate::config::Config;
use crate::db::{embed::EmbeddedPostgres, pool::create_pool};
use axum::http::Request;
use axum::Router;
use tower::ServiceBuilder;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::trace::TraceLayer;

const PID_FILE: &str = ".suparust.pid";

pub async fn cmd_start_foreground() {
    let cfg = crate::config::Config::from_env();
    crate::tracing::init_tracing(&cfg.log_level, &cfg.log_format, crate::tracing::TracingWriter::Stdout);

    let pid = std::process::id();
    std::fs::write(PID_FILE, pid.to_string()).ok();
    tracing::info!("PID {} written to {}", pid, PID_FILE);

    if let Err(e) = run_server().await {
        let msg = e.to_string();
        if msg.contains("10048") || msg.contains("address in use") || msg.contains("Address already in use") {
            dotenvy::dotenv().ok();
            let port = std::env::var("SUPARUST_PORT")
                .or_else(|_| std::env::var("PORT"))
                .unwrap_or_else(|_| "3000".to_string());
            tracing::error!(
                "Port {} is already in use. Run `suparust stop` to kill the existing process.",
                port
            );
        } else {
            tracing::error!("Server error: {}", e);
        }
    }

    std::fs::remove_file(PID_FILE).ok();
}

pub async fn cmd_start_daemon() {
    // Check if already running
    if let Ok(pid_str) = std::fs::read_to_string(PID_FILE) {
        let pid: u32 = pid_str.trim().parse().unwrap_or(0);
        if pid > 0 {
            dotenvy::dotenv().ok();
            let port = std::env::var("SUPARUST_PORT").unwrap_or_else(|_| "3000".to_string());
            let addr = format!("127.0.0.1:{}", port);
            let alive = std::net::TcpStream::connect_timeout(
                &addr.parse().unwrap_or_else(|_| "127.0.0.1:3000".parse().unwrap()),
                std::time::Duration::from_secs(1),
            )
            .is_ok();
            if alive {
                println!("Already running (PID {})", pid);
                std::process::exit(1);
            }
        }
    }

    // Get path to current binary
    let exe = std::env::current_exe().expect("Cannot determine executable path");

    // Spawn daemon child: same binary with --daemon-child flag
    let child = std::process::Command::new(&exe)
        .args(["start", "--daemon-child"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("Failed to spawn daemon child");

    let child_pid = child.id();
    std::fs::write(PID_FILE, child_pid.to_string()).expect("Failed to write PID file");

    println!("Started SupaRust daemon (PID {})", child_pid);
    println!("Logs: app.log");
}

pub async fn cmd_start_daemon_child() {
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("app.log")
        .expect("Cannot open app.log");

    let cfg = crate::config::Config::from_env();
    crate::tracing::init_tracing(&cfg.log_level, &cfg.log_format, crate::tracing::TracingWriter::File(log_file));

    tracing::info!("SupaRust daemon child started (PID {})", std::process::id());

    if let Err(e) = run_server().await {
        tracing::error!("Server error: {}", e);
    }
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
        ))
        .layer(
            ServiceBuilder::new()
                .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
                .layer(PropagateRequestIdLayer::x_request_id())
                .layer(
                    TraceLayer::new_for_http()
                        .make_span_with(|req: &Request<_>| {
                            let req_id = req
                                .headers()
                                .get("x-request-id")
                                .and_then(|v| v.to_str().ok())
                                .unwrap_or("unknown");
                            tracing::info_span!(
                                "http_request",
                                req_id = %req_id,
                                method = %req.method(),
                                path   = %req.uri().path(),
                            )
                        }),
                ),
        );

    let addr = format!("0.0.0.0:{}", cfg.port);
    tracing::info!("SupaRust listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            tokio::signal::ctrl_c().await.ok();
            tracing::info!("Ctrl+C received — shutting down gracefully...");
        })
        .await?;

    tracing::info!("HTTP server stopped. Cleaning up...");
    // _embedded drops here → EmbeddedPostgres::drop() calls stop_db()
    Ok(())
}
