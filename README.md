<div align="center">

# ⚡ SupaRust

**A Supabase-compatible backend in a single Rust binary.**

[![Rust](https://img.shields.io/badge/Rust-2021_Edition-orange?logo=rust)](https://www.rust-lang.org/)
[![Axum](https://img.shields.io/badge/Axum-0.8-blue)](https://github.com/tokio-rs/axum)
[![SQLx](https://img.shields.io/badge/SQLx-0.8-blue)](https://github.com/launchbadge/sqlx)
[![License](https://img.shields.io/badge/License-MIT-green)](LICENSE)

*Drop-in replacement for Supabase's REST, Auth, and Storage APIs — no Docker, no containers, one binary.*

</div>

---

## ✨ Features

| | Feature | Details |
|---|---|---|
| 🔋 | **Single binary** | One `suparust` executable — no containers, no sidecars |
| 🐘 | **Embedded PostgreSQL** | Auto-managed via `pg-embed`, data in `./data/postgres` |
| 🔌 | **supabase-js compatible** | Drop-in for `createClient('http://localhost:3000', ANON_KEY)` |
| 🔍 | **PostgREST REST API** | Filter, select, order, limit, offset, upsert via URL params |
| 🔐 | **Auth** | Signup, login, JWT sessions, Argon2 password hashing |
| 🗂️ | **Storage** | Multipart upload/download, bucket management, RLS-gated access |
| 🛡️ | **Row-Level Security** | Enforced via `SET LOCAL ROLE` + JWT claims per request |
| 🏗️ | **SeaQuery SQL builder** | Injection-safe AST-based query construction for the REST layer |
| 📊 | **Structured logging** | JSON logs with request ID correlation — plug into any log pipeline |
| 🔀 | **Multi-instance safe** | Per-env PID files — production and test can run concurrently |

---

## 🚀 Quick Start

### Build

```bash
cargo build --release
# Binary: ./target/release/suparust
```

### Run

```bash
# Foreground (logs to stdout, Ctrl+C to stop)
suparust start

# Background daemon (logs to app.log)
suparust start --daemon
```

On first run, SupaRust auto-generates a JWT secret and writes it to `.env`:

```
SUPARUST_JWT_SECRET=...
SUPARUST_ANON_KEY=eyJ...
SUPARUST_SERVICE_KEY=eyJ...
```

### Connect

```javascript
import { createClient } from '@supabase/supabase-js'

const supabase = createClient(
  'http://localhost:3000',
  process.env.SUPARUST_ANON_KEY
)
```

---

## 🖥️ CLI

```bash
suparust start              # Start server in foreground
suparust start --daemon     # Start as background daemon
suparust stop               # Stop server
suparust restart            # Stop + restart daemon
suparust status             # Show status, endpoints, and API keys
suparust logs               # Tail app.log (daemon mode)
suparust logs --lines 100   # Tail last N lines
```

### Environment Profiles

Isolate environments with `--profile`:

```bash
suparust start                    # .env → identity: local
suparust start --profile test     # .env.test only → identity: profile.test
suparust start --env-file dev.env # dev.env only   → identity: env.dev_env
```

The default identity `local` reflects the universal convention that `.env` = local development (Supabase, Next.js, Vite, docker-compose all use this). Every instance — including default — gets a deterministic PID file: `.suparust.<identity>.<port>.pid`.

All subcommands (`stop`, `status`, `restart`) accept `--profile` too.

**→ See [`docs/migrating-from-supabase.md`](docs/migrating-from-supabase.md#running-with-profiles-environment-isolation) for full profile reference.**

### `suparust status`

```
Status:      RUNNING  (PID 12345, uptime 2h 14m 33s)
API URL:     http://localhost:3000/rest/v1
Auth URL:    http://localhost:3000/auth/v1
Storage URL: http://localhost:3000/storage/v1
Anon key:    eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9...
Service key: eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9...
```

---

## ⚙️ Configuration

All config via `.env` or environment variables:

| Variable | Default | Description |
|---|---|---|
| `SUPARUST_PORT` | `3000` | HTTP listen port |
| `SUPARUST_ENV` | `local` | Environment name — used in PID filename |
| `SUPARUST_JWT_SECRET` | auto-generated | HS256 signing secret |
| `SUPARUST_ANON_KEY` | auto-generated | JWT for `anon` role |
| `SUPARUST_SERVICE_KEY` | auto-generated | JWT for `service_role` |
| `SUPARUST_DB_DATA_DIR` | `./data/postgres` | Embedded PG data directory |
| `SUPARUST_STORAGE_ROOT` | `./data/storage` | File storage root |
| `SUPARUST_DB_URL` | *(unset)* | External PG URL (disables embedded PG) |
| `SUPARUST_LOG_LEVEL` | `info` | `trace` \| `debug` \| `info` \| `warn` \| `error` |
| `SUPARUST_LOG_FORMAT` | `pretty` | `pretty` (dev) \| `json` (production) |
| `SUPARUST_PID_FILE` | *(derived)* | Override PID file path (Docker / systemd) |

### Multi-Instance PID Isolation

SupaRust derives a unique PID filename per instance from `SUPARUST_ENV` + port — so production and test servers coexist without collision:

| Instance | `SUPARUST_ENV` | Port | PID file |
|---|---|---|---|
| Local dev (default) | `local` | 3000 | `.suparust.local.3000.pid` |
| Test runner | `test` | 53001 | `.suparust.test.53001.pid` |
| Staging | `staging` | 8080 | `.suparust.staging.8080.pid` |
| Production | `prod` | 3000 | `.suparust.prod.3000.pid` |

`SUPARUST_PID_FILE` overrides the derived path entirely (useful for Docker, systemd socket activation, etc.).

---

## 📡 API Reference

### Auth `/auth/v1`

| Method | Endpoint | Description |
|---|---|---|
| `POST` | `/auth/v1/signup` | Register with email + password |
| `POST` | `/auth/v1/token?grant_type=password` | Login, returns JWT session |
| `GET` | `/auth/v1/user` | Get current user (requires Bearer token) |
| `GET` | `/auth/v1/health` | Health check — confirms DB + migrations ready |

### REST `/rest/v1`

Follows [PostgREST](https://postgrest.org) conventions:

```bash
GET  /rest/v1/users?select=id,email&role=eq.admin   # Filter + select
POST /rest/v1/users                                  # Insert
PATCH /rest/v1/users?id=eq.1                         # Update
DELETE /rest/v1/users?id=eq.1                        # Delete
```

**Supported operators:** `eq`, `neq`, `lt`, `lte`, `gt`, `gte`, `like`, `ilike`, `is`, `in`, `not.in`, `cs`, `cd`, `fts`, `and()`, `or()`

### Storage `/storage/v1`

```bash
GET    /storage/v1/bucket              # List buckets
POST   /storage/v1/bucket             # Create bucket
POST   /storage/v1/object/{bucket}/*  # Upload file (multipart)
GET    /storage/v1/object/{bucket}/*  # Download file
DELETE /storage/v1/object/{bucket}    # Delete files (JSON body: {prefixes:[...]})
```

---

## 📊 Observability

SupaRust emits structured JSON logs with request ID correlation — no embedded log shipper required:

```json
{"timestamp":"...","level":"INFO","target":"suparust::api::rest","req_id":"a1b2c3","method":"GET","path":"/rest/v1/users","message":"..."}
```

Every HTTP request gets a unique `req_id` that propagates through all log lines for that request — making concurrent request logs trivially correlatable.

```bash
# Development — human-readable, file + line numbers
SUPARUST_LOG_LEVEL=debug SUPARUST_LOG_FORMAT=pretty suparust start

# Production — JSON for log aggregators
SUPARUST_LOG_LEVEL=info SUPARUST_LOG_FORMAT=json suparust start --daemon

# Fine-grained override
RUST_LOG=suparust=debug,sqlx=debug suparust start
```

**→ See [`docs/observability.md`](docs/observability.md) for integration guides:** Vector, Grafana Loki + Promtail, Datadog, and systemd journald.

---

## 🧪 Integration Tests

21 Vitest tests covering Auth, REST API, Storage, and RLS — server starts and stops automatically.

### Prerequisites

- Rust toolchain (`cargo`)
- Node.js 18+

### First-Time Setup

Generate isolated test environment (separate port + database):

```bash
node scripts/gen-env-test.mjs
```

This creates:
- `.env.test` — server config (port 53001, `SUPARUST_ENV=test`, isolated pg-embed at `data/pg-test/`)
- `test-client/.env.test` — client config with matching JWT keys

### Run Tests

```bash
cd test-client && npm test
```

Server starts automatically on port 53001, all 21 tests run, server stops on completion.

> **First run:** pg-embed downloads its binary (~50MB). This takes 2–5 minutes.
> Subsequent runs start in seconds.

```
Tests  21 passed (21)
```

### Reset Test Environment

```bash
# Regenerate JWT secret + wipe test database
node scripts/gen-env-test.mjs --regen
```

Use `--regen` when:
- Auth tests fail unexpectedly (JWT secret drift)
- Test database is in a bad state
- Switching branches with schema changes

---

## 🏛️ Stack

| Layer | Library |
|---|---|
| HTTP server | `axum 0.8` |
| Async runtime | `tokio 1` |
| Database driver | `sqlx 0.8` — async, compile-time checked queries |
| SQL builder | `sea-query 0.32` + `sea-query-binder 0.7` |
| Filter parser | `nom 7` — PostgREST filter/select/order syntax |
| Embedded PG | `pg-embed 1.0` |
| Auth | `jsonwebtoken 9`, `argon2 0.5` |
| Logging | `tracing 0.1`, `tracing-subscriber 0.3`, `tower-http 0.6` |
| CLI | `clap 4` |

---

## 📁 Project Structure

```
src/
  main.rs          — CLI dispatch
  config.rs        — Config::from_env(), env + pid_file derivation
  tracing.rs       — init_tracing(), TracingWriter enum
  cli/
    start.rs       — foreground + daemon start, HTTP router setup
    stop.rs        — stop via PID file or port scan fallback
    status.rs      — status with endpoints + key display
    logs.rs        — tail app.log
  api/
    rest.rs        — PostgREST-compatible CRUD handlers
    auth.rs        — signup / login / getUser / health
    storage.rs     — bucket + object management
  parser/
    filter.rs      — nom parser for col.op.val filter syntax
    select.rs      — nom parser for select= column list
    order.rs       — order= parser
  sql/
    ast.rs         — QueryAst, Operation, CountMethod
    builder.rs     — SeaQuery AST builders
    rls.rs         — RlsContext → SET LOCAL statements
  db/
    embed.rs       — EmbeddedPostgres via pg-embed
    pool.rs        — sqlx PgPool creation
    execute.rs     — execute_query() with RLS context injection
migrations/        — 6 SQL migration files (roles, auth, storage, RLS, grants)
scripts/
  gen-env-test.mjs — generates .env.test + test-client/.env.test
docs/
  observability.md — Log forwarding guide (Vector, Loki, Datadog, journald)
  plans/           — Implementation design docs
test-client/       — Vitest integration test suite (21 tests)
```

---

## 🗺️ Roadmap

- [x] REST API (PostgREST-compatible)
- [x] Auth (JWT + Argon2)
- [x] Storage (multipart, RLS-gated)
- [x] Row-Level Security
- [x] Structured logging with request ID correlation
- [x] Multi-instance PID isolation (env + port based)
- [x] Integration test suite (21 Vitest tests, auto start/stop)
- [ ] Realtime WebSockets (logical replication → `axum::ws`)
- [ ] Edge Functions (`wasmtime` or V8 isolate)
- [ ] Local Studio UI dashboard

---

## 📄 License

MIT
