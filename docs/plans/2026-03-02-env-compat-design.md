# Design: Environment Compatibility Layer + Config Refactor

**Date:** 2026-03-02
**Status:** Approved
**Phase:** 1 (Core)

---

## Context

SupaRust is a Rust-native Supabase replacement. Its target users fall into two groups:

1. **New users** — start fresh with `SUPARUST_*` vars
2. **Migrating users** — drop in an existing Supabase `.env` and expect it to work

Currently, `Config` is a flat struct with implicit fallbacks for only a subset of vars. This design formalises the compatibility layer and restructures `Config` into domain-scoped sub-structs to support phased feature expansion cleanly.

---

## Decision

**Approach: Nested sub-structs + `env_any()` helper (Phase C — Phased)**

- `Config` struct refactored into domain sub-structs: `ServerConfig`, `DatabaseConfig`, `JwtConfig`, `AuthConfig`, `UrlConfig`
- A single `env_any(&[&str]) -> Option<String>` helper drives all resolution
- Priority chain: `SUPARUST_*` → Supabase alias → default → auto-generate
- Phase 2+ fields (`PoolerConfig`, `SmtpConfig`, `StorageConfig`) added when features are implemented — not before
- Migration docs cover the full mapping intent; code reflects current capability

---

## Config Struct Layout (Phase 1)

```rust
pub struct Config {
    pub server:   ServerConfig,
    pub database: DatabaseConfig,
    pub jwt:      JwtConfig,
    pub auth:     AuthConfig,
    pub urls:     UrlConfig,

    // observability + runtime metadata — intentionally flat
    pub log_level:  String,
    pub log_format: String,
    pub env:        String,
    pub pid_file:   String,
}

pub struct ServerConfig {
    pub port:      u16,    // SUPARUST_PORT | PORT, default: 3000
    pub bind_addr: String, // SUPARUST_BIND_ADDR, default: 0.0.0.0
}

pub struct DatabaseConfig {
    pub url:      Option<String>, // SUPARUST_DB_URL | DATABASE_URL — overrides all fields below when set
    pub host:     String,         // SUPARUST_DB_HOST | POSTGRES_HOST, default: localhost
    pub port:     u16,            // SUPARUST_DB_PORT | POSTGRES_PORT, default: 5432
    pub user:     String,         // SUPARUST_DB_USER | POSTGRES_USER, default: postgres
    pub password: String,         // SUPARUST_DB_PASSWORD | POSTGRES_PASSWORD, auto-generate
    pub name:     String,         // SUPARUST_DB_NAME | POSTGRES_DB, default: postgres
    pub data_dir: String,         // SUPARUST_DB_DATA_DIR | DATA_DIR, default: ./data/postgres
    pub schemas:  String,         // SUPARUST_DB_SCHEMAS | PGRST_DB_SCHEMAS, default: public
}
// Note: if `url` is set, host/port/user/password/name are ignored at runtime.
// They remain in the struct for debug/display purposes only.

pub struct JwtConfig {
    pub secret:      String, // SUPARUST_JWT_SECRET | JWT_SECRET, auto-generate
    pub expiry:      u64,    // SUPARUST_JWT_EXPIRY | JWT_EXPIRY, default: 3600
    pub anon_key:    String, // SUPARUST_ANON_KEY | ANON_KEY, derive from secret
    pub service_key: String, // SUPARUST_SERVICE_KEY | SERVICE_ROLE_KEY, derive from secret
}

pub struct AuthConfig {
    pub disable_signup:           bool, // SUPARUST_DISABLE_SIGNUP | DISABLE_SIGNUP, default: false
    pub enable_email_signup:      bool, // SUPARUST_ENABLE_EMAIL_SIGNUP | ENABLE_EMAIL_SIGNUP, default: true
    pub enable_email_autoconfirm: bool, // SUPARUST_ENABLE_EMAIL_AUTOCONFIRM | ENABLE_EMAIL_AUTOCONFIRM, default: false
    pub enable_anonymous_users:   bool, // SUPARUST_ENABLE_ANONYMOUS_USERS | ENABLE_ANONYMOUS_USERS, default: false
}

pub struct UrlConfig {
    pub site_url: String, // SUPARUST_SITE_URL | SITE_URL, default: http://localhost:{port}
    pub api_url:  String, // SUPARUST_API_URL | API_EXTERNAL_URL, default: http://localhost:{port}
}
```

`SUPARUST_STORAGE_ROOT` stays flat in `Config` (not in a sub-struct) — it is a filesystem runtime concern, not a domain config, consistent with `database.data_dir`.

---

## Env Resolution Rules

### Priority chain

```
1. SUPARUST_*      (canonical — always wins)
2. Supabase alias  (compat layer)
3. Default value
4. Auto-generate   (jwt_secret, db_password only)
```

### Helper

```rust
fn env_any(keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|k| std::env::var(k).ok())
}
```

The first key in the slice is always the `SUPARUST_*` canonical name. First match wins — priority is enforced by array order.

### Logging behavior

| Situation | Log level | Message |
|-----------|-----------|---------|
| Both `SUPARUST_X` and Supabase alias set | `INFO` | `Both SUPARUST_X and Y are set. Using SUPARUST_X.` |
| Fallback to Supabase alias | `WARN` | `Using legacy env Y. Prefer SUPARUST_X.` |
| Auto-generated | `WARN` | `No X found. Auto-generating securely.` |

Logs emitted once at startup only (not in request loops).

---

## Secret Derivation Rules

```
SUPARUST_JWT_SECRET (or JWT_SECRET)
    ↓ if SUPARUST_ANON_KEY and ANON_KEY are both absent
    → derive ANON_KEY   (HS256, role=anon)
    ↓ if SUPARUST_SERVICE_KEY and SERVICE_ROLE_KEY are both absent
    → derive SERVICE_KEY (HS256, role=service_role)
```

Explicit keys always override derived keys. DB password is auto-generated independently — not part of the JWT chain.

---

## Future Phase Slots

These fields are **not implemented in Phase 1** but the struct is designed to accommodate them:

| Phase | Addition |
|-------|----------|
| 2 | `pub pooler: Option<PoolerConfig>` |
| 3 | `pub smtp: Option<SmtpConfig>`, `pub storage_ext: Option<StorageExtConfig>` |

---

## Deliverables

1. `src/config.rs` — refactor to nested sub-structs + `env_any()` + full Phase 1 resolution
2. `docs/migrating-from-supabase.md` — full domain-scoped mapping table, phase notes, example `.env`
3. `.env.example` — update with all Phase 1 vars + inline comments
4. `.env.test.example` — update to match
5. Call site updates — `cfg.jwt_secret` → `cfg.jwt.secret`, etc. (one PR, no behaviour change)

---

## Optional Future Enhancements (Not Phase 1)

- `suparust doctor` — print resolved config with env source per field
- `Config::debug_dump()` — safe redacted dump (no secrets printed)
- `Config::validate()` — explicit validation errors at early boot
- `SUPARUST_MASTER_SECRET` — single root secret for future HA/cluster mode
