# SupaRust Builtin CLI Design

**Date:** 2026-03-01
**Status:** Approved
**Scope:** Add `clap`-based subcommand CLI to the existing `suparust` binary

---

## 1. Problem Statement

During development, stopping/restarting SupaRust requires manual `taskkill` (Windows) or `kill` (Unix) commands because there is no built-in lifecycle management. A builtin CLI with `start`, `stop`, `restart`, `status`, and `logs` subcommands solves this cleanly and cross-platform.

---

## 2. Architecture

Single binary — no separate crate. All CLI logic lives in a new `src/cli/` module.

```
src/
  cli/
    mod.rs       ← Cli struct, Command enum (clap derive), dispatch
    start.rs     ← cmd_start() foreground + --daemon spawn logic
    stop.rs      ← cmd_stop() reads PID file, kills process gracefully
    status.rs    ← cmd_status() checks PID liveness + TCP port
    logs.rs      ← cmd_logs() polling tail of app.log
  main.rs        ← parse Cli, dispatch to cmd_* or run server inline
  config.rs      ← unchanged
```

**New dependency:** `clap = { version = "4", features = ["derive"] }`
No other new dependencies required.

---

## 3. CLI Interface

```
suparust                   # backward compat: runs foreground server
suparust start             # foreground server, logs to stdout
suparust start --daemon    # background daemon, logs → app.log, PID → .suparust.pid
suparust stop              # read .suparust.pid, kill gracefully
suparust restart           # stop + start --daemon
suparust status            # is PID alive? is port responding?
suparust logs              # tail app.log (last 20 lines, then follow)
suparust logs --lines 50   # tail last N lines then follow
```

Hidden internal flag (not shown in `--help`):
```
suparust start --_daemon-child   # used internally by --daemon parent spawn
```

---

## 4. Data Flow

### 4a. Foreground start (`suparust` / `suparust start`)
1. Load `.env` via `dotenvy`
2. Write `std::process::id()` → `.suparust.pid`
3. Init `tracing_subscriber` writing to stdout (current behavior)
4. Run axum server — blocks until Ctrl+C or signal
5. On clean exit: delete `.suparust.pid`

### 4b. Daemon start (`suparust start --daemon`)
**Parent process:**
1. Check `.suparust.pid` — if exists and PID is alive, warn and exit
2. Spawn `suparust start --_daemon-child` with `Stdio::null()` for stdin/stdout/stderr
3. Write child PID → `.suparust.pid`
4. Print `Started SupaRust daemon (PID 12345)`
5. Parent exits

**Child process (`--_daemon-child`):**
1. Open `app.log` in append mode → `File` handle
2. Init `tracing_subscriber::fmt()` with `.with_writer(file_handle)` (no stdout)
3. Load `.env`, build pool, run migrations, bind axum — server runs until killed

### 4c. Stop (`suparust stop`)
1. Read `.suparust.pid` → parse PID
2. If no PID file: print `No server running (no PID file)` — exit 0
3. Platform kill:
   - **Windows:** `taskkill /PID <pid> /F /T` (kills entire process tree → takes down embedded postgres)
   - **Unix:** `kill -TERM <pid>`, wait 5s, `kill -KILL <pid>` if still alive
4. Delete `.suparust.pid`
5. Print `SupaRust stopped (PID 12345)`

### 4d. Status (`suparust status`)
1. Load `.env` for port
2. Read `.suparust.pid` → get PID string (or `"(no pid file)"`)
3. TCP connect to `127.0.0.1:<port>` with 1s timeout → alive boolean
4. Print table: PID / Port / Status

### 4e. Logs (`suparust logs [--lines N]`)
1. Open `app.log` — if not found, print message and exit
2. Seek to show last N lines (default 20)
3. Print those lines
4. Enter polling loop: `read_to_end`, print new bytes, `sleep(200ms)` — repeat until Ctrl+C

---

## 5. Error Handling & Edge Cases

| Scenario | Behavior |
|---|---|
| `stop` — no PID file | Print warning, exit 0 (not error) |
| `stop` — stale PID (process dead) | Kill fails silently, delete PID file, print warning |
| `start --daemon` — already running | Warn "Already running (PID X)", exit 1 |
| `logs` — no `app.log` | Print "No log file — is server running in daemon mode?" |
| Port already in use | Existing axum bind error propagates normally |
| PID file location | Always relative to CWD (consistent with `.env` location) |
| `--_daemon-child` flag | Hidden from `--help`, internal-only |

---

## 6. Cross-Platform Kill Strategy

| Platform | Strategy |
|---|---|
| Windows | `taskkill /PID <pid> /F /T` — force-kills process tree |
| Linux / macOS | `kill -TERM <pid>` → 5s grace → `kill -KILL <pid>` |

The `/T` flag on Windows automatically kills the embedded postgres child process. On Unix, the `Drop` impl on `EmbeddedPostgres` handles postgres cleanup when the parent receives SIGTERM.

---

## 7. `logs` Implementation (Polling)

No `notify` crate. Simple polling loop:

```
open app.log
seek to last N lines
print them
loop:
  read_to_end → print new bytes if any
  sleep 200ms
```

Adequate for dev tool use. Zero additional dependencies.

---

## 8. Files Changed

| File | Change |
|---|---|
| `Cargo.toml` | Add `clap = { version = "4", features = ["derive"] }` |
| `src/main.rs` | Add `mod cli;`, parse `Cli`, dispatch subcommands |
| `src/cli/mod.rs` | `Cli` struct, `Command` enum, re-exports |
| `src/cli/start.rs` | `cmd_start()`, `cmd_start_daemon()`, `cmd_start_daemon_child()` |
| `src/cli/stop.rs` | `cmd_stop()` |
| `src/cli/status.rs` | `cmd_status()` |
| `src/cli/logs.rs` | `cmd_logs()` |

Total: 1 modified + 5 new files. Existing server logic in `main.rs` extracted into `cmd_start()` with minimal changes.
