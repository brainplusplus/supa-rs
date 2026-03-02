# Supabase-Compatible Env Test Mode Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a second test mode (`supabase.test`) that runs the full 21-test suite against a server started with Supabase-style alias env vars (`JWT_SECRET`, `ANON_KEY`, `POSTGRES_PASSWORD`, etc.) instead of canonical `SUPARUST_*` vars — proving the compat layer works end-to-end.

**Architecture:** Vitest already supports arbitrary modes via `--mode`. `globalSetup.js` already reads mode dynamically from `process.env.__TEST_MODE`. Adding `--mode supabase.test` causes Vite to load `.env.supabase.test` files automatically — zero changes needed to globalSetup or test files. The new mode uses port 53002 and isolated DB/storage dirs (`pg-compat`, `storage-compat`) so both modes can run without conflict.

**Tech Stack:** Node.js (gen-env-test.mjs), Bash (start-test-server.sh), Vitest 4, Vite loadEnv.

---

## Isolation Matrix

| | Default (existing) | Compat (new) |
|---|---|---|
| Vitest mode | `test` | `supabase.test` |
| Port | `53001` | `53002` |
| DB data dir | `./data/pg-test` | `./data/pg-compat` |
| Storage root | `./data/storage-test` | `./data/storage-compat` |
| Env style | `SUPARUST_*` | Supabase aliases |
| Server env file | `.env.test` | `.env.supabase.test` |
| Client env file | `test-client/.env.test` | `test-client/.env.supabase.test` |
| npm script | `npm test` | `npm run test:compat` |
| Manual server | `start-test-server.sh` | `start-test-server.sh --compat` |

---

## Task 1: Update `.gitignore`

**Files:**
- Modify: `.gitignore`

**Step 1: Add compat env files to .gitignore**

Current `.gitignore` has:
```
.env.test
test-client/.env.test
```

Add immediately after those two lines:
```
.env.supabase.test
test-client/.env.supabase.test
```

**Step 2: Verify**

```bash
cd /d/Rust/SupaRust && grep "supabase.test" .gitignore
```

Expected:
```
.env.supabase.test
test-client/.env.supabase.test
```

**Step 3: Commit**

```bash
git add .gitignore
git commit -m "chore(gitignore): add .env.supabase.test files"
```

---

## Task 2: Update `scripts/gen-env-test.mjs`

**Files:**
- Modify: `scripts/gen-env-test.mjs`

**Step 1: Replace the file with this content**

```js
#!/usr/bin/env node
/**
 * gen-env-test.mjs
 *
 * Generates two pairs of env files from a single randomly-generated JWT secret:
 *
 *   Pair A — SUPARUST_* style (port 53001, pg-test / storage-test):
 *     .env.test               → suparust server config
 *     test-client/.env.test   → vitest client config
 *
 *   Pair B — Supabase alias style (port 53002, pg-compat / storage-compat):
 *     .env.supabase.test               → suparust server config (compat layer test)
 *     test-client/.env.supabase.test   → vitest client config
 *
 * Both pairs share the same JWT secret so token signing is consistent.
 * All four files are gitignored — this script is the single source of truth.
 *
 * Usage:
 *   node scripts/gen-env-test.mjs           # generate (preserve pg dirs)
 *   node scripts/gen-env-test.mjs --regen   # generate + wipe all test data dirs
 */
import crypto from 'crypto'
import fs     from 'fs'
import path   from 'path'

// ── Paths ──────────────────────────────────────────────────────────────────
const ROOT = path.resolve(import.meta.dirname, '..')

// Pair A — SUPARUST_* style
const PG_TEST_DIR      = path.join(ROOT, 'data', 'pg-test')
const STORAGE_TEST_DIR = path.join(ROOT, 'data', 'storage-test')
const SERVER_ENV       = path.join(ROOT, '.env.test')
const CLIENT_ENV       = path.join(ROOT, 'test-client', '.env.test')

// Pair B — Supabase compat style
const PG_COMPAT_DIR      = path.join(ROOT, 'data', 'pg-compat')
const STORAGE_COMPAT_DIR = path.join(ROOT, 'data', 'storage-compat')
const SERVER_COMPAT_ENV  = path.join(ROOT, '.env.supabase.test')
const CLIENT_COMPAT_ENV  = path.join(ROOT, 'test-client', '.env.supabase.test')

// ── Constants ──────────────────────────────────────────────────────────────
const TEST_PORT   = 53001
const COMPAT_PORT = 53002
const TEST_EMAIL  = 'test@suparust.dev'
const TEST_PASS   = 'Password123!'

// ── Flags ──────────────────────────────────────────────────────────────────
const isRegen = process.argv.includes('--regen')

// ── Step 1: Wipe test data dirs if --regen ─────────────────────────────────
// Preserves pg-embed binary cache (extracted/) — only wipe PostgreSQL cluster files.
// Binary cache is ~50MB and takes minutes to download; no need to re-download on regen.
function wipePgDir(dir, label) {
  if (!fs.existsSync(dir)) return
  for (const entry of fs.readdirSync(dir)) {
    if (entry === 'extracted') continue // preserve pg-embed binary cache (~50MB)
    fs.rmSync(path.join(dir, entry), { recursive: true, force: true })
  }
  console.log(`[gen-env-test] Wiped ${label} cluster data (preserved extracted/ binary cache)`)
}

function wipeStorageDir(dir, label) {
  if (!fs.existsSync(dir)) return
  fs.rmSync(dir, { recursive: true, force: true })
  console.log(`[gen-env-test] Deleted ${label}`)
}

if (isRegen) {
  wipePgDir(PG_TEST_DIR, 'pg-test')
  wipeStorageDir(STORAGE_TEST_DIR, 'storage-test')
  wipePgDir(PG_COMPAT_DIR, 'pg-compat')
  wipeStorageDir(STORAGE_COMPAT_DIR, 'storage-compat')
}

// ── Step 2: Generate JWT secret (shared by both pairs) ─────────────────────
const JWT_SECRET = crypto.randomBytes(32).toString('hex')
const iat = Math.floor(Date.now() / 1000)
const exp = iat + 10 * 365 * 24 * 3600  // 10 years — matches config.rs generate_jwt

function b64url(obj) {
  const str = typeof obj === 'string' ? obj : JSON.stringify(obj)
  return Buffer.from(str).toString('base64url')
}

function signJWT(role) {
  const header   = b64url({ alg: 'HS256', typ: 'JWT' })
  const payload  = b64url({ role, iss: 'suparust', iat, exp })
  const unsigned = `${header}.${payload}`
  const sig = crypto
    .createHmac('sha256', JWT_SECRET)
    .update(unsigned)
    .digest('base64url')
  return `${unsigned}.${sig}`
}

const ANON_KEY    = signJWT('anon')
const SERVICE_KEY = signJWT('service_role')

// ── Step 3: Write Pair A — SUPARUST_* style ────────────────────────────────
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
fs.writeFileSync(SERVER_ENV, serverEnv, 'utf8')
console.log(`[gen-env-test] Written ${SERVER_ENV}`)

const clientEnv = `# Auto-generated by scripts/gen-env-test.mjs — DO NOT EDIT manually
# Regenerate with: node scripts/gen-env-test.mjs [--regen]
SUPABASE_URL=http://127.0.0.1:${TEST_PORT}
SUPABASE_ANON_KEY=${ANON_KEY}
SUPABASE_SERVICE_KEY=${SERVICE_KEY}
TEST_EMAIL=${TEST_EMAIL}
TEST_PASSWORD=${TEST_PASS}
`
fs.writeFileSync(CLIENT_ENV, clientEnv, 'utf8')
console.log(`[gen-env-test] Written ${CLIENT_ENV}`)

// ── Step 4: Write Pair B — Supabase alias style ────────────────────────────
const serverCompatEnv = `# Auto-generated by scripts/gen-env-test.mjs — DO NOT EDIT manually
# Supabase-compatible alias style — tests SupaRust env compat layer end-to-end.
# Regenerate with: node scripts/gen-env-test.mjs [--regen]
PORT=${COMPAT_PORT}
JWT_SECRET=${JWT_SECRET}
ANON_KEY=${ANON_KEY}
SERVICE_ROLE_KEY=${SERVICE_KEY}
DATA_DIR=./data/pg-compat
STORAGE_ROOT=./data/storage-compat
`
fs.writeFileSync(SERVER_COMPAT_ENV, serverCompatEnv, 'utf8')
console.log(`[gen-env-test] Written ${SERVER_COMPAT_ENV}`)

const clientCompatEnv = `# Auto-generated by scripts/gen-env-test.mjs — DO NOT EDIT manually
# Supabase-compatible alias style — tests SupaRust env compat layer end-to-end.
# Regenerate with: node scripts/gen-env-test.mjs [--regen]
SUPABASE_URL=http://127.0.0.1:${COMPAT_PORT}
SUPABASE_ANON_KEY=${ANON_KEY}
SUPABASE_SERVICE_KEY=${SERVICE_KEY}
TEST_EMAIL=${TEST_EMAIL}
TEST_PASSWORD=${TEST_PASS}
`
fs.writeFileSync(CLIENT_COMPAT_ENV, clientCompatEnv, 'utf8')
console.log(`[gen-env-test] Written ${CLIENT_COMPAT_ENV}`)

// ── Summary ────────────────────────────────────────────────────────────────
console.log(`
[gen-env-test] Done!
  Pair A (SUPARUST_* style):
    Port    : ${TEST_PORT}
    PG dir  : ${PG_TEST_DIR}
    Storage : ${STORAGE_TEST_DIR}

  Pair B (Supabase compat style):
    Port    : ${COMPAT_PORT}
    PG dir  : ${PG_COMPAT_DIR}
    Storage : ${STORAGE_COMPAT_DIR}

  JWT Secret : ${JWT_SECRET.slice(0, 8)}...  (shared)
  Regen mode : ${isRegen}
`)
```

**Step 2: Run to verify both pairs are generated**

```bash
cd /d/Rust/SupaRust && node scripts/gen-env-test.mjs
```

Expected output includes:
```
[gen-env-test] Written .../.env.test
[gen-env-test] Written .../test-client/.env.test
[gen-env-test] Written .../.env.supabase.test
[gen-env-test] Written .../test-client/.env.supabase.test

  Pair A (SUPARUST_* style):
    Port    : 53001
  Pair B (Supabase compat style):
    Port    : 53002
```

**Step 3: Verify content of `.env.supabase.test`**

```bash
cat /d/Rust/SupaRust/.env.supabase.test
```

Expected: contains `PORT=53002`, `JWT_SECRET=...`, `ANON_KEY=...`, `SERVICE_ROLE_KEY=...` — no `SUPARUST_` prefix on any key.

**Step 4: Verify content of `test-client/.env.supabase.test`**

```bash
cat /d/Rust/SupaRust/test-client/.env.supabase.test
```

Expected: contains `SUPABASE_URL=http://127.0.0.1:53002`.

**Step 5: Commit**

```bash
git add scripts/gen-env-test.mjs
git commit -m "feat(scripts): gen-env-test generates both SUPARUST_* and Supabase compat env pairs"
```

---

## Task 3: Update `scripts/start-test-server.sh`

**Files:**
- Modify: `scripts/start-test-server.sh`

**Step 1: Replace the file with this content**

```bash
#!/usr/bin/env bash
# start-test-server.sh
#
# Starts SupaRust with test env. Two modes:
#
#   Default  — SUPARUST_* style, port 53001
#   --compat — Supabase alias style, port 53002 (tests env compat layer)
#
# Usage:
#   bash scripts/start-test-server.sh           # SUPARUST_* style
#   bash scripts/start-test-server.sh --compat  # Supabase compat style
#
# Requires env files (run gen-env-test.mjs first):
#   Default:  .env.test
#   Compat:   .env.supabase.test

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(dirname "$SCRIPT_DIR")"

# ── Mode selection ────────────────────────────────────────────────────────────
if [[ "${1:-}" == "--compat" ]]; then
  ENV_FILE="$ROOT/.env.supabase.test"
  MODE_LABEL="Supabase compat"
else
  ENV_FILE="$ROOT/.env.test"
  MODE_LABEL="SUPARUST_*"
fi

if [[ ! -f "$ENV_FILE" ]]; then
  echo "[start-test-server] ERROR: $ENV_FILE not found."
  echo "  Run: node scripts/gen-env-test.mjs"
  exit 1
fi

echo "[start-test-server] Mode: $MODE_LABEL"
echo "[start-test-server] Loading $ENV_FILE"

# Export all vars from env file — these win over .env
# because dotenvy skips vars already present in process env.
set -a
# shellcheck disable=SC1090
source "$ENV_FILE"
set +a

# Print the port — works for both SUPARUST_PORT and PORT aliases
PORT_VAL="${SUPARUST_PORT:-${PORT:-3000}}"
echo "[start-test-server] Starting SupaRust on port $PORT_VAL"

cd "$ROOT"
exec cargo run -- start
```

**Step 2: Verify syntax**

```bash
bash -n /d/Rust/SupaRust/scripts/start-test-server.sh
```

Expected: no output (syntax OK).

**Step 3: Commit**

```bash
git add scripts/start-test-server.sh
git commit -m "feat(scripts): start-test-server --compat flag for Supabase alias env mode"
```

---

## Task 4: Update `test-client/package.json`

**Files:**
- Modify: `test-client/package.json`

**Step 1: Add `test:compat` and `gen:test:regen` scripts**

Current `scripts` block:
```json
"scripts": {
  "test":           "vitest run --mode test --reporter=verbose",
  "test:staging":   "vitest run --mode staging --reporter=verbose",
  "test:ci":        "vitest run --mode ci --reporter=verbose",
  "gen:test":       "node ../scripts/gen-env-test.mjs",
  "gen:test:regen": "node ../scripts/gen-env-test.mjs --regen"
}
```

Replace with:
```json
"scripts": {
  "test":              "vitest run --mode test --reporter=verbose",
  "test:compat":       "vitest run --mode supabase.test --reporter=verbose",
  "test:staging":      "vitest run --mode staging --reporter=verbose",
  "test:ci":           "vitest run --mode ci --reporter=verbose",
  "gen:test":          "node ../scripts/gen-env-test.mjs",
  "gen:test:regen":    "node ../scripts/gen-env-test.mjs --regen"
}
```

The key insight: `--mode supabase.test` causes Vite's `loadEnv('supabase.test', dir, '')` to load `.env.supabase.test` automatically. `globalSetup.js` already passes `__TEST_MODE` through and uses it for `loadEnv` — zero changes needed there.

**Step 2: Commit**

```bash
git add test-client/package.json
git commit -m "feat(test-client): add test:compat script for Supabase alias env mode"
```

---

## Task 5: Run both test modes — verification

**Step 1: Regenerate env files (both pairs)**

```bash
cd /d/Rust/SupaRust && node scripts/gen-env-test.mjs
```

Expected: 4 files written, summary shows both pairs.

**Step 2: Run default test suite (regression check)**

```bash
cd /d/Rust/SupaRust/test-client && npm test 2>&1 | tail -10
```

Expected:
```
Tests  21 passed (21)
```

**Step 3: Run compat test suite**

```bash
cd /d/Rust/SupaRust/test-client && npm run test:compat 2>&1 | tail -15
```

Expected:
```
Tests  21 passed (21)
```

This is the key verification — 21/21 with Supabase-style `.env.supabase.test` proves the `env_any()` compat layer works end-to-end.

**Step 4: If compat tests fail — debug**

Check server startup logs in test output for `[WARN] Using legacy env`. These confirm `env_any()` is picking up the aliases correctly.

If server fails to start, the most likely cause is `PORT` (not `SUPARUST_PORT`) not being passed correctly — check `.env.supabase.test` content and ensure `PORT=53002` is present.

**Step 5: Commit if any fixes were needed**

```bash
git add -p
git commit -m "fix(compat-test): correct env var resolution in compat mode"
```

---

## Task 6: Add `.env.supabase.test.example` files

**Files:**
- Create: `.env.supabase.test.example`
- Create: `test-client/.env.supabase.test.example`

**Step 1: Create root `.env.supabase.test.example`**

```env
# .env.supabase.test.example — Supabase alias style for compat testing
# DO NOT copy values from here — run: node scripts/gen-env-test.mjs
PORT=53002
JWT_SECRET=<generated>
ANON_KEY=<generated>
SERVICE_ROLE_KEY=<generated>
DATA_DIR=./data/pg-compat
STORAGE_ROOT=./data/storage-compat
```

**Step 2: Create `test-client/.env.supabase.test.example`**

```env
# test-client/.env.supabase.test.example — Vitest client config for compat mode
# DO NOT copy values from here — run: node scripts/gen-env-test.mjs
SUPABASE_URL=http://127.0.0.1:53002
SUPABASE_ANON_KEY=<generated>
SUPABASE_SERVICE_KEY=<generated>
TEST_EMAIL=test@suparust.dev
TEST_PASSWORD=Password123!
```

**Step 3: Commit**

```bash
git add .env.supabase.test.example test-client/.env.supabase.test.example
git commit -m "docs(env): add .env.supabase.test.example files for compat test mode"
```
