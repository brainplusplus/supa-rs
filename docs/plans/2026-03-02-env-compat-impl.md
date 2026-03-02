# Env Compatibility Layer + Config Refactor Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Refactor `Config` into nested domain sub-structs, add full Phase 1 env resolution with `SUPARUST_*` → Supabase alias fallback, update all call sites, and produce migration docs.

**Architecture:** Single `env_any()` helper drives all resolution (first-match-wins on ordered key slice). `Config` becomes a composition of `ServerConfig`, `DatabaseConfig`, `JwtConfig`, `AuthConfig`, `UrlConfig`. All existing behaviour is preserved — this is a pure refactor + new-field addition.

**Tech Stack:** Rust, `std::env`, `dotenvy`, no new dependencies.

---

## Task 1: Refactor `src/config.rs` — sub-structs + `env_any()` + Phase 1 fields

**Files:**
- Modify: `src/config.rs` (full rewrite)

**Step 1: Replace the entire file**

```rust
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
        dotenvy::dotenv().ok();

        // ── JWT secret (must resolve first — keys are derived from it) ──────
        let jwt_secret_opt = env_any(&["SUPARUST_JWT_SECRET", "JWT_SECRET"]);
        let jwt_secret = match jwt_secret_opt {
            Some(s) => s,
            None => {
                eprintln!("[WARN] No JWT secret found. Auto-generating securely to .env...");
                load_or_generate_env()
            }
        };

        // Reload .env in case it was just written
        dotenvy::dotenv().ok();

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
        let pid_file = env_any(&["SUPARUST_PID_FILE"])
            .unwrap_or_else(|| format!(".suparust.{}.{}.pid", env_name, port));

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
```

**Step 2: Build to confirm no compile errors**

```bash
cd /d/Rust/SupaRust && cargo build 2>&1 | head -60
```

Expected: errors about `cfg.jwt_secret`, `cfg.port`, etc. in other files — that is correct, call sites not updated yet. `config.rs` itself must be error-free.

**Step 3: Commit**

```bash
git add src/config.rs
git commit -m "refactor(config): nested sub-structs + env_any() + Phase 1 fields"
```

---

## Task 2: Update call sites in `src/cli/start.rs`

**Files:**
- Modify: `src/cli/start.rs`

**Step 1: Apply field renames**

| Old | New |
|-----|-----|
| `cfg.log_level` | `cfg.log_level` (flat, unchanged) |
| `cfg.log_format` | `cfg.log_format` (flat, unchanged) |
| `cfg.pid_file` | `cfg.pid_file` (flat, unchanged) |
| `cfg.port` | `cfg.server.port` |
| `cfg.database_url` | `cfg.database.url` |
| `cfg.data_dir` | `cfg.database.data_dir` |
| `cfg.jwt_secret` | `cfg.jwt.secret` |
| `cfg.storage_root` | `cfg.storage_root` (flat, unchanged) |

Changes in `run_server()`:
```rust
// Line 91 — was: match cfg.database_url {
match cfg.database.url {

// Line 97 — was: cfg.data_dir
cfg.database.data_dir

// Line 111 — was: cfg.jwt_secret.clone()
cfg.jwt.secret.clone()

// Line 116 — was: cfg.storage_root.clone()  / cfg.jwt_secret.clone()
cfg.storage_root.clone()
cfg.jwt.secret.clone()

// Line 140 — was: cfg.port
cfg.server.port
```

Changes in `cmd_start_foreground()`:
```rust
// Line 22 — was: cfg.port
cfg.server.port

// Line 39 — was: cfg.port
cfg.server.port
```

**Step 2: Build**

```bash
cargo build 2>&1 | grep "^error" | head -20
```

Expected: only errors from `status.rs` and `stop.rs` (not yet updated).

**Step 3: Commit**

```bash
git add src/cli/start.rs
git commit -m "refactor(start): update cfg field paths to nested sub-structs"
```

---

## Task 3: Update call sites in `src/cli/status.rs` and `src/cli/stop.rs`

**Files:**
- Modify: `src/cli/status.rs`
- Modify: `src/cli/stop.rs`

**Step 1: `status.rs` — rename `cfg.port` → `cfg.server.port`**

All occurrences of `cfg.port` in `status.rs` become `cfg.server.port`. `cfg.pid_file` is flat — no change.

**Step 2: `stop.rs` — rename `cfg.port` → `cfg.server.port`**

All occurrences of `cfg.port` in `stop.rs` become `cfg.server.port`. `cfg.pid_file` flat — no change.

**Step 3: Full build must pass**

```bash
cargo build 2>&1
```

Expected: zero errors. If any remain, fix them before proceeding.

**Step 4: Run integration tests**

```bash
cd test-client && npm test 2>&1 | tail -20
```

Expected: 21/21 passing (no behaviour change).

**Step 5: Commit**

```bash
git add src/cli/status.rs src/cli/stop.rs
git commit -m "refactor(cli): update cfg field paths in status + stop"
```

---

## Task 4: Update `.env.example` and `.env.test.example`

**Files:**
- Modify: `.env.example`
- Modify: `.env.test.example`

**Step 1: Replace `.env.example`**

```env
# SupaRust Environment Configuration
# Rename to .env and fill in your values.
# Run: scripts/gen-env.mjs to auto-generate JWT keys.

# ── Server ────────────────────────────────────────────────────────────────────
SUPARUST_PORT=3000
SUPARUST_BIND_ADDR=0.0.0.0

# ── Database ──────────────────────────────────────────────────────────────────
# Set SUPARUST_DB_URL to use an external Postgres and skip pg-embed.
# SUPARUST_DB_URL=postgresql://postgres:password@localhost:5432/postgres
SUPARUST_DB_DATA_DIR=./data/postgres
SUPARUST_DB_HOST=localhost
SUPARUST_DB_PORT=5432
SUPARUST_DB_USER=postgres
SUPARUST_DB_PASSWORD=           # auto-generated if blank
SUPARUST_DB_NAME=postgres
SUPARUST_DB_SCHEMAS=public

# ── Storage ───────────────────────────────────────────────────────────────────
SUPARUST_STORAGE_ROOT=./data/storage

# ── Auth / JWT ────────────────────────────────────────────────────────────────
SUPARUST_JWT_SECRET=            # min 32 chars — auto-generated if blank
SUPARUST_ANON_KEY=              # derived from JWT_SECRET if blank
SUPARUST_SERVICE_KEY=           # derived from JWT_SECRET if blank
SUPARUST_JWT_EXPIRY=3600

# ── Auth Behaviour ────────────────────────────────────────────────────────────
SUPARUST_DISABLE_SIGNUP=false
SUPARUST_ENABLE_EMAIL_SIGNUP=true
SUPARUST_ENABLE_EMAIL_AUTOCONFIRM=false
SUPARUST_ENABLE_ANONYMOUS_USERS=false

# ── Public URLs ───────────────────────────────────────────────────────────────
SUPARUST_SITE_URL=http://localhost:3000
SUPARUST_API_URL=http://localhost:3000

# ── Logging ───────────────────────────────────────────────────────────────────
SUPARUST_LOG_LEVEL=info         # trace | debug | info | warn | error
SUPARUST_LOG_FORMAT=pretty      # pretty | json
```

**Step 2: Replace `.env.test.example`**

```env
# .env.test.example — auto-generated structure (run scripts/gen-env-test.mjs)
SUPARUST_ENV=test
SUPARUST_PORT=3001
SUPARUST_DB_DATA_DIR=./data/pg-test
SUPARUST_STORAGE_ROOT=./data/storage-test
SUPARUST_JWT_SECRET=<generated>
SUPARUST_ANON_KEY=<generated>
SUPARUST_SERVICE_KEY=<generated>
SUPARUST_JWT_EXPIRY=3600
SUPARUST_LOG_LEVEL=info
SUPARUST_LOG_FORMAT=pretty
SUPARUST_SITE_URL=http://localhost:3001
SUPARUST_API_URL=http://localhost:3001
```

**Step 3: Commit**

```bash
git add .env.example .env.test.example
git commit -m "docs(env): update examples with Phase 1 canonical vars"
```

---

## Task 5: Write `docs/migrating-from-supabase.md`

**Files:**
- Create: `docs/migrating-from-supabase.md`

**Step 1: Write the file**

```markdown
# Migrating from Supabase Self-Hosted

SupaRust is a drop-in Supabase replacement. If you already have a Supabase
self-hosted `.env`, it will work out of the box — SupaRust reads Supabase
env vars as compatibility aliases.

For new projects, use `SUPARUST_*` names directly.

## Compatibility Priority

```
1. SUPARUST_*       ← canonical (always wins)
2. Supabase alias   ← compatibility fallback
3. Default value
4. Auto-generate    ← jwt_secret, db_password only
```

When both are set, SupaRust logs:
```
[INFO] Both SUPARUST_DB_PASSWORD and POSTGRES_PASSWORD are set. Using SUPARUST_DB_PASSWORD.
```

When falling back to a Supabase alias:
```
[WARN] Using legacy env POSTGRES_PASSWORD. Prefer SUPARUST_DB_PASSWORD.
```

---

## Variable Mapping — Phase 1 (Core)

### Core / JWT

| Supabase | SupaRust | Default |
|---|---|---|
| `JWT_SECRET` | `SUPARUST_JWT_SECRET` | auto-generated |
| `ANON_KEY` | `SUPARUST_ANON_KEY` | derived from `JWT_SECRET` |
| `SERVICE_ROLE_KEY` | `SUPARUST_SERVICE_KEY` | derived from `JWT_SECRET` |
| `JWT_EXPIRY` | `SUPARUST_JWT_EXPIRY` | `3600` |

### Server

| Supabase | SupaRust | Default |
|---|---|---|
| *(none)* | `SUPARUST_PORT` | `3000` |
| `API_EXTERNAL_URL` | `SUPARUST_API_URL` | `http://localhost:3000` |
| `SITE_URL` | `SUPARUST_SITE_URL` | `http://localhost:3000` |

### Database

| Supabase | SupaRust | Default |
|---|---|---|
| `POSTGRES_PASSWORD` | `SUPARUST_DB_PASSWORD` | auto-generated |
| `POSTGRES_HOST` | `SUPARUST_DB_HOST` | `localhost` |
| `POSTGRES_PORT` | `SUPARUST_DB_PORT` | `5432` |
| `POSTGRES_USER` | `SUPARUST_DB_USER` | `postgres` |
| `POSTGRES_DB` | `SUPARUST_DB_NAME` | `postgres` |
| `DATABASE_URL` | `SUPARUST_DB_URL` | *(none — uses pg-embed)* |
| `PGRST_DB_SCHEMAS` | `SUPARUST_DB_SCHEMAS` | `public` |

### Auth

| Supabase | SupaRust | Default |
|---|---|---|
| `DISABLE_SIGNUP` | `SUPARUST_DISABLE_SIGNUP` | `false` |
| `ENABLE_EMAIL_SIGNUP` | `SUPARUST_ENABLE_EMAIL_SIGNUP` | `true` |
| `ENABLE_EMAIL_AUTOCONFIRM` | `SUPARUST_ENABLE_EMAIL_AUTOCONFIRM` | `false` |
| `ENABLE_ANONYMOUS_USERS` | `SUPARUST_ENABLE_ANONYMOUS_USERS` | `false` |

---

## Variable Mapping — Phase 2+ (Planned)

These are documented here for migration planning. They are **not yet active**
in SupaRust — see the [Roadmap](../README.md#roadmap).

### Connection Pooler

| Supabase | SupaRust | Default |
|---|---|---|
| `POOLER_PROXY_PORT_TRANSACTION` | `SUPARUST_POOLER_PORT` | `6543` |
| `POOLER_DEFAULT_POOL_SIZE` | `SUPARUST_POOLER_DEFAULT_POOL_SIZE` | `20` |
| `POOLER_MAX_CLIENT_CONN` | `SUPARUST_POOLER_MAX_CLIENTS` | `100` |
| `POOLER_TENANT_ID` | `SUPARUST_POOLER_TENANT_ID` | `default` |

### SMTP (Email)

| Supabase | SupaRust | Default |
|---|---|---|
| `SMTP_HOST` | `SUPARUST_SMTP_HOST` | *(optional)* |
| `SMTP_PORT` | `SUPARUST_SMTP_PORT` | `587` |
| `SMTP_USER` | `SUPARUST_SMTP_USER` | *(optional)* |
| `SMTP_PASS` | `SUPARUST_SMTP_PASS` | *(optional)* |
| `SMTP_ADMIN_EMAIL` | `SUPARUST_SMTP_ADMIN_EMAIL` | *(optional)* |
| `SMTP_SENDER_NAME` | `SUPARUST_SMTP_SENDER_NAME` | *(optional)* |
| `ADDITIONAL_REDIRECT_URLS` | `SUPARUST_REDIRECT_URLS` | *(optional)* |

### Storage (S3 backend)

| Supabase | SupaRust | Default |
|---|---|---|
| `GLOBAL_S3_BUCKET` | `SUPARUST_S3_BUCKET` | *(optional)* |
| `REGION` | `SUPARUST_S3_REGION` | *(optional)* |
| `STORAGE_TENANT_ID` | `SUPARUST_STORAGE_TENANT_ID` | `default` |
| `S3_PROTOCOL_ACCESS_KEY_ID` | `SUPARUST_S3_ACCESS_KEY` | *(optional)* |
| `S3_PROTOCOL_ACCESS_KEY_SECRET` | `SUPARUST_S3_SECRET_KEY` | *(optional)* |

---

## Variables Not Supported

These Supabase variables have no equivalent in SupaRust (services not present):

- `KONG_*` — no API gateway
- `LOGFLARE_*` / `GOOGLE_PROJECT_*` — no analytics service
- `IMGPROXY_*` — no image proxy
- `STUDIO_*` / `DASHBOARD_*` — no Studio UI (planned)
- `MINIO_*` — use your own MinIO; point `SUPARUST_DB_URL` at it
- `DOCKER_SOCKET_LOCATION` — no container management
- `FUNCTIONS_VERIFY_JWT` — no Edge Functions yet
- `SECRET_KEY_BASE` / `VAULT_ENC_KEY` — Elixir/Phoenix-specific
- `MAILER_URLPATHS_*` — hardcoded in SupaRust

---

## Quick Migration Example

**Supabase `.env` (before):**
```env
JWT_SECRET=my-super-secret-32-char-minimum-key
ANON_KEY=eyJ...
SERVICE_ROLE_KEY=eyJ...
POSTGRES_PASSWORD=your-super-secret-and-long-postgres-password
POSTGRES_HOST=localhost
POSTGRES_DB=postgres
SITE_URL=http://localhost:3000
API_EXTERNAL_URL=http://localhost:3000
```

**Option A — Use as-is (zero changes):**
SupaRust reads the Supabase vars directly as aliases. Just run `suparust start`.

**Option B — Migrate to canonical names:**
```env
SUPARUST_JWT_SECRET=my-super-secret-32-char-minimum-key
SUPARUST_ANON_KEY=eyJ...
SUPARUST_SERVICE_KEY=eyJ...
SUPARUST_DB_PASSWORD=your-super-secret-and-long-postgres-password
SUPARUST_DB_HOST=localhost
SUPARUST_DB_NAME=postgres
SUPARUST_SITE_URL=http://localhost:3000
SUPARUST_API_URL=http://localhost:3000
```
```

**Step 2: Commit**

```bash
git add docs/migrating-from-supabase.md
git commit -m "docs: add migrating-from-supabase compatibility guide"
```

---

## Task 6: Final verification

**Step 1: Full clean build**

```bash
cargo build 2>&1
```

Expected: zero errors, zero warnings about unused fields.

**Step 2: Integration tests**

```bash
cd test-client && npm test 2>&1 | tail -30
```

Expected: 21/21 passing.

**Step 3: Spot-check env compat manually**

```bash
# Supabase-style alias should work
JWT_SECRET=aaaabbbbccccddddeeeeffffgggghhhhiiiijjjj cargo run -- start &
sleep 3 && curl -s http://localhost:3000/health | head -2
kill %1
```

Expected: server starts, `/health` responds.

**Step 4: Commit if any stragglers**

```bash
git add -p
git commit -m "chore: final cleanup after config refactor"
```
