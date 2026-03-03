# Changelog

All notable changes to this project will be documented in this file.

Format follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).
Versioning follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [v0.1.0-alpha.1] — 2026-03-03

First tagged release. Core foundation is functional and tested.

### What works

**Auth (`/auth/v1`)**
- Signup, login, JWT sessions (HS256), Argon2id password hashing
- `GET /auth/v1/user` with Bearer token
- `GET /auth/v1/health` — DB + migration readiness check

**REST API (`/rest/v1`)**
- PostgREST-compatible CRUD: select, insert, update, delete
- Filter operators: `eq`, `neq`, `lt`, `lte`, `gt`, `gte`, `like`, `ilike`, `is`, `in`, `not.in`, `cs`, `cd`, `fts`, `and()`, `or()`
- `select=`, `order=`, `limit=`, `offset=` query params
- Row-Level Security enforced via `SET LOCAL ROLE` + JWT claims

**Storage (`/storage/v1`)**
- Bucket create/list/delete
- Object upload (multipart/form-data), download, delete (batch)
- RLS-gated access (anon vs authenticated vs service_role)
- Public URL generation

**Embedded PostgreSQL**
- Auto-managed via `pg-embed` — no external PostgreSQL required
- Per-instance port isolation: `pg_port = http_port + 10_000`
- Phase 1: 3-attempt retry loop for transient Windows file-lock issues
- Phase 2: auto-clear binary cache on corruption, retry once more
- Pre-flight port poll — waits for orphan processes to release before start
- Clean shutdown via `Drop::stop_db()` (not force-killed)

**CLI**
- `suparust start` / `start --daemon` / `stop` / `restart` / `status` / `logs`
- `--profile <name>` — load `.env.<name>`, total isolation from `.env`
- `--env-file <path>` — load arbitrary env file, total isolation
- Deterministic PID files per identity+port: `.suparust.<identity>.<port>.pid`
- Multi-instance safe: production + test + compat can coexist

**Supabase compatibility layer**
- Reads Supabase alias env vars (`JWT_SECRET`, `ANON_KEY`, `SERVICE_ROLE_KEY`, `PORT`, `DATA_DIR`, etc.) as fallback
- `SUPARUST_*` canonical vars always take precedence
- `[WARN]` logged when falling back to alias vars

**Observability**
- Structured JSON logs with `req_id` correlation per HTTP request
- `SUPARUST_LOG_LEVEL` + `SUPARUST_LOG_FORMAT` (pretty | json)
- `RUST_LOG` override for per-crate filtering

**Integration tests**
- 21 Vitest tests: Auth, REST API, Storage, RLS — 2 modes
- Mode A (`npm test`): SUPARUST_* canonical vars, port 53001
- Mode B (`npm run test:compat`): Supabase alias vars, port 53002
- Server auto-starts and auto-stops; teardown idempotent
- `node scripts/gen-env-test.mjs --regen` — full clean slate incl. pg-embed cache

### Known limitations

- `pg-embed` Windows reliability: fixed `/T` taskkill bug and port isolation, but not yet battle-tested across diverse Windows environments
- No CI/CD pipeline yet — tests run manually
- No Realtime WebSockets
- No Edge Functions
- No Studio UI dashboard
- `postgresql_embedded` migration not yet done (planned before `v0.1.0`)

### Roadmap to `v0.1.0`

- [ ] Migrate embedded PG from `pg-embed` → `postgresql_embedded`
- [ ] CI/CD pipeline (GitHub Actions)
- [ ] One additional major feature (Realtime or Edge Functions)

### Roadmap to `v1.0.0`

- [ ] Realtime WebSockets (PostgreSQL LISTEN/NOTIFY → axum::ws)
- [ ] Edge Functions (wasmtime)
- [ ] Local Studio UI dashboard
- [ ] Meaningful Supabase API parity

[v0.1.0-alpha.1]: https://github.com/brainplusplus/supa-rs/releases/tag/v0.1.0-alpha.1
