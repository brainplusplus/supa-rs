# SupaRust Builtin CLI Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add `start`, `stop`, `restart`, `status`, and `logs` subcommands to the `suparust` binary using `clap`, with cross-platform PID-file-based process lifecycle management.

**Architecture:** Single binary with a new `src/cli/` module. `main.rs` parses the `Cli` struct and dispatches to `cmd_*` functions. Foreground mode writes PID on start and deletes on exit. Daemon mode re-spawns self with a hidden `--_daemon-child` flag and redirects tracing to `app.log`.

**Tech Stack:** Rust, clap 4 (derive), tracing-subscriber (already present), dotenvy (already present), std::process, std::net::TcpStream

---

## Task 1: Add `clap` dependency

**Files:**
- Modify: `Cargo.toml`

**Step 1: Add dependency**

In `Cargo.toml`, add to `[dependencies]`:
```toml
clap = { version = "4", features = ["derive"] }
```

**Step 2: Verify it compiles**

```bash
cargo check
```
Expected: no errors. `clap` downloads and compiles.

**Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "chore: add clap dependency for CLI subcommands"
```

---

## Task 2: Create `src/cli/mod.rs` — Cli struct and Command enum

**Files:**
- Create: `src/cli/mod.rs`
- Create: `src/cli/start.rs` (stub)
- Create: `src/cli/stop.rs` (stub)
- Create: `src/cli/status.rs` (stub)
- Create: `src/cli/logs.rs` (stub)

**Step 1: Create `src/cli/mod.rs`**

```rust
pub mod start;
pub mod stop;
pub mod status;
pub mod logs;

#[derive(clap::Parser)]
#[command(name = "suparust", about = "SupaRust — Supabase-compatible backend")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(clap::Subcommand)]
pub enum Command {
    /// Start the server (foreground by default)
    Start {
        /// Run as background daemon, logs → app.log
        #[arg(long)]
        daemon: bool,

        /// Internal: child process spawned by --daemon (hidden)
        #[arg(long, hide = true)]
        daemon_child: bool,
    },
    /// Stop the running server
    Stop,
    /// Stop then restart as daemon
    Restart,
    /// Show server status (PID and port)
    Status,
    /// Tail app.log (daemon mode logs)
    Logs {
        /// Number of lines to show before following
        #[arg(long, default_value = "20")]
        lines: usize,
    },
}
```

**Step 2: Create stub files**

`src/cli/start.rs`:
```rust
pub async fn cmd_start_foreground() {
    todo!("foreground start")
}
pub async fn cmd_start_daemon() {
    todo!("daemon start")
}
pub async fn cmd_start_daemon_child() {
    todo!("daemon child")
}
```

`src/cli/stop.rs`:
```rust
pub fn cmd_stop() {
    todo!("stop")
}
```

`src/cli/status.rs`:
```rust
pub fn cmd_status() {
    todo!("status")
}
```

`src/cli/logs.rs`:
```rust
pub fn cmd_logs(_lines: usize) {
    todo!("logs")
}
```

**Step 3: Wire into `src/main.rs`**

Add at top of `main.rs`:
```rust
pub mod cli;
use clap::Parser;
use cli::{Cli, Command};
```

Replace `#[tokio::main] async fn main()` body with:
```rust
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Some(Command::Start { daemon: true, .. }) => cli::start::cmd_start_daemon().await,
        Some(Command::Start { daemon_child: true, .. }) => cli::start::cmd_start_daemon_child().await,
        Some(Command::Start { .. }) | None => cli::start::cmd_start_foreground().await,
        Some(Command::Stop) => cli::stop::cmd_stop(),
        Some(Command::Restart) => {
            cli::stop::cmd_stop();
            cli::start::cmd_start_daemon().await;
        }
        Some(Command::Status) => cli::status::cmd_status(),
        Some(Command::Logs { lines }) => cli::logs::cmd_logs(lines),
    }

    Ok(())
}
```

**Step 4: Verify compiles (stubs with todo! are fine)**

```bash
cargo check
```
Expected: compiles cleanly, no errors.

**Step 5: Commit**

```bash
git add src/cli/ src/main.rs
git commit -m "feat(cli): scaffold Cli struct, Command enum, stub handlers"
```

---

## Task 3: Implement `cmd_start_foreground()`

**Files:**
- Modify: `src/cli/start.rs`
- Note: Extract existing `main()` server boot logic into this function

**Step 1: Move server startup logic from `main.rs` into `cmd_start_foreground()`**

The current `main.rs` body (after CLI dispatch) is the foreground server. Move it:

```rust
use crate::config::Config;
use crate::db::{embed::EmbeddedPostgres, pool::create_pool};
use axum::Router;

const PID_FILE: &str = ".suparust.pid";

pub async fn cmd_start_foreground() {
    tracing_subscriber::fmt::init();

    // Write PID
    let pid = std::process::id();
    std::fs::write(PID_FILE, pid.to_string()).ok();
    tracing::info!("PID {} written to {}", pid, PID_FILE);

    // Run server (extracted from old main)
    if let Err(e) = run_server().await {
        tracing::error!("Server error: {}", e);
    }

    // Clean up PID on exit
    std::fs::remove_file(PID_FILE).ok();
}

async fn run_server() -> Result<(), Box<dyn std::error::Error>> {
    let cfg = Config::from_env();

    let (conn_str, _embedded) = match cfg.database_url {
        Some(url) => {
            tracing::info!("Using external PostgreSQL: {}", url);
            (url.clone(), None)
        }
        None => {
            tracing::info!("Starting embedded PostgreSQL in {}", cfg.data_dir);
            let embedded = EmbeddedPostgres::start(&cfg.data_dir).await?;
            let cs = embedded.connection_string.clone();
            (cs, Some(embedded))
        }
    };

    let pool = create_pool(&conn_str).await?;
    tracing::info!("Database pool established");

    sqlx::migrate!("./migrations").run(&pool).await?;
    tracing::info!("Migrations complete");

    let app = Router::new()
        .nest("/rest/v1",    crate::api::rest::router(pool.clone(), cfg.jwt_secret.clone()))
        .nest("/auth/v1",    crate::api::auth::router(pool.clone(), cfg.jwt_secret.clone()))
        .nest("/storage/v1", crate::api::storage::router(
            pool.clone(),
            cfg.storage_root.clone(),
            cfg.jwt_secret.clone(),
        ));

    let addr = format!("0.0.0.0:{}", cfg.port);
    tracing::info!("SupaRust listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
```

**Step 2: Simplify `main.rs`**

`main.rs` should now just be dispatch — all server logic lives in `start.rs`. The `main()` body is only the match statement from Task 2.

**Step 3: Cargo check**

```bash
cargo check
```
Expected: clean compile.

**Step 4: Smoke test foreground**

```bash
cargo run
# Should start server as before on port 3000
# Ctrl+C to stop
# .suparust.pid should appear during run, disappear after
```

**Step 5: Commit**

```bash
git add src/cli/start.rs src/main.rs
git commit -m "feat(cli): implement cmd_start_foreground with PID file"
```

---

## Task 4: Implement `cmd_stop()`

**Files:**
- Modify: `src/cli/stop.rs`

**Step 1: Implement**

```rust
const PID_FILE: &str = ".suparust.pid";

pub fn cmd_stop() {
    let pid_str = match std::fs::read_to_string(PID_FILE) {
        Ok(s) => s,
        Err(_) => {
            println!("No server running (no {} found)", PID_FILE);
            return;
        }
    };

    let pid: u32 = match pid_str.trim().parse() {
        Ok(p) if p > 0 => p,
        _ => {
            println!("Invalid PID in {} — deleting", PID_FILE);
            std::fs::remove_file(PID_FILE).ok();
            return;
        }
    };

    kill_pid(pid);
    std::fs::remove_file(PID_FILE).ok();
    println!("SupaRust stopped (PID {})", pid);
}

fn kill_pid(pid: u32) {
    #[cfg(target_os = "windows")]
    {
        let result = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/F", "/T"])
            .output();
        if result.is_err() {
            println!("Warning: taskkill failed — process may already be gone");
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        // Send SIGTERM first
        let _ = std::process::Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .output();

        // Wait up to 5s for graceful shutdown
        for _ in 0..10 {
            std::thread::sleep(std::time::Duration::from_millis(500));
            // Check if still alive by sending signal 0
            let alive = std::process::Command::new("kill")
                .args(["-0", &pid.to_string()])
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            if !alive {
                return;
            }
        }

        // Fallback: SIGKILL
        let _ = std::process::Command::new("kill")
            .args(["-KILL", &pid.to_string()])
            .output();
    }
}
```

**Step 2: Cargo check**

```bash
cargo check
```

**Step 3: Manual test**

```bash
# Terminal 1:
cargo run -- start
# Note PID printed

# Terminal 2:
cargo run -- stop
# Expected: "SupaRust stopped (PID XXXXX)"
# Terminal 1 should have exited
```

**Step 4: Commit**

```bash
git add src/cli/stop.rs
git commit -m "feat(cli): implement cmd_stop with cross-platform PID kill"
```

---

## Task 5: Implement `cmd_status()`

**Files:**
- Modify: `src/cli/status.rs`

**Step 1: Implement**

```rust
pub fn cmd_status() {
    dotenvy::dotenv().ok();
    let port = std::env::var("SUPARUST_PORT")
        .or_else(|_| std::env::var("PORT"))
        .unwrap_or_else(|_| "3000".to_string());

    let pid = std::fs::read_to_string(".suparust.pid")
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "(no pid file)".to_string());

    let addr = format!("127.0.0.1:{}", port);
    let alive = std::net::TcpStream::connect_timeout(
        &addr.parse().unwrap_or_else(|_| "127.0.0.1:3000".parse().unwrap()),
        std::time::Duration::from_secs(1),
    )
    .is_ok();

    println!("PID:    {}", pid);
    println!("Port:   {}", port);
    println!("Status: {}", if alive { "RUNNING" } else { "STOPPED" });
}
```

**Step 2: Cargo check**

```bash
cargo check
```

**Step 3: Test**

```bash
# With server not running:
cargo run -- status
# Expected:
# PID:    (no pid file)
# Port:   3000
# Status: STOPPED

# With server running (cargo run -- start in another terminal):
cargo run -- status
# Expected:
# PID:    <some number>
# Port:   3000
# Status: RUNNING
```

**Step 4: Commit**

```bash
git add src/cli/status.rs
git commit -m "feat(cli): implement cmd_status with PID file and TCP port check"
```

---

## Task 6: Implement `cmd_start_daemon()` and `cmd_start_daemon_child()`

**Files:**
- Modify: `src/cli/start.rs`

**Step 1: Implement `cmd_start_daemon()` (parent side)**

Add to `src/cli/start.rs`:

```rust
pub async fn cmd_start_daemon() {
    // Check if already running
    if let Ok(pid_str) = std::fs::read_to_string(PID_FILE) {
        let pid: u32 = pid_str.trim().parse().unwrap_or(0);
        if pid > 0 {
            // Check if it's actually alive via TCP
            dotenvy::dotenv().ok();
            let port = std::env::var("SUPARUST_PORT").unwrap_or_else(|_| "3000".to_string());
            let addr = format!("127.0.0.1:{}", port);
            let alive = std::net::TcpStream::connect_timeout(
                &addr.parse().unwrap(),
                std::time::Duration::from_secs(1),
            ).is_ok();
            if alive {
                println!("Already running (PID {})", pid);
                std::process::exit(1);
            }
        }
    }

    // Get path to current binary
    let exe = std::env::current_exe().expect("Cannot determine executable path");

    // Spawn daemon child: same binary with --_daemon-child flag
    let child = std::process::Command::new(&exe)
        .args(["start", "--daemon-child"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("Failed to spawn daemon child");

    let child_pid = child.id();
    std::fs::write(PID_FILE, child_pid.to_string()).expect("Failed to write PID file");

    println!("Started SupaRust daemon (PID {})", child_pid);
    println!("Logs: app.log");
}
```

**Step 2: Implement `cmd_start_daemon_child()` (child side)**

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

    tracing::info!("SupaRust daemon child started (PID {})", std::process::id());

    if let Err(e) = run_server().await {
        tracing::error!("Server error: {}", e);
    }
}
```

**Step 3: Cargo check**

```bash
cargo check
```

**Step 4: Test daemon mode**

```bash
cargo build
./target/debug/suparust start --daemon
# Expected: "Started SupaRust daemon (PID XXXXX)" then returns to prompt

# Check it's running:
./target/debug/suparust status
# Expected: RUNNING

# Check logs:
./target/debug/suparust logs

# Stop it:
./target/debug/suparust stop
# Expected: "SupaRust stopped (PID XXXXX)"
```

**Step 5: Commit**

```bash
git add src/cli/start.rs
git commit -m "feat(cli): implement daemon start via self-respawn with app.log redirect"
```

---

## Task 7: Implement `cmd_logs()`

**Files:**
- Modify: `src/cli/logs.rs`

**Step 1: Implement**

```rust
use std::io::{Read, Seek, SeekFrom};

const LOG_FILE: &str = "app.log";

pub fn cmd_logs(lines: usize) {
    let mut file = match std::fs::File::open(LOG_FILE) {
        Ok(f) => f,
        Err(_) => {
            println!("No log file found — is server running in daemon mode?");
            println!("Start with: suparust start --daemon");
            return;
        }
    };

    // Seek to show last N lines
    let content = {
        let mut buf = String::new();
        file.read_to_string(&mut buf).ok();
        buf
    };

    let all_lines: Vec<&str> = content.lines().collect();
    let start = all_lines.len().saturating_sub(lines);
    for line in &all_lines[start..] {
        println!("{}", line);
    }

    // Seek to end for follow mode
    file.seek(SeekFrom::End(0)).ok();

    // Polling follow loop
    println!("--- following app.log (Ctrl+C to stop) ---");
    loop {
        let mut buf = Vec::new();
        file.read_to_end(&mut buf).ok();
        if !buf.is_empty() {
            print!("{}", String::from_utf8_lossy(&buf));
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}
```

**Step 2: Cargo check**

```bash
cargo check
```

**Step 3: Test**

```bash
# Start daemon first:
./target/debug/suparust start --daemon

# Tail logs:
./target/debug/suparust logs
# Expected: last 20 lines of app.log, then follows

./target/debug/suparust logs --lines 5
# Expected: last 5 lines then follows

# Stop:
./target/debug/suparust stop
```

**Step 4: Commit**

```bash
git add src/cli/logs.rs
git commit -m "feat(cli): implement cmd_logs with polling tail and follow mode"
```

---

## Task 8: Implement `restart`

**Files:**
- Modify: `src/main.rs` (dispatch already wired — verify `Restart` arm)

**Step 1: Verify restart dispatch in `main.rs`**

The `Restart` arm should already be:
```rust
Some(Command::Restart) => {
    cli::stop::cmd_stop();
    cli::start::cmd_start_daemon().await;
}
```

That's it — no additional code needed. `stop` kills the old PID, `start_daemon` re-spawns.

**Step 2: Test restart**

```bash
./target/debug/suparust start --daemon
./target/debug/suparust status   # RUNNING, PID=X

./target/debug/suparust restart
# Expected: "SupaRust stopped (PID X)" then "Started SupaRust daemon (PID Y)"

./target/debug/suparust status   # RUNNING, PID=Y
```

**Step 3: Commit if any changes were needed**

```bash
git add src/main.rs
git commit -m "feat(cli): wire restart subcommand (stop + start daemon)"
```

---

## Task 9: Final integration test and cleanup

**Step 1: Full workflow test**

```bash
cargo build --release

# Foreground mode
./target/release/suparust start &
sleep 2
./target/release/suparust status   # RUNNING
./target/release/suparust stop

# Daemon mode
./target/release/suparust start --daemon
./target/release/suparust status   # RUNNING
./target/release/suparust logs --lines 5
./target/release/suparust restart
./target/release/suparust status   # RUNNING with new PID
./target/release/suparust stop
./target/release/suparust status   # STOPPED
```

**Step 2: Test edge cases**

```bash
# Stop when not running
./target/release/suparust stop
# Expected: "No server running (no .suparust.pid found)"

# Start daemon when already running
./target/release/suparust start --daemon
./target/release/suparust start --daemon
# Expected: "Already running (PID X)" + exit 1

./target/release/suparust stop

# Logs when not in daemon mode
rm -f app.log
./target/release/suparust logs
# Expected: "No log file found — is server running in daemon mode?"
```

**Step 3: Final commit**

```bash
git add -A
git commit -m "feat(cli): complete CLI implementation — start/stop/restart/status/logs"
```

---

## Summary of Files

| File | Action |
|---|---|
| `Cargo.toml` | Add `clap = { version = "4", features = ["derive"] }` |
| `src/main.rs` | Add `mod cli`, parse `Cli`, dispatch subcommands |
| `src/cli/mod.rs` | `Cli` struct, `Command` enum |
| `src/cli/start.rs` | `cmd_start_foreground`, `cmd_start_daemon`, `cmd_start_daemon_child`, `run_server` |
| `src/cli/stop.rs` | `cmd_stop`, `kill_pid` |
| `src/cli/status.rs` | `cmd_status` |
| `src/cli/logs.rs` | `cmd_logs` |
