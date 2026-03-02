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
| `SUPARUST_JWT_SECRET` | auto-generated | HS256 signing secret |
| `SUPARUST_ANON_KEY` | auto-generated | JWT for `anon` role |
| `SUPARUST_SERVICE_KEY` | auto-generated | JWT for `service_role` |
| `SUPARUST_DB_DATA_DIR` | `./data/postgres` | Embedded PG data directory |
| `SUPARUST_STORAGE_ROOT` | `./data/storage` | File storage root |
| `SUPARUST_DB_URL` | *(unset)* | External PG URL (disables embedded PG) |
| `SUPARUST_LOG_LEVEL` | `info` | `trace` \| `debug` \| `info` \| `warn` \| `error` |
| `SUPARUST_LOG_FORMAT` | `pretty` | `pretty` (dev) \| `json` (production) |

---

## 📡 API Reference

### Auth `/auth/v1`

| Method | Endpoint | Description |
|---|---|---|
| `POST` | `/auth/v1/signup` | Register with email + password |
| `POST` | `/auth/v1/token?grant_type=password` | Login, returns JWT session |
| `GET` | `/auth/v1/user` | Get current user (requires Bearer token) |

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

21 Vitest tests covering Auth, REST API, Storage, and RLS:

```bash
suparust start          # Start the server first

cd test-client
npm install
npx vitest run --reporter=verbose
```

```
Tests  21 passed (21)
```

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
  config.rs        — Config::from_env(), .env generation
  tracing.rs       — init_tracing(), TracingWriter enum
  cli/
    start.rs       — foreground + daemon start, HTTP router setup
    stop.rs        — stop via PID file or port scan fallback
    status.rs      — status with endpoints + key display
    logs.rs        — tail app.log
  api/
    rest.rs        — PostgREST-compatible CRUD handlers
    auth.rs        — signup / login / getUser
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
docs/
  observability.md — Log forwarding guide (Vector, Loki, Datadog, journald)
  plans/           — Implementation design docs
test-client/       — Vitest integration test suite
```

---

## 🗺️ Roadmap

- [x] REST API (PostgREST-compatible)
- [x] Auth (JWT + Argon2)
- [x] Storage (multipart, RLS-gated)
- [x] Row-Level Security
- [x] Structured logging with request ID correlation
- [ ] Realtime WebSockets (logical replication → `axum::ws`)
- [ ] Edge Functions (`wasmtime` or V8 isolate)
- [ ] Local Studio UI dashboard

---

## 📄 License

MIT
