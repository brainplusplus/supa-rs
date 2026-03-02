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
        let pg_settings = PgSettings {
            database_dir: PathBuf::from(data_dir),
            port: 5433,
            user: "postgres".to_string(),
            password: "postgres".to_string(),
            auth_method: PgAuthMethod::Plain,
            persistent: true,
            timeout: Some(Duration::from_secs(300)), // 300s — allows first-run binary download (~50MB)
            migration_dir: None,
        };

        let fetch_settings = PgFetchSettings {
            version: PG_V15,
            ..Default::default()
        };

        tracing::info!("Setting up embedded PostgreSQL (first run downloads binary ~50MB)...");
        let mut pg = PgEmbed::new(pg_settings, fetch_settings).await?;

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
