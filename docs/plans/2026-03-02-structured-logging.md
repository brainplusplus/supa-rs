# Structured Logging + HTTP TraceLayer with Request ID — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add configurable structured logging (`SUPARUST_LOG_LEVEL`, `SUPARUST_LOG_FORMAT`) and automatic HTTP request logging with request ID correlation via `tower-http` `TraceLayer`.

**Architecture:** A new `src/tracing.rs` module exposes `init_tracing(level, format, TracingWriter)` using an enum to avoid generic type complexity. Both start paths (foreground + daemon child) call it after loading config. `TraceLayer` + `SetRequestIdLayer` wrap the Axum router so every request is auto-logged with a `req_id` that propagates through all child spans.

**Tech Stack:** `tracing` 0.1, `tracing-subscriber` 0.3 (add `env-filter` + `json` features), `tower-http` 0.6 (add `trace` + `request-id` features), `tower` (already transitive via axum).

---

### Task 1: Update Cargo.toml dependencies

**Files:**
- Modify: `Cargo.toml`

**Step 1: Edit `Cargo.toml`**

Replace the existing `tracing-subscriber` line and add `tower-http`:

```toml
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }
tower-http = { version = "0.6", features = ["trace", "request-id"] }
```

**Step 2: Verify it compiles (no code change yet)**

```bash
cargo check 2>&1 | head -20
```

Expected: warnings only, no errors. If `tower-http` version conflicts, check `cargo tree -i tower-http` and pin accordingly.

**Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "chore(deps): add tower-http trace+request-id, tracing-subscriber features"
```

---

### Task 2: Add `log_level` and `log_format` to `Config`

**Files:**
- Modify: `src/config.rs`

**Step 1: Add fields to the struct**

In `src/config.rs`, add two fields after `service_key`:

```rust
pub struct Config {
    pub database_url: Option<String>,
    pub jwt_secret: String,
    pub port: u16,
    pub data_dir: String,
    pub storage_root: String,
    pub anon_key: String,
    pub service_key: String,
    pub log_level: String,   // SUPARUST_LOG_LEVEL
    pub log_format: String,  // SUPARUST_LOG_FORMAT
}
```

**Step 2: Parse them in `Config::from_env()`**

Add at the end of the `Self { ... }` block (before the closing brace):

```rust
log_level: env::var("SUPARUST_LOG_LEVEL")
    .unwrap_or_else(|_| "info".to_string()),
log_format: env::var("SUPARUST_LOG_FORMAT")
    .unwrap_or_else(|_| "pretty".to_string()),
```

**Step 3: Add vars to the auto-generated `.env` block in `load_or_generate_env()`**

In the `full_content` format string (fresh `.env` case), add after `SUPARUST_STORAGE_ROOT`:

```rust
let full_content = format!(
    "# SupaRust Environment\n\
    SUPARUST_PORT=3000\n\
    SUPARUST_DB_DATA_DIR=./data/postgres\n\
    SUPARUST_STORAGE_ROOT=./data/storage\n\
    SUPARUST_LOG_LEVEL=info\n\
    SUPARUST_LOG_FORMAT=pretty\n\
    {env_content}"
);
```

**Step 4: Verify compilation**

```bash
cargo check 2>&1 | grep "^error"
```

Expected: no output (no errors).

**Step 5: Commit**

```bash
git add src/config.rs
git commit -m "feat(config): add log_level and log_format env vars"
```

---

### Task 3: Create `src/tracing.rs` with `init_tracing()`

**Files:**
- Create: `src/tracing.rs`

**Step 1: Write the file**

```rust
use tracing_subscriber::{fmt, EnvFilter};

pub enum TracingWriter {
    Stdout,
    File(std::fs::File),
}

pub fn init_tracing(log_level: &str, log_format: &str, writer: TracingWriter) {
    let sqlx_level = if log_level == "trace" { "debug" } else { "warn" };
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new(format!(
            "suparust={log_level},sqlx={sqlx_level},tower_http=debug"
        ))
    });

    match (log_format, writer) {
        ("json", TracingWriter::Stdout) => {
            fmt()
                .json()
                .with_env_filter(filter)
                .with_current_span(true)
                .with_span_list(true)
                .init();
        }
        ("json", TracingWriter::File(f)) => {
            fmt()
                .json()
                .with_env_filter(filter)
                .with_current_span(true)
                .with_span_list(true)
                .with_writer(f)
                .with_ansi(false)
                .init();
        }
        (_, TracingWriter::Stdout) => {
            fmt()
                .pretty()
                .with_env_filter(filter)
                .with_file(true)
                .with_line_number(true)
                .init();
        }
        (_, TracingWriter::File(f)) => {
            fmt()
                .pretty()
                .with_env_filter(filter)
                .with_file(true)
                .with_line_number(true)
                .with_writer(f)
                .with_ansi(false)
                .init();
        }
    }
}
```

**Step 2: Register module in `src/main.rs`**

Add at the top of `src/main.rs` with the other `mod` declarations:

```rust
pub mod tracing;
```

**Step 3: Verify compilation**

```bash
cargo check 2>&1 | grep "^error"
```

Expected: no errors. Common issue: `fmt()` ambiguity with `tracing::fmt` — if it occurs, use fully-qualified `tracing_subscriber::fmt()`.

**Step 4: Commit**

```bash
git add src/tracing.rs src/main.rs
git commit -m "feat(tracing): add init_tracing() with TracingWriter enum"
```

---

### Task 4: Wire `init_tracing()` into start paths

**Files:**
- Modify: `src/cli/start.rs`

**Step 1: Update `cmd_start_foreground()`**

Replace:
```rust
pub async fn cmd_start_foreground() {
    tracing_subscriber::fmt::init();
```

With:
```rust
pub async fn cmd_start_foreground() {
    let cfg = crate::config::Config::from_env();
    crate::tracing::init_tracing(&cfg.log_level, &cfg.log_format, crate::tracing::TracingWriter::Stdout);
```

Note: `run_server()` also calls `Config::from_env()` — that's fine, it's idempotent (dotenvy ignores already-set vars).

**Step 2: Update `cmd_start_daemon_child()`**

Replace:
```rust
pub async fn cmd_start_daemon_child() {
    // Open app.log in append mode
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("app.log")
        .expect("Cannot open app.log");

    // Init tracing to write to file instead of stdout
    tracing_subscriber::fmt()
        .with_writer(log_file)
        .with_ansi(false)
        .init();
```

With:
```rust
pub async fn cmd_start_daemon_child() {
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("app.log")
        .expect("Cannot open app.log");

    let cfg = crate::config::Config::from_env();
    crate::tracing::init_tracing(&cfg.log_level, &cfg.log_format, crate::tracing::TracingWriter::File(log_file));
```

**Step 3: Verify compilation**

```bash
cargo check 2>&1 | grep "^error"
```

**Step 4: Commit**

```bash
git add src/cli/start.rs
git commit -m "feat(cli): wire init_tracing into foreground and daemon start paths"
```

---

### Task 5: Add `TraceLayer` + request ID to the Axum router

**Files:**
- Modify: `src/cli/start.rs` — `run_server()` function

**Step 1: Add imports at top of `src/cli/start.rs`**

```rust
use axum::http::Request;
use tower::ServiceBuilder;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::trace::TraceLayer;
```

**Step 2: Wrap the router in `run_server()`**

Replace the current `let app = Router::new()...` block with:

```rust
let app = Router::new()
    .nest("/rest/v1",    crate::api::rest::router(pool.clone(), cfg.jwt_secret.clone()))
    .nest("/auth/v1",    crate::api::auth::router(pool.clone(), cfg.jwt_secret.clone()))
    .nest("/storage/v1", crate::api::storage::router(
        pool.clone(),
        cfg.storage_root.clone(),
        cfg.jwt_secret.clone(),
    ))
    .layer(
        ServiceBuilder::new()
            .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
            .layer(PropagateRequestIdLayer::x_request_id())
            .layer(
                TraceLayer::new_for_http()
                    .make_span_with(|req: &Request<_>| {
                        let req_id = req
                            .headers()
                            .get("x-request-id")
                            .and_then(|v| v.to_str().ok())
                            .unwrap_or("unknown");
                        tracing::info_span!(
                            "http_request",
                            req_id = %req_id,
                            method = %req.method(),
                            path   = %req.uri().path(),
                        )
                    }),
            ),
    );
```

**Layer ordering is critical:** `SetRequestIdLayer` must be above `TraceLayer` — the UUID is set first, then the span reads it. Reversed order produces `req_id="unknown"` on every request.

**Step 3: Verify compilation**

```bash
cargo check 2>&1 | grep "^error"
```

Common issue: `tower::ServiceBuilder` not found — add `tower` to `Cargo.toml` explicitly if needed:
```toml
tower = { version = "0.5", features = ["util"] }
```
(Usually present transitively via axum, but explicit is cleaner if compiler complains.)

**Step 4: Smoke test — start the server and hit an endpoint**

```bash
cargo run -- start
# In another terminal:
curl -s http://localhost:3000/rest/v1/nonexistent -H "apikey: <anon_key>" | head
```

Expected in server log:
```
INFO http_request{req_id=<uuid> method=GET path=/rest/v1/nonexistent}: ...
```

**Step 5: Commit**

```bash
git add src/cli/start.rs Cargo.toml Cargo.lock
git commit -m "feat(api): add TraceLayer with request ID to Axum router"
```

---

### Task 6: Update `.env.example`

**Files:**
- Modify: `.env.example`

**Step 1: Add log vars**

Add a new section after the port/storage block:

```
# Logging
SUPARUST_LOG_LEVEL=info   # trace | debug | info | warn | error
SUPARUST_LOG_FORMAT=pretty  # pretty | json
```

**Step 2: Commit**

```bash
git add .env.example
git commit -m "docs(env): add SUPARUST_LOG_LEVEL and SUPARUST_LOG_FORMAT"
```

---

### Task 7: Final verification

**Step 1: Full build**

```bash
cargo build 2>&1 | grep "^error"
```

Expected: no errors.

**Step 2: Test pretty format**

```bash
SUPARUST_LOG_FORMAT=pretty SUPARUST_LOG_LEVEL=debug cargo run -- start
```

Expected output shape:
```
 INFO suparust::cli::start: PID 12345 written to .suparust.pid
 INFO suparust::cli::start: Starting embedded PostgreSQL in ./data/postgres
 INFO http_request{req_id=abc123 method=GET path=/rest/v1/users}: tower_http::trace: started processing request
```

**Step 3: Test JSON format**

```bash
SUPARUST_LOG_FORMAT=json SUPARUST_LOG_LEVEL=info cargo run -- start
```

Expected: one JSON object per line, each with `"req_id"` field when handling requests.

**Step 4: Verify RUST_LOG override works**

```bash
RUST_LOG=warn cargo run -- start
```

Expected: only WARN and ERROR lines, no INFO startup messages.

**Step 5: Final commit if any fixups needed, then tag**

```bash
git log --oneline -6
```

---

## Backlog

**Option C — Daily rotating log file (`tracing-appender`)**
When `app.log` becomes too large for production use, replace `TracingWriter::File` in daemon mode with `tracing_appender::rolling::daily(".", "suparust.log")` as a non-blocking writer. Add `tracing-appender = "0.2"` to dependencies. Not needed now.
