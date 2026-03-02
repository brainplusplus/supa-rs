# Design: CLI Profile System

**Date:** 2026-03-02
**Status:** Approved
**Phase:** 1.5

---

## Problem

`Config::from_env()` calls `dotenvy::dotenv()` hardcoded — always loads `.env`. This causes:

- Test contamination: `.env` vars leak into test/compat environments
- No way to select env file from CLI
- Multi-instance isolation incomplete (PID file already isolated, but env is not)

---

## Decision

**Profile-based env loading — CLI owns env, Config is a pure reader.**

Move all `dotenvy` calls out of `Config::from_env()` and into a single `load_env()` function called at CLI startup before any subcommand executes.

---

## CLI Interface

```
suparust start   [--profile <name>] [--env-file <path>] [--daemon]
suparust stop    [--profile <name>]
suparust status  [--profile <name>]
suparust restart [--profile <name>]
suparust logs    [--profile <name>]
```

`--profile` and `--env-file` live on the top-level `Cli` struct — not per-subcommand — because env must be loaded before any subcommand logic runs. All subcommands through the same entry point.

---

## `load_env()` Behaviour

```
--profile test       →  load .env.test only (hard error if not found)
--env-file /x.env    →  load /x.env only (hard error if not found)
(no flags)           →  load .env (silent ok if not found)
--profile + --env-file  →  error: cannot use both
```

**Isolation is total.** When a profile or env-file is specified, `.env` is never loaded. Zero overlay, zero leakage.

---

## PID File Resolution

Instance identity = `(profile, port)`. PID file:

```
.suparust.<profile>.<port>.pid
```

Profile is resolved from:
1. `--profile <name>` (CLI flag)
2. `SUPARUST_ENV` inside the env file (when `--env-file` is used without `--profile`)
3. Hard error if neither is available

**Case A — Profile mode (recommended):**
```
suparust start --profile test
→ loads: .env.test
→ pid:   .suparust.test.53001.pid
```

**Case B — Env-file with explicit profile:**
```
suparust start --env-file ./configs/custom.env --profile staging
→ loads: ./configs/custom.env
→ pid:   .suparust.staging.54321.pid
```

**Case C — Env-file with SUPARUST_ENV inside:**
```
suparust start --env-file ./configs/custom.env
# custom.env contains: SUPARUST_ENV=debug
→ loads: ./configs/custom.env
→ pid:   .suparust.debug.7001.pid
```

**Case D — Env-file, no profile anywhere → hard error:**
```
suparust start --env-file ./configs/custom.env
# file has no SUPARUST_ENV
→ error: SUPARUST_ENV not set. Use --profile or define SUPARUST_ENV in env file.
```

**Case E — Default (no flags):**
```
suparust start
→ loads: .env (silent ok if missing)
→ pid:   .suparust.local.<port>.pid  (SUPARUST_ENV defaults to "local")
```

---

## Behaviour Matrix

| Command | Env loaded | `.env` loaded? | Error if not found? |
|---|---|---|---|
| `suparust start` | `.env` | yes | silent ok |
| `suparust start --profile test` | `.env.test` only | ❌ | hard error |
| `suparust start --env-file /x.env` | `/x.env` only | ❌ | hard error |
| `--profile x --env-file y` | — | — | error: cannot use both |

---

## Architecture Change

**Before:**
```
Config::from_env() {
    dotenvy::dotenv().ok();   // hardcoded, always .env
    dotenvy::dotenv().ok();   // called again after potential key generation
    ...
}
```

**After:**
```
main() {
    let cli = Cli::parse();
    load_env(cli.profile.as_deref(), cli.env_file.as_deref());  // owns env loading
    // dispatch subcommand
}

Config::from_env() {
    // pure reader — no dotenvy calls
}
```

---

## Impact on Test Infrastructure

`globalSetup.js` currently injects env vars via `process.env` spread into child process. With profiles:

```js
// Before (hacky — dotenvy in Config still loads .env)
serverProcess = spawn('cargo', ['run', '--', 'start'], {
    env: { ...process.env, ...serverEnv },
})

// After (clean)
serverProcess = spawn('cargo', ['run', '--', 'start', '--profile', 'supabase.test'], {
    env: process.env,  // no injection needed
})
```

Teardown also switches from `SIGTERM` to `cargo run -- stop --profile <X>` to properly clean up PID file.

---

## Deliverables

1. `src/cli/mod.rs` — add `profile: Option<String>` + `env_file: Option<PathBuf>` to `Cli` struct
2. `src/main.rs` — add `load_env()` function, call before subcommand dispatch, pass profile/env_file through to subcommands that need it for PID resolution
3. `src/config.rs` — remove both `dotenvy::dotenv().ok()` calls from `Config::from_env()`
4. `src/cli/start.rs`, `stop.rs`, `status.rs` — accept profile/env_file for PID file resolution
5. `test-client/globalSetup.js` — use `--profile supabase.test` arg, teardown via `cargo run -- stop`
6. `scripts/start-test-server.sh` — use `--profile` flag
7. `scripts/gen-env-test.mjs` — update `.env.supabase.test` to include `SUPARUST_ENV=compat`

---

## Multi-Instance Outcome

```bash
suparust start --profile dev      # .suparust.dev.3000.pid
suparust start --profile test     # .suparust.test.53001.pid
suparust start --profile compat   # .suparust.compat.53002.pid

suparust status --profile test
suparust stop   --profile compat
```

All isolated. All deterministic. No cross-contamination possible.
