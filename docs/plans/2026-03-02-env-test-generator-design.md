# Design: `.env.test` Generator + Ephemeral Test Database

**Date:** 2026-03-02
**Status:** Approved
**Scope:** Pure JS — no Rust changes

---

## Problem

Test files (`globalSetup.js`, `suparust.test.js`) hardcode JWT keys, base URL, and test credentials. No mechanism to run tests against an isolated database or change ports/ports without editing source files.

---

## Goals

1. Extract all hardcoded values into `.env.test` (auto-generated, gitignored)
2. Generator script produces consistent JWT keys shared between server and test client
3. Test server runs on a separate port with a separate pg-embed data directory
4. `--regen` flag wipes test database for a fresh start
5. Server env injected via shell env vars (Opsi B) — not a second `.env` file read

---

## Architecture

```
scripts/gen-env-test.mjs
    │
    ├── Generate JWT_SECRET (crypto.randomBytes)
    ├── Derive ANON_KEY + SERVICE_KEY (HS256, includes exp = iat + 10yr)
    ├── Write suparust/.env.test   ← SUPARUST_* vars
    ├── Write test-client/.env.test ← SUPABASE_* vars
    └── If --regen: rm -rf data/pg-test/ + data/storage-test/
```

### Server startup (Opsi B — env var injection)

`dotenvy::dotenv()` in `Config::from_env()` skips vars already present in the process environment. Injecting via `source .env.test && cargo run` means the shell-exported vars take priority over `.env`.

```
shell: source suparust/.env.test → sets SUPARUST_PORT=3001 etc.
  └─► cargo run  (suparust binary)
        └─► Config::from_env()
              dotenvy reads .env → skips SUPARUST_* already in env ✅
              SUPARUST_PORT=3001 (from shell) wins over .env value
```

### Test client env loading

```
vitest --mode test
  └─► vitest.config.js: loadEnv('test', cwd, '') from 'vitest/config'
        reads: .env + .env.test + .env.test.local (priority: highest last)
        sets: process.env.__TEST_MODE = mode  ← bridge for Node context
        returns: { test: { env } }  ← injected as import.meta.env in test files

globalSetup.js (Node context, outside Vite transform)
  └─► loadEnv(process.env.__TEST_MODE ?? 'test', cwd, '')
        same files, same values as vitest.config.js
```

---

## File Inventory

### New files

| File | Purpose |
|---|---|
| `scripts/gen-env-test.mjs` | Generator script |
| `suparust/.env.test` | Auto-generated, gitignored |
| `suparust/.env.test.example` | Committed documentation |
| `test-client/.env.test` | Auto-generated, gitignored |
| `test-client/.env.test.example` | Committed documentation |
| `scripts/start-test-server.sh` | Source env + start server |

### Modified files

| File | Change |
|---|---|
| `.gitignore` | Remove `.env.test`; add `suparust/.env.test`, `test-client/.env.test`, `data/pg-test/`, `data/storage-test/` |
| `test-client/vitest.config.js` | Switch to `defineConfig(({ mode }) => ...)` with `loadEnv` + `__TEST_MODE` bridge |
| `test-client/globalSetup.js` | `loadEnv` from `vitest/config`, dynamic mode, env vars from object |
| `test-client/suparust.test.js` | Replace all hardcoded strings with `import.meta.env.*` |
| `package.json` (root or test-client) | Add `gen:test`, `gen:test:regen`, `test:server` scripts |

---

## Key Implementation Details

### JWT must match Rust's `generate_jwt`

`config.rs` generates JWTs with `exp = iat + 10 years`. Generator script must produce identical format:

```js
function signJWT(payload, secret) {
  const header = { alg: 'HS256', typ: 'JWT' }
  const iat = Math.floor(Date.now() / 1000)
  const exp = iat + 10 * 365 * 24 * 3600   // ← required, matches Rust
  // ...
}
```

### `SUPARUST_DB_DATA_DIR` for test isolation

```
data/postgres/   ← production (default, SUPARUST_DB_DATA_DIR not set)
data/pg-test/    ← test (SUPARUST_DB_DATA_DIR=./data/pg-test in suparust/.env.test)
data/storage/    ← production storage
data/storage-test/ ← test storage
```

### `--regen` vs default behavior

| Command | JWT regeneration | pg-test wiped |
|---|---|---|
| `node scripts/gen-env-test.mjs` | ✅ new secret | ❌ preserved |
| `node scripts/gen-env-test.mjs --regen` | ✅ new secret | ✅ deleted |

### `.gitignore` final state (relevant lines)

```gitignore
.env
suparust/.env.test
test-client/.env.test
.env.local
.env.*.local
data/pg-test/
data/storage-test/
```

---

## Workflow

```bash
# One-time setup
node scripts/gen-env-test.mjs

# Start test server (separate terminal)
bash scripts/start-test-server.sh

# Run tests
cd test-client && npm test

# Fresh start (new JWT + wipe test DB)
node scripts/gen-env-test.mjs --regen
# restart server, run tests again
```

---

## Out of Scope

- Rust changes to `Config::from_env()`
- CI pipeline integration (deferred — `--mode ci` flag hook is available when needed)
- Auto-start server as part of test run (vitest `globalSetup` cannot start a Cargo process portably)
