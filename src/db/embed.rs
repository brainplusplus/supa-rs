use pg_embed::pg_enums::PgAuthMethod;
use pg_embed::pg_fetch::{PgFetchSettings, PG_V15};
use pg_embed::postgres::{PgEmbed, PgSettings};
use std::path::PathBuf;
use std::time::Duration;

pub struct EmbeddedPostgres {
    pg: PgEmbed,
    pub connection_string: String,
}

/// Returns the pg-embed binary cache directory — cross-platform, no extra crates.
///
/// | Platform | Path                                         |
/// |----------|----------------------------------------------|
/// | Windows  | `%LOCALAPPDATA%\pg-embed`                    |
/// | macOS    | `$HOME/Library/Caches/pg-embed`              |
/// | Linux    | `$XDG_CACHE_HOME/pg-embed` or `~/.cache/pg-embed` |
fn pg_embed_cache_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var("LOCALAPPDATA").ok().map(|d| PathBuf::from(d).join("pg-embed"))
    }
    #[cfg(target_os = "macos")]
    {
        std::env::var("HOME").ok().map(|d| PathBuf::from(d).join("Library").join("Caches").join("pg-embed"))
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        // XDG_CACHE_HOME first, fallback to ~/.cache
        let base = std::env::var("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                std::env::var("HOME").map(|h| PathBuf::from(h).join(".cache")).unwrap_or_else(|_| PathBuf::from("/tmp"))
            });
        Some(base.join("pg-embed"))
    }
}

/// Clear the pg-embed binary cache. Called when all retry attempts fail,
/// indicating a corrupt cache rather than a transient lock issue.
fn clear_pg_embed_cache() {
    if let Some(cache) = pg_embed_cache_dir() {
        if cache.exists() {
            match std::fs::remove_dir_all(&cache) {
                Ok(()) => tracing::info!("Cleared pg-embed cache at {}", cache.display()),
                Err(e) => tracing::warn!("Could not clear pg-embed cache {}: {}", cache.display(), e),
            }
        }
    }
}

impl EmbeddedPostgres {
    pub async fn start(data_dir: &str, pg_port: u16) -> Result<Self, Box<dyn std::error::Error>> {
        tracing::info!("Setting up embedded PostgreSQL (first run downloads binary ~50MB)...");

        // Pre-flight: wait for pg_port to be free (orphan from crash/kill may still hold it).
        // Poll up to 10s (50 × 200ms) before proceeding — avoids setup() failing due to port conflict.
        for i in 0..50u8 {
            let free = tokio::net::TcpListener::bind(format!("127.0.0.1:{}", pg_port))
                .await
                .is_ok();
            if free { break; }
            if i == 0 {
                tracing::warn!(
                    "pg port {} still occupied — waiting for orphan process to release \
                     (up to 10s). If this persists, kill the process holding that port.",
                    pg_port
                );
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
        }        let make_settings = || {
            (
                PgSettings {
                    database_dir: PathBuf::from(data_dir),
                    port: pg_port,
                    user: "postgres".to_string(),
                    password: "postgres".to_string(),
                    auth_method: PgAuthMethod::Plain,
                    persistent: true,
                    timeout: Some(Duration::from_secs(300)),
                    migration_dir: None,
                },
                PgFetchSettings { version: PG_V15, ..Default::default() },
            )
        };

        // Phase 1: retry new() + setup() together up to 3 times.
        // setup() is where pg-embed actually unpacks the binary — that's the failure point.
        // Transient Windows file-lock/antivirus issues can cause either step to fail.
        let pg_version_file = PathBuf::from(data_dir).join("PG_VERSION");
        let already_init = pg_version_file.exists();

        let mut result: Option<PgEmbed> = None;
        for attempt in 1..=3u8 {
            let (ps, fs) = make_settings();
            let new_result = PgEmbed::new(ps, fs).await;
            match new_result {
                Err(e) => {
                    tracing::warn!("pg-embed new() error (attempt {attempt}/3): {e}");
                }
                Ok(mut p) => {
                    if already_init {
                        result = Some(p);
                        break;
                    }
                    match p.setup().await {
                        Ok(()) => { result = Some(p); break; }
                        Err(e) => {
                            tracing::warn!("pg-embed setup() error (attempt {attempt}/3): {e}");
                        }
                    }
                }
            }
            if attempt < 3 {
                tracing::warn!(
                    "Retrying in 5s... On Windows: ensure antivirus is not blocking %LOCALAPPDATA%\\pg-embed"
                );
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }

        // Phase 2: all retries failed — likely corrupt cache. Clear it and try once more.
        let mut pg = if let Some(p) = result {
            p
        } else {
            tracing::warn!(
                "All 3 attempts failed — cache may be corrupt. \
                 Clearing pg-embed system cache and retrying once..."
            );
            clear_pg_embed_cache();
            tokio::time::sleep(Duration::from_secs(2)).await;
            let (ps, fs) = make_settings();
            let mut p = PgEmbed::new(ps, fs).await.map_err(|e| {
                tracing::error!(
                    "pg-embed new() failed after cache clear. \
                     Try disabling antivirus for %LOCALAPPDATA%\\pg-embed, then run: cargo run -- start"
                );
                Box::<dyn std::error::Error>::from(e.to_string())
            })?;
            if !already_init {
                p.setup().await.map_err(|e| {
                    tracing::error!(
                        "pg-embed setup() failed after cache clear. \
                         Try disabling antivirus for %LOCALAPPDATA%\\pg-embed, then run: cargo run -- start"
                    );
                    Box::<dyn std::error::Error>::from(e.to_string())
                })?;
            }
            p
        };

        if already_init {
            tracing::info!("Existing data directory detected, skipping initdb");
        }

        tracing::info!("Starting embedded PostgreSQL in {}", data_dir);
        pg.start_db().await?;

        // "postgres" database is created by initdb automatically — no need to create it
        let conn_str = pg.full_db_uri("postgres");
        tracing::info!("Embedded PostgreSQL ready at {}", conn_str);

        Ok(Self { pg, connection_string: conn_str })
    }
}

impl Drop for EmbeddedPostgres {
    fn drop(&mut self) {
        if tokio::runtime::Handle::try_current().is_ok() {
            let pg = &mut self.pg;
            tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(async {
                    let _ = pg.stop_db().await;
                });
            });
        }
    }
}
