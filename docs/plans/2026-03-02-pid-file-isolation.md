# PID File Isolation Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace the hardcoded `.suparust.pid` with a per-instance filename derived from `SUPARUST_ENV` + port, eliminating collisions when production and test servers run concurrently.

**Architecture:** Add `env` and `pid_file` fields to `Config` struct. `pid_file` derives as `.suparust.{env}.{port}.pid` (overridable via `SUPARUST_PID_FILE`). All CLI subcommands (`start`, `stop`, `status`) read `pid_file` from config instead of a hardcoded constant. `gen-env-test.mjs` writes `SUPARUST_ENV=test` so test servers always produce `.suparust.test.53001.pid`.

**Tech Stack:** Rust (no new crates), Node.js (gen-env-test.mjs already exists).

---

## Task 1: Update `src/config.rs` — add `env` + `pid_file` fields

**Files:**
- Modify: `src/config.rs:8-18` (struct definition)
- Modify: `src/config.rs:69-86` (Self { ... } block)

**Step 1: Add fields to `Config` struct**

In the `Config` struct (lines 8–18), add two new fields after `log_format`:

```rust
pub struct Config {
    pub database_url: Option<String>,
    pub jwt_secret: String,
    pub port: u16,
    pub data_dir: String,
    pub storage_root: String,
    pub anon_key: String,
    pub service_key: String,
    pub log_level: String,
    pub log_format: String,
    pub env: String,      // SUPARUST_ENV, default "local"
    pub pid_file: String, // derived from env+port, or SUPARUST_PID_FILE override
}
```

**Step 2: Populate fields in `from_env()`**

After the `port` parse block (after line 67, before `Self {`), add derivation logic:

```rust
let env_name = env::var("SUPARUST_ENV")
    .unwrap_or_else(|_| "local".to_string());

let pid_file = env::var("SUPARUST_PID_FILE")
    .unwrap_or_else(|_| format!(".suparust.{}.{}.pid", env_name, port));
```

Then in `Self { ... }` block, add the two new fields:

```rust
Self {
    database_url: ...,
    jwt_secret,
    port,
    data_dir: ...,
    storage_root: ...,
    anon_key,
    service_key,
    log_level: ...,
    log_format,
    env: env_name,
    pid_file,
}
```

**Step 3: Verify it compiles**

```bash
cargo build 2>&1 | tail -5
```

Expected: compile errors in `start.rs` / `stop.rs` / `status.rs` because `PID_FILE` constant still exists — that's fine, we fix those next. No errors from `config.rs` itself.

**Step 4: Commit**

```bash
git add src/config.rs
git commit -m "feat(config): add env + pid_file fields with port+env naming"
```

---

## Task 2: Update `src/cli/start.rs` — use `cfg.pid_file`

**Files:**
- Modify: `src/cli/start.rs:9` (remove const)
- Modify: `src/cli/start.rs:11-35` (foreground cmd)
- Modify: `src/cli/start.rs:38-75` (daemon cmd)
- Modify: `src/cli/start.rs:77-92` (daemon child cmd)

**Step 1: Remove `PID_FILE` constant (line 9)**

Delete:
```rust
const PID_FILE: &str = ".suparust.pid";
```

**Step 2: Update `cmd_start_foreground`**

Currently `cfg` is loaded at line 12. Replace every use of `PID_FILE` with `cfg.pid_file.as_str()` (or `&cfg.pid_file`):

```rust
pub async fn cmd_start_foreground() {
    let cfg = crate::config::Config::from_env();
    crate::tracing::init_tracing(&cfg.log_level, &cfg.log_format, crate::tracing::TracingWriter::Stdout);

    let pid = std::process::id();
    std::fs::write(&cfg.pid_file, pid.to_string()).ok();
    tracing::info!("PID {} written to {}", pid, cfg.pid_file);

    if let Err(e) = run_server().await {
        let msg = e.to_string();
        if msg.contains("10048") || msg.contains("address in use") || msg.contains("Address already in use") {
            dotenvy::dotenv().ok();
            let port = std::env::var("SUPARUST_PORT")
                .or_else(|_| std::env::var("PORT"))
                .unwrap_or_else(|_| "3000".to_string());
            tracing::error!(
                "Port {} is already in use. Run `suparust stop` to kill the existing process.",
                port
            );
        } else {
            tracing::error!("Server error: {}", e);
        }
    }

    std::fs::remove_file(&cfg.pid_file).ok();
}
```

**Step 3: Update `cmd_start_daemon`**

This function needs `cfg` to know the pid_file path. Add `Config::from_env()` at top, replace `PID_FILE` references:

```rust
pub async fn cmd_start_daemon() {
    let cfg = crate::config::Config::from_env();

    // Check if already running
    if let Ok(pid_str) = std::fs::read_to_string(&cfg.pid_file) {
        let pid: u32 = pid_str.trim().parse().unwrap_or(0);
        if pid > 0 {
            let addr = format!("127.0.0.1:{}", cfg.port);
            let alive = std::net::TcpStream::connect_timeout(
                &addr.parse().unwrap_or_else(|_| "127.0.0.1:3000".parse().unwrap()),
                std::time::Duration::from_secs(1),
            )
            .is_ok();
            if alive {
                println!("Already running (PID {})", pid);
                std::process::exit(1);
            }
        }
    }

    // Get path to current binary
    let exe = std::env::current_exe().expect("Cannot determine executable path");

    // Spawn daemon child: same binary with --daemon-child flag
    let child = std::process::Command::new(&exe)
        .args(["start", "--daemon-child"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("Failed to spawn daemon child");

    let child_pid = child.id();
    std::fs::write(&cfg.pid_file, child_pid.to_string()).expect("Failed to write PID file");

    println!("Started SupaRust daemon (PID {})", child_pid);
    println!("Logs: app.log");
}
```

**Step 4: Update `cmd_start_daemon_child`**

This function already calls `Config::from_env()` at line 84. It does not reference `PID_FILE` directly (the daemon parent writes PID, child just runs the server). No change needed here — verify by reading the rest of the function.

**Step 5: Build check**

```bash
cargo build 2>&1 | grep "^error" | head -10
```

Expected: errors only from `stop.rs` and `status.rs` still using hardcoded string — not from `start.rs`.

**Step 6: Commit**

```bash
git add src/cli/start.rs
git commit -m "feat(start): use cfg.pid_file for all PID file operations"
```

---

## Task 3: Update `src/cli/stop.rs` — use derived pid path

**Files:**
- Modify: `src/cli/stop.rs:1-47`

**Step 1: Remove `PID_FILE` constant, call `Config::from_env()`**

Replace:
```rust
const PID_FILE: &str = ".suparust.pid";

pub fn cmd_stop() {
    match std::fs::read_to_string(PID_FILE) {
```

With:
```rust
pub fn cmd_stop() {
    let cfg = crate::config::Config::from_env();
    let pid_file = &cfg.pid_file;

    match std::fs::read_to_string(pid_file) {
```

**Step 2: Replace all `PID_FILE` references in `cmd_stop`**

Four occurrences — all become `pid_file`:

```rust
pub fn cmd_stop() {
    let cfg = crate::config::Config::from_env();
    let pid_file = &cfg.pid_file;

    match std::fs::read_to_string(pid_file) {
        Ok(pid_str) => {
            let pid: u32 = match pid_str.trim().parse() {
                Ok(p) if p > 0 => p,
                _ => {
                    println!("Invalid PID in {} — deleting", pid_file);
                    std::fs::remove_file(pid_file).ok();
                    return;
                }
            };

            let was_running = kill_pid(pid);
            std::fs::remove_file(pid_file).ok();

            if was_running {
                println!("SupaRust stopped (PID {})", pid);
            } else {
                println!("Process {} was not running — PID file cleaned up", pid);
            }
        }
        Err(_) => {
            // No PID file — try to find the process by port
            println!("No {} found — searching for process on port {}...", pid_file, cfg.port);

            match find_pid_on_port(&cfg.port.to_string()) {
                Some(pid) => {
                    if kill_pid(pid) {
                        println!("SupaRust stopped (PID {} found via port {})", pid, cfg.port);
                    } else {
                        println!("Process {} was not running", pid);
                    }
                }
                None => {
                    println!("No process found on port {} — server is not running", cfg.port);
                }
            }
        }
    }
}
```

Note: The `Err(_)` branch previously called `dotenvy::dotenv().ok()` and re-parsed port manually. Now `cfg` already has `port` — remove the duplicate dotenv/port parsing.

**Step 3: Build check**

```bash
cargo build 2>&1 | grep "^error" | head -10
```

Expected: error only from `status.rs`.

**Step 4: Commit**

```bash
git add src/cli/stop.rs
git commit -m "feat(stop): derive pid_file from Config instead of hardcoded constant"
```

---

## Task 4: Update `src/cli/status.rs` — use derived pid path

**Files:**
- Modify: `src/cli/status.rs:1-58`

**Step 1: Add Config call and replace hardcoded strings**

`status.rs` already calls `dotenvy::dotenv().ok()` and parses port manually. Replace the manual port parsing + hardcoded `.suparust.pid` string with `Config::from_env()`:

```rust
pub fn cmd_status() {
    let cfg = crate::config::Config::from_env();

    let pid_raw = std::fs::read_to_string(&cfg.pid_file)
        .map(|s| s.trim().to_string())
        .ok();

    let addr = format!("127.0.0.1:{}", cfg.port);
    let alive = std::net::TcpStream::connect_timeout(
        &addr.parse().unwrap_or_else(|_| "127.0.0.1:3000".parse().unwrap()),
        std::time::Duration::from_secs(1),
    )
    .is_ok();

    let status_line = if alive {
        let pid_part = pid_raw.as_deref().unwrap_or("?");
        let uptime_part = uptime_from_pid_file(&cfg.pid_file).unwrap_or_default();
        format!("RUNNING  (PID {}{})", pid_part, uptime_part)
    } else {
        "STOPPED".to_string()
    };

    let base = format!("http://localhost:{}", cfg.port);
    let anon_key = std::env::var("SUPARUST_ANON_KEY")
        .unwrap_or_else(|_| "(not set — check .env)".to_string());
    let service_key = std::env::var("SUPARUST_SERVICE_KEY")
        .unwrap_or_else(|_| "(not set — check .env)".to_string());

    println!("Status:      {}", status_line);
    if alive {
        println!("API URL:     {}/rest/v1", base);
        println!("Auth URL:    {}/auth/v1", base);
        println!("Storage URL: {}/storage/v1", base);
        println!("Anon key:    {}", anon_key);
        println!("Service key: {}", service_key);
    }
}
```

**Step 2: Update `uptime_from_pid_file` signature**

The function currently hardcodes `.suparust.pid`. Add a `pid_file: &str` parameter:

```rust
fn uptime_from_pid_file(pid_file: &str) -> Option<String> {
    let meta = std::fs::metadata(pid_file).ok()?;
    let modified = meta.modified().ok()?;
    let elapsed = modified.elapsed().ok()?;
    let s = elapsed.as_secs();
    let (h, m, s) = (s / 3600, (s % 3600) / 60, s % 60);
    let uptime = if h > 0 {
        format!("{}h {}m {}s", h, m, s)
    } else if m > 0 {
        format!("{}m {}s", m, s)
    } else {
        format!("{}s", s)
    };
    Some(format!(", uptime {}", uptime))
}
```

**Step 3: Full build check**

```bash
cargo build 2>&1 | tail -5
```

Expected: `Finished` — no errors.

**Step 4: Commit**

```bash
git add src/cli/status.rs
git commit -m "feat(status): derive pid_file from Config, pass to uptime helper"
```

---

## Task 5: Update `scripts/gen-env-test.mjs` + `.gitignore`

**Files:**
- Modify: `scripts/gen-env-test.mjs` (serverEnv template string)
- Modify: `.gitignore`

**Step 1: Add `SUPARUST_ENV=test` to server env output**

In `gen-env-test.mjs`, find the `serverEnv` template string. Add `SUPARUST_ENV=test` after the port line:

```js
const serverEnv = `# Auto-generated by scripts/gen-env-test.mjs — DO NOT EDIT manually
# Regenerate with: node scripts/gen-env-test.mjs [--regen]
SUPARUST_PORT=${TEST_PORT}
SUPARUST_ENV=test
SUPARUST_DB_DATA_DIR=./data/pg-test
SUPARUST_STORAGE_ROOT=./data/storage-test
SUPARUST_JWT_SECRET=${JWT_SECRET}
SUPARUST_ANON_KEY=${ANON_KEY}
SUPARUST_SERVICE_KEY=${SERVICE_KEY}
SUPARUST_LOG_LEVEL=info
SUPARUST_LOG_FORMAT=pretty
`
```

**Step 2: Verify generated output**

```bash
node scripts/gen-env-test.mjs
grep SUPARUST_ENV .env.test
```

Expected: `SUPARUST_ENV=test`

**Step 3: Update `.gitignore`**

Find and replace the `.suparust.pid` line in `.gitignore`:

```gitignore
# Before:
.suparust.pid

# After:
.suparust.*.pid
```

**Step 4: Commit**

```bash
git add scripts/gen-env-test.mjs .gitignore
git commit -m "feat(scripts): add SUPARUST_ENV=test to gen-env-test output; gitignore pid pattern"
```

---

## Task 6: Smoke verification

**Step 1: Run `suparust start` (foreground, Ctrl+C after a few seconds)**

```bash
./target/debug/suparust.exe start &
sleep 3
ls .suparust.*.pid
kill %1
```

Expected: file `.suparust.local.3000.pid` appears (because no `SUPARUST_ENV` set → fallback `local`).

**Step 2: Verify test env produces correct PID name**

```bash
# Source test env and start briefly
source .env.test 2>/dev/null || true
SUPARUST_ENV=test SUPARUST_PORT=53001 ./target/debug/suparust.exe start &
sleep 3
ls .suparust.*.pid
kill %1
```

Expected: `.suparust.test.53001.pid` present, `.suparust.local.3000.pid` absent.

**Step 3: Verify `suparust stop` works with new PID format**

```bash
./target/debug/suparust.exe start &
sleep 2
./target/debug/suparust.exe stop
ls .suparust.*.pid 2>/dev/null || echo "pid file cleaned up ok"
```

Expected: `pid file cleaned up ok`

**Step 4: Final build**

```bash
cargo build --release 2>&1 | tail -3
```

Expected: `Finished`.

**Step 5: Commit**

No new files — if all green:
```bash
git tag pid-isolation-done
```

---

## Summary of commits

```
feat(config): add env + pid_file fields with port+env naming
feat(start): use cfg.pid_file for all PID file operations
feat(stop): derive pid_file from Config instead of hardcoded constant
feat(status): derive pid_file from Config, pass to uptime helper
feat(scripts): add SUPARUST_ENV=test to gen-env-test output; gitignore pid pattern
```
