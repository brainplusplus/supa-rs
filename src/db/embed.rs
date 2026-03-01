use postgresql_embedded::{PostgreSQL, Settings, VersionReq};
use std::str::FromStr;

pub struct EmbeddedPostgres {
    pg: PostgreSQL,
    pub connection_string: String,
}

impl EmbeddedPostgres {
    pub async fn start(data_dir: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let settings = Settings {
            version: VersionReq::from_str("=17.2.0")?,
            data_dir: std::path::PathBuf::from(data_dir),
            host: "127.0.0.1".to_string(),
            port: 5433,
            username: "postgres".to_string(),
            temporary: false,
            ..Default::default()
        };

        let mut pg = PostgreSQL::new(settings);

        tracing::info!("Setting up embedded PostgreSQL (first run downloads binary ~50MB)...");
        pg.setup().await?;

        tracing::info!("Starting embedded PostgreSQL in {}", data_dir);
        pg.start().await?;

        // Buat database "postgres" kalau belum ada
        if !pg.database_exists("postgres").await? {
            pg.create_database("postgres").await?;
        }

        let conn_str = "postgres://postgres@127.0.0.1:5433/postgres".to_string();
        tracing::info!("Embedded PostgreSQL ready at {}", conn_str);

        Ok(Self { pg, connection_string: conn_str })
    }
}

impl Drop for EmbeddedPostgres {
    fn drop(&mut self) {
        // postgresql_embedded handles graceful shutdown via its own Drop
        // We use blocking runtime since Drop is sync
        let rt = tokio::runtime::Handle::try_current();
        if let Ok(handle) = rt {
            handle.block_on(async {
                let _ = self.pg.stop().await;
            });
        }
    }
}
