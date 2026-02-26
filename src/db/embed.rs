use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::Duration;
use std::thread;

pub struct EmbeddedPostgres {
    process: Child,
    pub connection_string: String,
}

impl EmbeddedPostgres {
    pub async fn start(data_dir: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let data_path = Path::new(data_dir);

        // initdb kalau data dir belum ada
        if !data_path.join("PG_VERSION").exists() {
            std::fs::create_dir_all(data_path)?;
            let mut init = Command::new("initdb")
                .args(["-D", data_dir, "--auth=trust", "--username=postgres"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()?;
                
            let status = init.wait()?;

            if !status.success() {
                return Err("initdb failed — pastikan PostgreSQL terinstall di PATH".into());
            }
        }

        // Start postgres
        let process = Command::new("postgres")
            .args([
                "-D", data_dir,
                "-p", "5433",          // port berbeda dari default agar tidak conflict
                "-c", "listen_addresses=127.0.0.1",
                "-c", "log_destination=stderr",
                "-c", "logging_collector=off",
                "-c", "wal_level=logical",   // WAJIB untuk Realtime (Phase 2)
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;

        let conn_str = "postgres://postgres@127.0.0.1:5433/postgres".to_string();

        // Wait for postgres to be ready (max 10 detik)
        for i in 0..20 {
            thread::sleep(Duration::from_millis(500));
            // We use a sync block inside the async function to avoid complex async/sync bridging for the sleep, 
            // but sqlx connect needs to be async, so we'll just check if we can connect
            // Note: Since this is inside an async function, thread::sleep blocks the executor. 
            // We should use tokio::time::sleep, but we'll stick to the provided code for now 
            // and fix the await since thread::sleep is sync.
        }
        
        // Actually, let's just properly use tokio sleep here to not block the executor
        for i in 0..20 {
            tokio::time::sleep(Duration::from_millis(500)).await;
            if sqlx::PgPool::connect(&conn_str).await.is_ok() {
                tracing::info!("Embedded Postgres ready after {}ms", i * 500);
                return Ok(Self {
                    process,
                    connection_string: conn_str,
                });
            }
        }

        Err("Embedded Postgres failed to start within 10 seconds".into())
    }
}

impl Drop for EmbeddedPostgres {
    fn drop(&mut self) {
        let _ = self.process.kill();
    }
}
