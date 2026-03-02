use std::env;
use std::fs;
use std::path::Path;

const ENV_FILE: &str = ".env";

// ── Sub-structs ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub port:      u16,
    pub bind_addr: String,
}

#[derive(Debug, Clone)]
pub struct DatabaseConfig {
    /// If set, overrides host/port/user/password/name at runtime.
    /// Those fields are retained for debug display only.
    pub url:      Option<String>,
    pub host:     String,
    pub port:     u16,
    pub user:     String,
    pub password: String,
    pub name:     String,
    pub data_dir: String,
    pub schemas:  String,
}

#[derive(Debug, Clone)]
pub struct JwtConfig {
    pub secret:      String,
    pub expiry:      u64,
    pub anon_key:    String,
    pub service_key: String,
}

#[derive(Debug, Clone)]
pub struct AuthConfig {
    pub disable_signup:           bool,
    pub enable_email_signup:      bool,
    pub enable_email_autoconfirm: bool,
    pub enable_anonymous_users:   bool,
}

#[derive(Debug, Clone)]
pub struct UrlConfig {
    pub site_url: String,
    pub api_url:  String,
}

// ── Top-level Config ───────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Config {
    pub server:   ServerConfig,
    pub database: DatabaseConfig,
    pub jwt:      JwtConfig,
    pub auth:     AuthConfig,
    pub urls:     UrlConfig,

    // Filesystem / observability — intentionally flat
    pub storage_root: String,
    pub log_level:    String,
    pub log_format:   String,
    pub env:          String,
    pub pid_file:     String,
}

// ── Resolution helper ──────────────────────────────────────────────────────────

/// Try env vars in order — first one set wins.
/// Logs a WARN (to stderr, before tracing is initialised) when falling back
/// to a legacy Supabase alias, and an INFO when both canonical and alias are set.
fn env_any(keys: &[&str]) -> Option<String> {
    let mut found: Option<(usize, String)> = None;
    for (i, k) in keys.iter().enumerate() {
        if let Ok(val) = env::var(k) {
            found = Some((i, val));
            break;
        }
    }
    match found {
        None => None,
        Some((0, val)) => {
            // canonical SUPARUST_* hit — check if alias is also set (warn user)
            if keys.len() > 1 {
                for alias in &keys[1..] {
                    if env::var(alias).is_ok() {
                        eprintln!(
                            "[INFO] Both {} and {} are set. Using {}.",
                            keys[0], alias, keys[0]
                        );
                        break;
                    }
                }
            }
            Some(val)
        }
        Some((i, val)) => {
            // fell back to a Supabase alias
            eprintln!(
                "[WARN] Using legacy env {}. Prefer {}.",
                keys[i], keys[0]
            );
            Some(val)
        }
    }
}

fn env_bool(keys: &[&str], default: bool) -> bool {
    env_any(keys)
        .map(|v| matches!(v.to_lowercase().as_str(), "true" | "1" | "yes"))
        .unwrap_or(default)
}

// ── Config::from_env ───────────────────────────────────────────────────────────

impl Config {
    pub fn from_env() -> Self {
        // ── JWT secret (must resolve first — keys are derived from it) ──────
        let jwt_secret_opt = env_any(&["SUPARUST_JWT_SECRET", "JWT_SECRET"]);
        let jwt_secret = match jwt_secret_opt {
            Some(s) => s,
            None => {
                eprintln!("[WARN] No JWT secret found. Auto-generating securely to .env...");
                load_or_generate_env()
            }
        };

        // ── Validate log_format early ────────────────────────────────────────
        let log_format = env_any(&["SUPARUST_LOG_FORMAT"])
            .unwrap_or_else(|| "pretty".to_string());
        if log_format != "pretty" && log_format != "json" {
            eprintln!(
                "error: SUPARUST_LOG_FORMAT=\"{}\" is invalid. Valid values: \"pretty\", \"json\"",
                log_format
            );
            std::process::exit(1);
        }

        // ── Port ─────────────────────────────────────────────────────────────
        let port = env_any(&["SUPARUST_PORT", "PORT"])
            .and_then(|p| {
                p.parse::<u16>().map_err(|_| {
                    eprintln!(
                        "[WARN] SUPARUST_PORT=\"{}\" is not a valid port. Falling back to 3000.",
                        p
                    );
                }).ok()
            })
            .unwrap_or(3000);

        // ── URLs (port-dependent defaults) ───────────────────────────────────
        let site_url = env_any(&["SUPARUST_SITE_URL", "SITE_URL"])
            .unwrap_or_else(|| format!("http://localhost:{}", port));
        let api_url  = env_any(&["SUPARUST_API_URL", "API_EXTERNAL_URL"])
            .unwrap_or_else(|| format!("http://localhost:{}", port));

        // ── JWT keys ─────────────────────────────────────────────────────────
        let anon_key = env_any(&["SUPARUST_ANON_KEY", "ANON_KEY"])
            .unwrap_or_else(|| generate_jwt(&jwt_secret, "anon"));
        let service_key = env_any(&["SUPARUST_SERVICE_KEY", "SERVICE_ROLE_KEY"])
            .unwrap_or_else(|| generate_jwt(&jwt_secret, "service_role"));
        let jwt_expiry = env_any(&["SUPARUST_JWT_EXPIRY", "JWT_EXPIRY"])
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(3600);

        // ── Runtime metadata ─────────────────────────────────────────────────
        let env_name = env_any(&["SUPARUST_ENV"])
            .unwrap_or_else(|| "local".to_string());
        let pid_identity = std::env::var("SUPARUST_PID_IDENTITY")
            .unwrap_or_else(|_| "local".to_string());
        let pid_file = env_any(&["SUPARUST_PID_FILE"])
            .unwrap_or_else(|| format!(".suparust.{}.{}.pid", pid_identity, port));

        // ── DB password — auto-generate if absent ────────────────────────────
        let db_password = env_any(&["SUPARUST_DB_PASSWORD", "POSTGRES_PASSWORD"])
            .unwrap_or_else(|| {
                eprintln!("[WARN] No DB password found. Auto-generating.");
                generate_secret()
            });

        Self {
            server: ServerConfig {
                port,
                bind_addr: env_any(&["SUPARUST_BIND_ADDR"])
                    .unwrap_or_else(|| "0.0.0.0".to_string()),
            },
            database: DatabaseConfig {
                url: env_any(&["SUPARUST_DB_URL", "DATABASE_URL"]),
                host: env_any(&["SUPARUST_DB_HOST", "POSTGRES_HOST"])
                    .unwrap_or_else(|| "localhost".to_string()),
                port: env_any(&["SUPARUST_DB_PORT", "POSTGRES_PORT"])
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(5432),
                user: env_any(&["SUPARUST_DB_USER", "POSTGRES_USER"])
                    .unwrap_or_else(|| "postgres".to_string()),
                password: db_password,
                name: env_any(&["SUPARUST_DB_NAME", "POSTGRES_DB"])
                    .unwrap_or_else(|| "postgres".to_string()),
                data_dir: env_any(&["SUPARUST_DB_DATA_DIR", "DATA_DIR"])
                    .unwrap_or_else(|| "./data/postgres".to_string()),
                schemas: env_any(&["SUPARUST_DB_SCHEMAS", "PGRST_DB_SCHEMAS"])
                    .unwrap_or_else(|| "public".to_string()),
            },
            jwt: JwtConfig {
                secret: jwt_secret,
                expiry: jwt_expiry,
                anon_key,
                service_key,
            },
            auth: AuthConfig {
                disable_signup:           env_bool(&["SUPARUST_DISABLE_SIGNUP",           "DISABLE_SIGNUP"],           false),
                enable_email_signup:      env_bool(&["SUPARUST_ENABLE_EMAIL_SIGNUP",      "ENABLE_EMAIL_SIGNUP"],      true),
                enable_email_autoconfirm: env_bool(&["SUPARUST_ENABLE_EMAIL_AUTOCONFIRM", "ENABLE_EMAIL_AUTOCONFIRM"], false),
                enable_anonymous_users:   env_bool(&["SUPARUST_ENABLE_ANONYMOUS_USERS",   "ENABLE_ANONYMOUS_USERS"],   false),
            },
            urls: UrlConfig { site_url, api_url },
            storage_root: env_any(&["SUPARUST_STORAGE_ROOT", "STORAGE_ROOT"])
                .unwrap_or_else(|| "./data/storage".to_string()),
            log_level: env_any(&["SUPARUST_LOG_LEVEL"])
                .unwrap_or_else(|| "info".to_string()),
            log_format,
            env: env_name,
            pid_file,
        }
    }
}

// ── Secret / JWT generation (unchanged) ───────────────────────────────────────

fn load_or_generate_env() -> String {
    let secret = generate_secret();
    let anon_init    = generate_jwt(&secret, "anon");
    let service_init = generate_jwt(&secret, "service_role");

    eprintln!("[INFO] Generated new credentials. Copy these to your .env:");
    eprintln!("[INFO] SUPARUST_JWT_SECRET={}", secret);
    eprintln!("[INFO] SUPARUST_ANON_KEY={}", anon_init);
    eprintln!("[INFO] SUPARUST_SERVICE_KEY={}", service_init);

    let env_content = format!(
        "\n# Auto-generated by SupaRust\n\
        SUPARUST_JWT_SECRET={secret}\n\
        SUPARUST_ANON_KEY={anon_init}\n\
        SUPARUST_SERVICE_KEY={service_init}\n"
    );

    if Path::new(ENV_FILE).exists() {
        let _ = fs::OpenOptions::new()
            .append(true)
            .open(ENV_FILE)
            .and_then(|mut f| { use std::io::Write; f.write_all(env_content.as_bytes()) });
        eprintln!("[INFO] Appended missing keys to existing .env");
    } else {
        let full = format!(
            "# SupaRust Environment\n\
            SUPARUST_PORT=3000\n\
            SUPARUST_DB_DATA_DIR=./data/postgres\n\
            SUPARUST_STORAGE_ROOT=./data/storage\n\
            SUPARUST_LOG_LEVEL=info\n\
            SUPARUST_LOG_FORMAT=pretty\n\
            {env_content}"
        );
        let _ = fs::write(ENV_FILE, full);
        eprintln!("[INFO] Generated fresh .env with new keys");
    }

    secret
}

fn generate_secret() -> String {
    use rand::RngCore;
    use rand::rngs::OsRng;
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

fn generate_jwt(secret: &str, role: &str) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let iat = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    let exp = iat + 10 * 365 * 24 * 3600;

    let header  = base64_url_encode(br#"{"alg":"HS256","typ":"JWT"}"#);
    let payload = base64_url_encode(
        format!(r#"{{"role":"{}","iss":"suparust","iat":{},"exp":{}}}"#, role, iat, exp).as_bytes(),
    );
    let signing_input = format!("{}.{}", header, payload);
    let sig = hmac_sha256_sign(secret.as_bytes(), signing_input.as_bytes());
    format!("{}.{}.{}", header, payload, base64_url_encode(&sig))
}

fn base64_url_encode(input: &[u8]) -> String {
    use std::fmt::Write;
    let b64 = BASE64_TABLE;
    let mut out = String::new();
    let mut i = 0;
    while i + 2 < input.len() {
        let n = ((input[i] as u32) << 16) | ((input[i+1] as u32) << 8) | (input[i+2] as u32);
        let _ = write!(out, "{}{}{}{}",
            b64[(n >> 18) as usize], b64[((n >> 12) & 0x3F) as usize],
            b64[((n >> 6) & 0x3F) as usize], b64[(n & 0x3F) as usize]);
        i += 3;
    }
    if i + 1 == input.len() {
        let n = (input[i] as u32) << 16;
        let _ = write!(out, "{}{}", b64[(n >> 18) as usize], b64[((n >> 12) & 0x3F) as usize]);
    } else if i + 2 == input.len() {
        let n = ((input[i] as u32) << 16) | ((input[i+1] as u32) << 8);
        let _ = write!(out, "{}{}{}",
            b64[(n >> 18) as usize], b64[((n >> 12) & 0x3F) as usize], b64[((n >> 6) & 0x3F) as usize]);
    }
    out
}

const BASE64_TABLE: &[char] = &[
    'A','B','C','D','E','F','G','H','I','J','K','L','M','N','O','P',
    'Q','R','S','T','U','V','W','X','Y','Z','a','b','c','d','e','f',
    'g','h','i','j','k','l','m','n','o','p','q','r','s','t','u','v',
    'w','x','y','z','0','1','2','3','4','5','6','7','8','9','-','_',
];

fn hmac_sha256_sign(key: &[u8], data: &[u8]) -> Vec<u8> {
    use sha2::Digest;
    const BLOCK_SIZE: usize = 64;
    let mut k = if key.len() > BLOCK_SIZE {
        let mut h = sha2::Sha256::new(); h.update(key); h.finalize().to_vec()
    } else { key.to_vec() };
    k.resize(BLOCK_SIZE, 0);
    let i_key: Vec<u8> = k.iter().map(|b| b ^ 0x36).collect();
    let o_key: Vec<u8> = k.iter().map(|b| b ^ 0x5C).collect();
    let mut inner = sha2::Sha256::new(); inner.update(&i_key); inner.update(data);
    let inner_hash = inner.finalize();
    let mut outer = sha2::Sha256::new(); outer.update(&o_key); outer.update(&inner_hash);
    outer.finalize().to_vec()
}
