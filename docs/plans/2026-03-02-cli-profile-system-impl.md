# CLI Profile System Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add `--profile <name>` and `--env-file <path>` flags to all SupaRust CLI commands, moving env loading out of `Config::from_env()` into a single `load_env()` function called at startup — making environments fully isolated and multi-instance deterministic.

**Architecture:** `load_env()` lives in `src/main.rs` and is called once before any subcommand runs. `Config::from_env()` loses its `dotenvy` calls and becomes a pure env var reader. Profile/env_file are passed through the CLI arg struct to all subcommands that need them for PID file resolution. Total isolation: when `--profile` or `--env-file` is given, `.env` is never loaded.

**Tech Stack:** Rust, clap 4, dotenvy, existing Config sub-structs.

---

## Task 1: Add `--profile` and `--env-file` to CLI struct + `load_env()`

**Files:**
- Modify: `src/cli/mod.rs`
- Modify: `src/main.rs`

**Step 1: Update `src/cli/mod.rs`**

Add `profile` and `env_file` to the top-level `Cli` struct, and add them to every subcommand variant that needs to pass them for PID resolution (`Start`, `Stop`, `Status`, `Restart`). `Logs` does not need them (it reads a file path, not config).

```rust
pub mod start;
pub mod stop;
pub mod status;
pub mod logs;

use std::path::PathBuf;

#[derive(clap::Parser)]
#[command(name = "suparust", about = "SupaRust — Supabase-compatible backend")]
pub struct Cli {
    /// Load environment from .env.<profile> (e.g. --profile test loads .env.test)
    #[arg(long, global = true)]
    pub profile: Option<String>,

    /// Load environment from a specific file path (advanced; cannot combine with --profile)
    #[arg(long = "env-file", global = true)]
    pub env_file: Option<PathBuf>,

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

Note: `global = true` on clap args means the flag can appear before or after the subcommand — `suparust --profile test start` and `suparust start --profile test` both work.

**Step 2: Update `src/main.rs`**

Add `load_env()` and call it before dispatch:

```rust
pub mod config;
pub mod api;
pub mod db;
pub mod parser;
pub mod sql;
pub mod cli;
pub mod tracing;

use clap::Parser;
use cli::{Cli, Command};
use std::path::Path;

fn load_env(profile: Option<&str>, env_file: Option<&Path>) {
    match (env_file, profile) {
        (Some(_), Some(_)) => {
            eprintln!("error: cannot use both --profile and --env-file");
            std::process::exit(1);
        }
        (Some(path), None) => {
            dotenvy::from_path(path).unwrap_or_else(|_| {
                eprintln!("error: env file not found: {}", path.display());
                std::process::exit(1);
            });
        }
        (None, Some(p)) => {
            let filename = format!(".env.{}", p);
            dotenvy::from_filename(&filename).unwrap_or_else(|_| {
                eprintln!("error: profile '{}' not found: {}", p, filename);
                std::process::exit(1);
            });
        }
        (None, None) => {
            dotenvy::dotenv().ok();
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    load_env(cli.profile.as_deref(), cli.env_file.as_deref());

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

**Step 3: Build — expect errors only in config.rs (dotenvy calls still there)**

```bash
cd /d/Rust/SupaRust && cargo build 2>&1 | grep "^error" | head -20
```

Expected: one or two errors pointing to `config.rs` dotenvy usage — those are addressed in Task 2. Zero errors in `main.rs` or `cli/mod.rs`.

**Step 4: Commit**

```bash
git add src/cli/mod.rs src/main.rs
git commit -m "feat(cli): add --profile and --env-file global flags + load_env()"
```

---

## Task 2: Remove `dotenvy` from `Config::from_env()`

**Files:**
- Modify: `src/config.rs`

**Step 1: Remove both `dotenvy::dotenv().ok()` calls**

Find and remove these two lines in `Config::from_env()`:

```rust
// Line ~25 — REMOVE:
dotenvy::dotenv().ok();

// Line ~37 — REMOVE (the "reload after generation" call):
dotenvy::dotenv().ok();
```

The function signature and all `env_any()` / `env_bool()` calls stay unchanged. Only the two `dotenvy` calls are removed.

After removal, `from_env()` starts with:

```rust
impl Config {
    pub fn from_env() -> Self {
        // ── JWT secret (must resolve first — keys are derived from it) ──────
        let jwt_secret_opt = env_any(&["SUPARUST_JWT_SECRET", "JWT_SECRET"]);
        // ... rest unchanged
    }
}
```

**Step 2: Full build must pass**

```bash
cargo build 2>&1
```

Expected: zero errors. If warnings about unused imports appear, check if `dotenvy` is used elsewhere in `config.rs` — it should not be after this change.

**Step 3: Verify `dotenvy` is still in `Cargo.toml` (it's still used in `main.rs`)**

```bash
grep "dotenvy" /d/Rust/SupaRust/Cargo.toml
```

Expected: `dotenvy` still listed. Do NOT remove it.

**Step 4: Commit**

```bash
git add src/config.rs
git commit -m "refactor(config): remove dotenvy from Config::from_env() — CLI owns env loading"
```

---

## Task 3: Update `globalSetup.js` — use `--profile` arg + `stop` on teardown

**Files:**
- Modify: `test-client/globalSetup.js`

**Step 1: Read current globalSetup.js to understand what needs changing**

Two things change:
1. Server spawn: replace env injection with `--profile supabase.test` arg
2. Teardown: replace `serverProcess.kill('SIGTERM')` with `cargo run -- stop --profile supabase.test`

**Step 2: Update server spawn (around line 63)**

Before:
```js
serverProcess = spawn('cargo', ['run', '--', 'start'], {
    cwd: ROOT,
    env: { ...process.env, ...serverEnv },  // env injection
    shell: true,
    stdio: ['ignore', 'pipe', 'pipe'],
})
```

After:
```js
// mode is already set to 'test' or 'supabase.test' by vitest --mode
// Pass it as --profile to cargo so Config gets the right env file, isolated
serverProcess = spawn('cargo', ['run', '--', '--profile', mode, 'start'], {
    cwd: ROOT,
    env: process.env,  // no env injection — profile flag handles isolation
    shell: true,
    stdio: ['ignore', 'pipe', 'pipe'],
})
```

**Step 3: Update teardown (around line 114)**

Before:
```js
export async function teardown() {
    if (serverProcess) {
        console.log('[globalSetup] Stopping test server...')
        serverProcess.kill('SIGTERM')
        await new Promise(r => setTimeout(r, 1500))
        console.log('[globalSetup] Server stopped.')
    }
}
```

After:
```js
export async function teardown() {
    if (serverProcess) {
        console.log('[globalSetup] Stopping test server...')
        // Use suparust stop --profile <mode> to cleanly remove PID file
        const { spawnSync } = await import('child_process')
        spawnSync('cargo', ['run', '--', '--profile', mode, 'stop'], {
            cwd: ROOT,
            env: process.env,
            shell: true,
            stdio: 'inherit',
        })
        await new Promise(r => setTimeout(r, 1000))
        console.log('[globalSetup] Server stopped.')
    }
}
```

**Step 4: Update `scripts/gen-env-test.mjs` — add `SUPARUST_ENV=compat` to Pair B**

The `.env.supabase.test` must contain `SUPARUST_ENV=compat` (or `supabase.test`) so that when loaded via `--profile supabase.test`, the PID file derives correctly. But wait — with `--profile supabase.test`, the profile name IS `supabase.test`, so `SUPARUST_ENV` in the file is actually not needed for PID resolution (PID uses the profile name from CLI, not `SUPARUST_ENV`).

However, the `SUPARUST_ENV` field in Config still needs to be set correctly for the `env` field. Update `serverCompatEnv` in `gen-env-test.mjs` to add:

```js
const serverCompatEnv = `# Auto-generated by scripts/gen-env-test.mjs — DO NOT EDIT manually
# Supabase-compatible alias style — tests SupaRust env compat layer end-to-end.
# Regenerate with: node scripts/gen-env-test.mjs [--regen]
SUPARUST_ENV=compat
PORT=${COMPAT_PORT}
JWT_SECRET=${JWT_SECRET}
ANON_KEY=${ANON_KEY}
SERVICE_ROLE_KEY=${SERVICE_KEY}
DATA_DIR=./data/pg-compat
STORAGE_ROOT=./data/storage-compat
`
```

**Step 5: Regenerate env files**

```bash
cd /d/Rust/SupaRust && node scripts/gen-env-test.mjs
```

Expected: 4 files written, `.env.supabase.test` now contains `SUPARUST_ENV=compat`.

**Step 6: Commit**

```bash
git add test-client/globalSetup.js scripts/gen-env-test.mjs
git commit -m "feat(test): use --profile flag in globalSetup; teardown via suparust stop"
```

---

## Task 4: Update `scripts/start-test-server.sh`

**Files:**
- Modify: `scripts/start-test-server.sh`

**Step 1: Replace env-injection approach with `--profile` flag**

Before: the script sourced the env file into shell vars and relied on `dotenvy` to pick them up.

After: pass `--profile` to cargo and let `load_env()` handle file loading.

```bash
#!/usr/bin/env bash
# start-test-server.sh
#
# Starts SupaRust with a specific profile.
#
# Usage:
#   bash scripts/start-test-server.sh                      # default .env
#   bash scripts/start-test-server.sh --profile test       # .env.test
#   bash scripts/start-test-server.sh --profile compat     # .env.compat
#
# Requires the env file to exist (run gen-env-test.mjs first).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(dirname "$SCRIPT_DIR")"

PROFILE_ARG=""
if [[ "${1:-}" == "--profile" && -n "${2:-}" ]]; then
  PROFILE_ARG="--profile ${2}"
  echo "[start-test-server] Profile: ${2}"
elif [[ "${1:-}" == "--compat" ]]; then
  # Legacy alias kept for backwards compat
  PROFILE_ARG="--profile supabase.test"
  echo "[start-test-server] Profile: supabase.test (via --compat)"
else
  echo "[start-test-server] Profile: default (.env)"
fi

cd "$ROOT"
exec cargo run -- $PROFILE_ARG start
```

**Step 2: Verify syntax**

```bash
bash -n /d/Rust/SupaRust/scripts/start-test-server.sh
```

Expected: no output.

**Step 3: Commit**

```bash
git add scripts/start-test-server.sh
git commit -m "feat(scripts): start-test-server uses --profile flag instead of env injection"
```

---

## Task 5: Integration verification — both test modes

**Step 1: Full build**

```bash
cd /d/Rust/SupaRust && cargo build 2>&1
```

Expected: zero errors, zero warnings.

**Step 2: Smoke test default mode**

```bash
# Quick manual check that profile flag works
cargo run -- --profile test stop 2>&1
```

Expected: either "stopped" or "not running" — no crash, flag is accepted.

**Step 3: Run default test suite**

```bash
cd /d/Rust/SupaRust/test-client && npm test 2>&1 | tail -10
```

Expected:
```
Tests  21 passed (21)
```

**Step 4: Run compat test suite**

```bash
cd /d/Rust/SupaRust/test-client && npm run test:compat 2>&1 | tail -15
```

Expected:
```
Tests  21 passed (21)
```

Server logs should show `[WARN] Using legacy env ...` confirming aliases are being picked up by `env_any()`.

**Step 5: Verify no contamination between modes**

Check that server in compat mode used port 53002 and `pg-compat` dir:

```bash
ls /d/Rust/SupaRust/data/pg-compat/ 2>&1 | head -5
```

Expected: postgres cluster files exist (created during compat test run).

**Step 6: Commit if any stragglers**

```bash
git add -p
git commit -m "chore: cleanup after profile system integration"
```

---

## Task 6: Add `.env.supabase.test.example` files + update `.env.test.example`

**Files:**
- Create: `.env.supabase.test.example`
- Create: `test-client/.env.supabase.test.example`
- Modify: `.env.test.example` (already exists — verify up to date)

**Step 1: Create `.env.supabase.test.example`**

```env
# .env.supabase.test.example — Supabase alias style for compat testing
# DO NOT copy values — run: node scripts/gen-env-test.mjs
# Used with: suparust start --profile supabase.test
SUPARUST_ENV=compat
PORT=53002
JWT_SECRET=<generated>
ANON_KEY=<generated>
SERVICE_ROLE_KEY=<generated>
DATA_DIR=./data/pg-compat
STORAGE_ROOT=./data/storage-compat
```

**Step 2: Create `test-client/.env.supabase.test.example`**

```env
# test-client/.env.supabase.test.example — vitest client config for compat mode
# DO NOT copy values — run: node scripts/gen-env-test.mjs
SUPABASE_URL=http://127.0.0.1:53002
SUPABASE_ANON_KEY=<generated>
SUPABASE_SERVICE_KEY=<generated>
TEST_EMAIL=test@suparust.dev
TEST_PASSWORD=Password123!
```

**Step 3: Commit**

```bash
git add .env.supabase.test.example test-client/.env.supabase.test.example
git commit -m "docs(env): add .env.supabase.test.example files"
```
