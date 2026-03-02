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
    pub async fn start(data_dir: &str) -> Result<Self, Box<dyn std::error::Error>> {
        tracing::info!("Setting up embedded PostgreSQL (first run downloads binary ~50MB)...");

        let make_settings = || {
            (
                PgSettings {
                    database_dir: PathBuf::from(data_dir),
                    port: 5433,
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

        // Phase 1: retry up to 3 times for transient Windows file-lock issues.
        let mut result = None;
        for attempt in 1..=3u8 {
            let (ps, fs) = make_settings();
            match PgEmbed::new(ps, fs).await {
                Ok(p) => { result = Some(p); break; }
                Err(e) => {
                    tracing::warn!("pg-embed init error (attempt {attempt}/3): {e}");
                    if attempt < 3 {
                        tracing::warn!(
                            "pg-embed setup failed (attempt {attempt}/3), retrying in 5s... \
                             On Windows: ensure antivirus is not blocking %LOCALAPPDATA%\\pg-embed"
                        );
                        tokio::time::sleep(Duration::from_secs(5)).await;
                    }
                }
            }
        }

        // Phase 2: all retries failed — likely corrupt cache. Clear it and try once more.
        let mut pg = if let Some(p) = result {
            p
        } else {
            tracing::warn!(
                "All 3 attempts failed — cache may be corrupt. \
                 Clearing %LOCALAPPDATA%\\pg-embed and retrying once..."
            );
            clear_pg_embed_cache();
            tokio::time::sleep(Duration::from_secs(2)).await;
            let (ps, fs) = make_settings();
            PgEmbed::new(ps, fs).await.map_err(|e| {
                tracing::error!(
                    "pg-embed failed after cache clear. \
                     Try disabling antivirus for %LOCALAPPDATA%\\pg-embed, then run: cargo run -- start"
                );
                Box::<dyn std::error::Error>::from(e.to_string())
            })?
        };

        // setup() runs initdb — skip if data dir already initialized
        let pg_version_file = PathBuf::from(data_dir).join("PG_VERSION");
        if pg_version_file.exists() {
            tracing::info!("Existing data directory detected, skipping initdb");
        } else {
            pg.setup().await?;
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
