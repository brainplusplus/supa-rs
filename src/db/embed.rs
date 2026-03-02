use pg_embed::pg_enums::PgAuthMethod;
use pg_embed::pg_fetch::{PgFetchSettings, PG_V15};
use pg_embed::postgres::{PgEmbed, PgSettings};
use std::path::PathBuf;
use std::time::Duration;

pub struct EmbeddedPostgres {
    pg: PgEmbed,
    pub connection_string: String,
}

impl EmbeddedPostgres {
    pub async fn start(data_dir: &str) -> Result<Self, Box<dyn std::error::Error>> {
        tracing::info!("Setting up embedded PostgreSQL (first run downloads binary ~50MB)...");

        // Retry up to 3 times — Windows antivirus / file locking can cause
        // transient failures during zip extraction on first run.
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

        let mut pg = {
            let mut last_err: Box<dyn std::error::Error> = "pg-embed init failed".into();
            let mut result = None;
            for attempt in 1..=3u8 {
                let (ps, fs) = make_settings();
                match PgEmbed::new(ps, fs).await {
                    Ok(p) => { result = Some(p); break; }
                    Err(e) => {
                        last_err = e.into();
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
            match result {
                Some(p) => p,
                None => {
                    tracing::error!(
                        "pg-embed failed after 3 attempts. \
                         On Windows: delete %LOCALAPPDATA%\\pg-embed and temporarily disable antivirus, \
                         then run: cargo run -- start"
                    );
                    return Err(last_err);
                }
            }
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
