use std::env;
use std::fs;
use std::path::Path;

const CONFIG_FILE: &str = "./data/suparust-config.json";

#[derive(serde::Serialize, serde::Deserialize)]
struct PersistedConfig {
    jwt_secret: String,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: Option<String>,  // None = use embedded Postgres
    pub jwt_secret: String,
    pub port: u16,
    pub data_dir: String,              // where embedded PG stores data
    pub storage_root: String,
}

impl Config {
    pub fn from_env() -> Self {
        let jwt_secret = env::var("JWT_SECRET")
            .unwrap_or_else(|_| load_or_generate_secret());

        Self {
            database_url: env::var("DATABASE_URL").ok(),
            jwt_secret,
            port: env::var("PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(3000),
            data_dir: env::var("DATA_DIR")
                .unwrap_or_else(|_| "./data/postgres".to_string()),
            storage_root: env::var("STORAGE_ROOT")
                .unwrap_or_else(|_| "./data/storage".to_string()),
        }
    }
}

fn load_or_generate_secret() -> String {
    // Coba load dari file dulu
    if Path::new(CONFIG_FILE).exists() {
        if let Ok(content) = fs::read_to_string(CONFIG_FILE) {
            if let Ok(cfg) = serde_json::from_str::<PersistedConfig>(&content) {
                tracing::info!("Loaded JWT secret from {}", CONFIG_FILE);
                return cfg.jwt_secret;
            }
        }
    }

    // Generate baru dan persist
    let secret = generate_secret();
    let persisted = PersistedConfig { jwt_secret: secret.clone() };

    // Pastikan directory ada
    if let Some(parent) = Path::new(CONFIG_FILE).parent() {
        let _ = fs::create_dir_all(parent);
    }

    if let Ok(json) = serde_json::to_string_pretty(&persisted) {
        let _ = fs::write(CONFIG_FILE, json);
        tracing::info!("Generated and persisted JWT secret to {}", CONFIG_FILE);
    }

    secret
}

fn generate_secret() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    // Karena kita tidak punya dependensi rand:
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let hash_string = format!("{:x}some_salt_for_generating_secret_like_this", now);
    hex::encode(hash_string)
}
