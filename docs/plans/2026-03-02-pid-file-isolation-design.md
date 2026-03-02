# Design: Port-based PID File Isolation

**Date:** 2026-03-02
**Status:** Approved
**Scope:** Rust + scripts — no new dependencies

---

## Problem

`PID_FILE` is hardcoded as `.suparust.pid` in `start.rs`, `stop.rs`, and `status.rs`. When multiple instances run on the same machine (e.g., production on port 3000 and test on port 53001), they overwrite each other's PID file — breaking `suparust stop` and `suparust status`.

---

## Solution

PID filename is derived from two runtime values:

```
.suparust.{SUPARUST_ENV}.{port}.pid
```

| Instance | `SUPARUST_ENV` | Port | PID file |
|---|---|---|---|
| Developer local (no env set) | `local` (fallback) | 3000 | `.suparust.local.3000.pid` |
| Test runner | `test` | 53001 | `.suparust.test.53001.pid` |
| Staging | `staging` | 8080 | `.suparust.staging.8080.pid` |
| Production | `prod` | 3000 | `.suparust.prod.3000.pid` |

`SUPARUST_PID_FILE` remains available as a full override escape hatch (Docker, systemd socket activation, etc.).

**`local` as fallback** — not `prod` or `default`. SupaRust is a local-first tool; `prod` as a fallback would be actively misleading for developers who forget to set `SUPARUST_ENV`.

---

## Architecture

### `src/config.rs`

Add two fields:

```rust
pub env: String,      // SUPARUST_ENV, default "local"
pub pid_file: String, // derived or overridden via SUPARUST_PID_FILE
```

Derivation logic (port already parsed before this):

```rust
let env_name = env::var("SUPARUST_ENV")
    .unwrap_or_else(|_| "local".to_string());

let pid_file = env::var("SUPARUST_PID_FILE")
    .unwrap_or_else(|_| format!(".suparust.{}.{}.pid", env_name, port));
```

### `src/cli/start.rs`

Remove `const PID_FILE`. Call `Config::from_env()` once at the top of each command function (already done for foreground; daemon path needs it too). Use `cfg.pid_file` everywhere.

### `src/cli/stop.rs` + `src/cli/status.rs`

Both currently use a hardcoded string literal `".suparust.pid"`. Each must call `Config::from_env()` to derive the correct pid path. Minor cost: one extra env read on `stop`/`status` — acceptable.

### `scripts/gen-env-test.mjs`

Add `SUPARUST_ENV=test` to the server `.env.test` output so the test instance always produces `.suparust.test.53001.pid`.

### `.gitignore`

Replace `.suparust.pid` → `.suparust.*.pid` to cover all variants.

---

## Files Changed

| File | Change |
|---|---|
| `src/config.rs` | Add `env`, `pid_file` fields + derivation logic |
| `src/cli/start.rs` | Remove `PID_FILE` const, use `cfg.pid_file` |
| `src/cli/stop.rs` | Call `Config::from_env()`, use derived pid path |
| `src/cli/status.rs` | Call `Config::from_env()`, use derived pid path |
| `scripts/gen-env-test.mjs` | Add `SUPARUST_ENV=test` to server env output |
| `.gitignore` | `.suparust.pid` → `.suparust.*.pid` |

---

## Out of Scope

- Changing `stop.rs` port-scan fallback logic (already works fine)
- Any new env variables beyond `SUPARUST_ENV` and `SUPARUST_PID_FILE`
