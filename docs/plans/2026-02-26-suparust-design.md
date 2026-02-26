# SupaRust — Phase 1 Design Document

## Overview

SupaRust adalah single-binary Rust application yang merupakan drop-in replacement untuk Supabase stack, 100% kompatibel dengan `supabase-js` client. Default menggunakan embedded PostgreSQL via `postgresql_embedded` crate, dengan opsi koneksi ke PostgreSQL eksternal via `DATABASE_URL`.

## Section 1: Core Architecture & REST API

### Data Flow Pipeline (Per-Request)
Setiap HTTP request melalui 6-step pipeline yang menjamin RLS boundaries tidak pernah dilanggar:
1. **Axum Handler** — ekstrak JWT dari `Authorization: Bearer` header, parse URL params via hybrid parser.
2. **SQL AST Generator** — transform parsed filters menjadi intermediate AST (`SelectNode`, `FilterNode`, `JoinNode`, `OrderNode`).
3. **SQL Builder** — translate AST ke SQL query string berbasis `json_agg` **dengan parameterized binding wajib ($1, $2, dll) untuk semua user-supplied values** (anti SQL injection, terutama di JSON paths & IN clauses).
4. **Database Executor** — buka tepat satu `sqlx` transaction per request.
5. **RLS Context Injection** — dalam transaksi yang sama, jalankan `SET LOCAL` sebelum query utama:
   ```sql
   SET LOCAL role = $1;
   SET LOCAL request.jwt.claims = $2;
   SET LOCAL request.method = $3;
   SET LOCAL request.path = $4;
   ```
6. **Response** — raw JSON bytes dari Postgres dikembalikan langsung ke Axum tanpa Rust deserialization.

### Parser Architecture
- **Hand-written dispatcher** untuk query string keys literal: `select`, `order`, `limit`, `offset`, `or`, `and`.
- **`nom` combinators** untuk recursive values: `eq.25`, `in.(a,b)`, nested parentheses, JSON path chaining (`->`, `->>`).

### Module Structure
```text
src/
├── api/        # Axum routes (/rest/v1, /auth/v1, /storage/v1)
├── parser/     # nom: select.rs, filter.rs, order.rs
├── sql/        # ast.rs, builder.rs, rls.rs
└── db/         # pool.rs, execute.rs
```

## Section 2: Auth (GoTrue-Compatible)

### Required Postgres Schema
Tables wajib: `auth.users`, `auth.sessions`, `auth.refresh_tokens`, `auth.identities`

### Required Postgres Functions
```sql
auth.uid()   -- returns uuid dari request.jwt.claims->>'sub'
auth.role()  -- returns text dari request.jwt.claims->>'role'
auth.jwt()   -- returns jsonb seluruh claims
```

### Required Postgres Roles
```sql
anon, authenticated, service_role (BYPASSRLS)
```

### Startup Key Generation
Binary auto-generate saat inisialisasi pertama dan simpan ke config:
```text
JWT_SECRET   = random_256bit_hex()
anon_key     = sign({ role: "anon",         exp: far_future }, JWT_SECRET)
service_key  = sign({ role: "service_role", exp: far_future }, JWT_SECRET)
```

### Auth Endpoints Data Flow
- **POST `/auth/v1/token?grant_type=password`**
  → `argon2::verify` credentials → create `auth.sessions` → insert `auth.refresh_tokens` → sign JWT → return `{ access_token, refresh_token, user }`
- **POST `/auth/v1/token?grant_type=refresh_token`**
  → lookup token → if `revoked = true`: revoke entire session family, return 401 → else: rotate token (set old revoked, insert new with parent), issue new JWT.

### Public Users View
```sql
CREATE VIEW public.users AS
  SELECT id, email, raw_user_meta_data, created_at FROM auth.users;
GRANT SELECT ON public.users TO authenticated;
```

## Section 3: Storage (Supabase Storage-Compatible)

### Architecture Principle
**RLS-first:** Postgres metadata query selalu dijalankan sebelum menyentuh filesystem/S3. Jika RLS deny, return 403 tanpa membaca/menulis bytes.

### Required Postgres Schema
Tables: `storage.buckets`, `storage.objects` (dengan `path_tokens` generated column), `storage.tus_uploads`

### Required Storage Helper Functions
```sql
storage.foldername(name text) → text[]
storage.filename(name text)   → text
storage.extension(name text)  → text
```

### Storage Backend Abstraction
Via `object_store` crate — default Local FS, switchable ke S3-compatible via config.

### TUS Resumable Upload State
State disimpan di `storage.tus_uploads` (durable across restarts). Background `tokio::task` cleanup expired uploads periodik.

### Public Bucket
`GET /storage/v1/object/public/{bucket}/{path}` — skip RLS pipeline, langsung serve jika `buckets.public = true`.

## Critical Note: Migration Ordering
Karena interdependency schema, urutan eksekusi migration di embedded Postgres sangat krusial dan harus berurutan secara ketat:
- **Migration 001** → Postgres roles (`anon`, `authenticated`, `service_role`)
- **Migration 002** → auth schema + tables + functions (`auth.uid`, `auth.jwt`, `auth.role`)
- **Migration 003** → storage schema + tables + helper functions
- **Migration 004** → `public.users` view
- **Migration 005** → Default RLS policies untuk `storage.objects` dan `storage.buckets`

## Phase 2 Roadmap (Future Brainstorming)
- 🔴 **Realtime:** WAL logical replication + tokio-tungstenite + Phoenix protocol
- 🔴 **Edge Functions:** deno_core embedded runtime
- 🟡 **Supavisor:** PG connection pooler (wire protocol)
- 🟡 **NAPI-RS:** Node.js native addon exposure vs pure standalone binary
- 🟢 **CLI & config:** Project initialization, DATABASE_URL, secrets management
