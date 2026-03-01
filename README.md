# supa-rs

> A Supabase-compatible backend compiled into a single Rust binary.

**supa-rs** implements the `@supabase/supabase-js` API surface — REST, Auth, and Storage — as a single native binary with an embedded PostgreSQL instance. No Docker, no multi-service setup.

## Features

- **Single binary** — one `suparust` executable, no containers required
- **Embedded PostgreSQL** — auto-managed via `pg-embed`, data persisted in `./data/postgres`
- **supabase-js compatible** — drop-in for `createClient('http://localhost:3000', ANON_KEY)`
- **PostgREST-compatible REST API** — filter, select, order, limit, offset, upsert via URL params
- **Auth** — signup, login, JWT sessions, Argon2 password hashing
- **Storage** — multipart file upload/download, bucket management, RLS-gated access
- **Row-Level Security** — enforced via `SET LOCAL ROLE` + `SET LOCAL request.jwt.claims` per request
- **SeaQuery SQL builder** — injection-safe AST-based query construction for the REST layer

## Stack

| Layer | Library |
|---|---|
| HTTP | `axum 0.8` |
| Database driver | `sqlx 0.8` (async, compile-time checked static queries) |
| SQL builder | `sea-query 0.32` + `sea-query-binder 0.7` (dynamic REST queries) |
| Parser | `nom 7` (PostgREST filter/select/order syntax) |
| Embedded PG | `pg-embed 1.0` |
| Auth | `jsonwebtoken 9`, `argon2 0.5` |
| CLI | `clap 4` |

## Getting Started

### Prerequisites

- Rust (stable, 2021 edition)
- PostgreSQL binaries available on `$PATH` (required by pg-embed)
- Node.js 18+ (for integration tests only)

### Build

```bash
cargo build --release
# Binary: ./target/release/suparust
```

### Run

```bash
# Foreground (logs to stdout, Ctrl+C to stop)
suparust start

# Daemon (logs to app.log, writes PID to .suparust.pid)
suparust start --daemon
```

On first run, supa-rs auto-generates a JWT secret and writes it to `.env`:

```
SUPARUST_JWT_SECRET=...
SUPARUST_ANON_KEY=eyJ...
SUPARUST_SERVICE_KEY=eyJ...
```

### Connect your client

```javascript
import { createClient } from '@supabase/supabase-js'

const supabase = createClient(
  'http://localhost:3000',
  process.env.SUPARUST_ANON_KEY
)
```

## CLI Commands

```bash
suparust start              # Start server in foreground
suparust start --daemon     # Start as background daemon
suparust stop               # Stop server (via PID file or port scan)
suparust restart            # Stop + start daemon
suparust status             # Show status, endpoints, and API keys
suparust logs               # Tail app.log (daemon mode)
suparust logs --lines 100   # Tail last 100 lines
```

### `suparust status` output

```
Status:      RUNNING  (PID 12345, uptime 2h 14m 33s)
API URL:     http://localhost:3000/rest/v1
Auth URL:    http://localhost:3000/auth/v1
Storage URL: http://localhost:3000/storage/v1
Anon key:    eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9...
Service key: eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9...
```

## Configuration

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

## Integration Tests

21 Vitest tests covering Auth, REST API, Storage, and RLS.

```bash
# Start the server first
suparust start

# Run tests
cd test-client
npm install
npx vitest run --reporter=verbose
```

Expected output:
```
Tests  21 passed (21)
```

## API Coverage

### Auth (`/auth/v1`)

| Endpoint | Method | Description |
|---|---|---|
| `/auth/v1/signup` | POST | Register with email + password |
| `/auth/v1/token?grant_type=password` | POST | Login, returns JWT session |
| `/auth/v1/user` | GET | Get current user (requires Bearer token) |

### REST (`/rest/v1`)

Follows PostgREST conventions:

```bash
# Select with filter
GET /rest/v1/users?select=id,email&role=eq.admin

# Insert
POST /rest/v1/users
Content-Type: application/json
{"email": "user@example.com", "role": "user"}

# Update with filter
PATCH /rest/v1/users?id=eq.1
{"role": "admin"}

# Delete with filter
DELETE /rest/v1/users?id=eq.1

# Prefer: return=minimal (no response body)
# Prefer: count=exact
```

Supported filter operators: `eq`, `neq`, `lt`, `lte`, `gt`, `gte`, `like`, `ilike`, `is`, `in`, `not.in`, `cs`, `cd`, `fts`, and logical `and()/or()`.

### Storage (`/storage/v1`)

```bash
# List buckets
GET /storage/v1/bucket

# Create bucket
POST /storage/v1/bucket
{"id": "avatars", "name": "avatars", "public": false}

# Upload file
POST /storage/v1/object/avatars/profile.jpg
Content-Type: multipart/form-data

# Download file
GET /storage/v1/object/avatars/profile.jpg

# Delete files
DELETE /storage/v1/object/avatars
{"prefixes": ["profile.jpg"]}
```

## Project Structure

```
src/
  main.rs          — CLI dispatch
  config.rs        — Config::from_env(), .env generation
  cli/
    start.rs       — foreground + daemon start
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
    builder.rs     — SeaQuery AST builders (build_select/insert/update/delete)
    rls.rs         — RlsContext → SET LOCAL statements
  db/
    embed.rs       — EmbeddedPostgres via pg-embed
    pool.rs        — sqlx PgPool creation
    execute.rs     — execute_query() with RLS context injection
migrations/        — 6 SQL migration files (roles, auth, storage, RLS, grants)
test-client/       — Vitest integration test suite
```

## Roadmap (Phase 2)

- [ ] Realtime WebSockets (logical replication → `axum::ws`)
- [ ] Edge Functions (`wasmtime` or V8 isolate)
- [ ] Local Studio UI dashboard

## License

MIT
