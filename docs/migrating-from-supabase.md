# Migrating from Supabase Self-Hosted

SupaRust is a drop-in Supabase replacement. If you already have a Supabase
self-hosted `.env`, it will work out of the box — SupaRust reads Supabase
env vars as compatibility aliases.

For new projects, use `SUPARUST_*` names directly.

## Compatibility Priority

```
1. SUPARUST_*       ← canonical (always wins)
2. Supabase alias   ← compatibility fallback
3. Default value
4. Auto-generate    ← jwt_secret, db_password only
```

When both are set, SupaRust logs:
```
[INFO] Both SUPARUST_DB_PASSWORD and POSTGRES_PASSWORD are set. Using SUPARUST_DB_PASSWORD.
```

When falling back to a Supabase alias:
```
[WARN] Using legacy env POSTGRES_PASSWORD. Prefer SUPARUST_DB_PASSWORD.
```

---

## Variable Mapping — Phase 1 (Active)

### Core / JWT

| Supabase | SupaRust | Default |
|---|---|---|
| `JWT_SECRET` | `SUPARUST_JWT_SECRET` | auto-generated |
| `ANON_KEY` | `SUPARUST_ANON_KEY` | derived from `JWT_SECRET` |
| `SERVICE_ROLE_KEY` | `SUPARUST_SERVICE_KEY` | derived from `JWT_SECRET` |
| `JWT_EXPIRY` | `SUPARUST_JWT_EXPIRY` | `3600` |

### Server

| Supabase | SupaRust | Default |
|---|---|---|
| *(none)* | `SUPARUST_PORT` | `3000` |
| `API_EXTERNAL_URL` | `SUPARUST_API_URL` | `http://localhost:3000` |
| `SITE_URL` | `SUPARUST_SITE_URL` | `http://localhost:3000` |

### Database

| Supabase | SupaRust | Default |
|---|---|---|
| `POSTGRES_PASSWORD` | `SUPARUST_DB_PASSWORD` | auto-generated |
| `POSTGRES_HOST` | `SUPARUST_DB_HOST` | `localhost` |
| `POSTGRES_PORT` | `SUPARUST_DB_PORT` | `5432` |
| `POSTGRES_USER` | `SUPARUST_DB_USER` | `postgres` |
| `POSTGRES_DB` | `SUPARUST_DB_NAME` | `postgres` |
| `DATABASE_URL` | `SUPARUST_DB_URL` | *(none — uses pg-embed)* |
| `PGRST_DB_SCHEMAS` | `SUPARUST_DB_SCHEMAS` | `public` |

### Auth

| Supabase | SupaRust | Default |
|---|---|---|
| `DISABLE_SIGNUP` | `SUPARUST_DISABLE_SIGNUP` | `false` |
| `ENABLE_EMAIL_SIGNUP` | `SUPARUST_ENABLE_EMAIL_SIGNUP` | `true` |
| `ENABLE_EMAIL_AUTOCONFIRM` | `SUPARUST_ENABLE_EMAIL_AUTOCONFIRM` | `false` |
| `ENABLE_ANONYMOUS_USERS` | `SUPARUST_ENABLE_ANONYMOUS_USERS` | `false` |

---

## Variable Mapping — Phase 2+ (Planned)

These are documented here for migration planning. They are **not yet active**
in SupaRust — see the [Roadmap](../README.md#roadmap).

### Connection Pooler

| Supabase | SupaRust | Default |
|---|---|---|
| `POOLER_PROXY_PORT_TRANSACTION` | `SUPARUST_POOLER_PORT` | `6543` |
| `POOLER_DEFAULT_POOL_SIZE` | `SUPARUST_POOLER_DEFAULT_POOL_SIZE` | `20` |
| `POOLER_MAX_CLIENT_CONN` | `SUPARUST_POOLER_MAX_CLIENTS` | `100` |
| `POOLER_TENANT_ID` | `SUPARUST_POOLER_TENANT_ID` | `default` |

### SMTP (Email)

| Supabase | SupaRust | Default |
|---|---|---|
| `SMTP_HOST` | `SUPARUST_SMTP_HOST` | *(optional)* |
| `SMTP_PORT` | `SUPARUST_SMTP_PORT` | `587` |
| `SMTP_USER` | `SUPARUST_SMTP_USER` | *(optional)* |
| `SMTP_PASS` | `SUPARUST_SMTP_PASS` | *(optional)* |
| `SMTP_ADMIN_EMAIL` | `SUPARUST_SMTP_ADMIN_EMAIL` | *(optional)* |
| `SMTP_SENDER_NAME` | `SUPARUST_SMTP_SENDER_NAME` | *(optional)* |
| `ADDITIONAL_REDIRECT_URLS` | `SUPARUST_REDIRECT_URLS` | *(optional)* |

### Storage (S3 backend)

| Supabase | SupaRust | Default |
|---|---|---|
| `GLOBAL_S3_BUCKET` | `SUPARUST_S3_BUCKET` | *(optional)* |
| `REGION` | `SUPARUST_S3_REGION` | *(optional)* |
| `STORAGE_TENANT_ID` | `SUPARUST_STORAGE_TENANT_ID` | `default` |
| `S3_PROTOCOL_ACCESS_KEY_ID` | `SUPARUST_S3_ACCESS_KEY` | *(optional)* |
| `S3_PROTOCOL_ACCESS_KEY_SECRET` | `SUPARUST_S3_SECRET_KEY` | *(optional)* |

---

## Variables Not Supported

These Supabase variables have no equivalent in SupaRust (services not present):

- `KONG_*` — no API gateway
- `LOGFLARE_*` / `GOOGLE_PROJECT_*` — no analytics service
- `IMGPROXY_*` — no image proxy
- `STUDIO_*` / `DASHBOARD_*` — no Studio UI (planned)
- `MINIO_*` — manage your own MinIO; connect via `SUPARUST_DB_URL`
- `DOCKER_SOCKET_LOCATION` — no container management
- `FUNCTIONS_VERIFY_JWT` — no Edge Functions yet
- `SECRET_KEY_BASE` / `VAULT_ENC_KEY` — Elixir/Phoenix-specific
- `MAILER_URLPATHS_*` — hardcoded in SupaRust

---

## Running with Profiles (Environment Isolation)

SupaRust supports profile-based environment isolation via the `--profile` flag. This replaces the need to inject env vars manually or manage multiple `.env` files by hand.

### Profile modes

| Command | Env loaded | PID file |
|---|---|---|
| `suparust start` | `.env` (silent ok if missing) | `.suparust.local.<port>.pid` |
| `suparust start --profile test` | `.env.test` (hard error if missing) | `.suparust.profile.test.<port>.pid` |
| `suparust start --env-file /path/custom.env` | `custom.env` only | `.suparust.env.<filename>.<port>.pid` |
| `--profile x --env-file y` | — | error: cannot use both |

**Isolation is total** — when `--profile` or `--env-file` is specified, `.env` is never loaded. Zero overlay, zero leakage.

### Why default identity is `local`

When no profile is given, SupaRust uses `local` as the identity. This is intentional:

- **Convention**: `.env` = local dev in Supabase, Next.js, Vite, and docker-compose — `local` matches this universal mental model
- **Determinism**: PID file `.suparust.local.<port>.pid` is predictable even in default mode — no anonymous `.suparust.pid` that collides between instances
- **Consistency**: every running instance has an explicit identity (`local`, `profile.test`, `env.custom`) — `stop` and `status` always target the right instance

### All commands respect `--profile`

The flag works globally — before or after any subcommand:

```bash
suparust --profile test start
suparust start --profile test     # same thing

suparust stop   --profile test
suparust status --profile test
suparust restart --profile test
```

### Multi-instance example

```bash
suparust start --profile dev    # .suparust.profile.dev.3000.pid
suparust start --profile test   # .suparust.profile.test.53001.pid

suparust status --profile test
suparust stop   --profile dev
```

---

## Quick Migration Example

**Your existing Supabase `.env`:**

```env
JWT_SECRET=my-super-secret-32-char-minimum-key
ANON_KEY=eyJ...
SERVICE_ROLE_KEY=eyJ...
POSTGRES_PASSWORD=your-super-secret-and-long-postgres-password
POSTGRES_HOST=localhost
POSTGRES_DB=postgres
SITE_URL=http://localhost:3000
API_EXTERNAL_URL=http://localhost:3000
```

**Option A — Zero changes:**
SupaRust reads the Supabase vars directly as compatibility aliases. Just run `suparust start`.

**Option B — Migrate to canonical names:**

```env
SUPARUST_JWT_SECRET=my-super-secret-32-char-minimum-key
SUPARUST_ANON_KEY=eyJ...
SUPARUST_SERVICE_KEY=eyJ...
SUPARUST_DB_PASSWORD=your-super-secret-and-long-postgres-password
SUPARUST_DB_HOST=localhost
SUPARUST_DB_NAME=postgres
SUPARUST_SITE_URL=http://localhost:3000
SUPARUST_API_URL=http://localhost:3000
```
