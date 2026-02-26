# ⚡ supa-rs

> **The Supabase drop-in replacement, compiled into a single, blazingly fast Rust binary.**

**supa-rs** is a lightweight, zero-configuration backend that provides a 100% compatible API layer for the official `@supabase/supabase-js` client. It runs embedded PostgreSQL internally, removes the overhead of multi-container Docker deployments, and executes queries via a highly optimized Zero-Deserialization Engine.

## 🚀 Features

- **Single Binary Architecture**: No Docker required. Run it anywhere natively.
- **Embedded PostgreSQL**: Automatically spawns and manages a local PostgreSQL instance (`pg_tmp` style) in your `.data/postgres` folder.
- **100% supabase-js Compatible**: Plug it into your existing frontend using `createClient('http://127.0.0.1:3000', ANON_KEY)`.
- **Zero-Deserialization Engine**: Bypasses traditional ORM overhead by directly piping PostgreSQL `json_agg` raw bytes to the HTTP response payload via `axum`.
- **GoTrue Auth Module**: Complete JWT, password hashing (Argon2), and identity session management.
- **PostgREST-compatible API**: AST-based parser utilizing `nom` to map `supabase.from().select().eq()` natively to Postgres parameters.
- **Object Storage**: High-performance local multipart file handling with anti-path traversal protections and pre-flight RLS validations.
- **Row-Level Security (RLS)**: Strict RLS context injection via `SET LOCAL request.jwt.claims` for fully secured multi-tenant access.

## 📦 Getting Started

### 1. Prerequisites
- Rust (cargo)
- PostgreSQL binaries installed (must be available in your system `$PATH`)
- Node.JS `v18+` (for Integration Test Client)

### 2. Build and Run
Clone the repository and run the server. It will automatically initialize the embedded database, install schema migrations, generate a persistent JWT secret, and start listening on port 3000.

```bash
cargo run --release
```

### 3. Connect your Client
On startup, `supa-rs` will persist a JWT Secret in `./data/suparust-config.json`. Generate an `ANON_KEY` using that secret, and point your frontend to the local port:

```javascript
import { createClient } from '@supabase/supabase-js'

const supabaseUrl = 'http://127.0.0.1:3000'
const supabaseAnonKey = '<YOUR_GENERATED_ANON_KEY>'

const supabase = createClient(supabaseUrl, supabaseAnonKey)
```

## 🧪 Running Integration Tests
**supa-rs** guarantees its compatibility against the official SDK via a rigorous Vitest test suite.
Ensure the server is running, then execute:

```bash
cd test-client
npm install
npm run test
```

## 🏗️ Architecture Stack
- **Web Framework**: [`axum`](https://github.com/tokio-rs/axum)
- **Database Driver**: [`sqlx`](https://github.com/launchbadge/sqlx)
- **Parser Engine**: [`nom`](https://github.com/rust-bakery/nom) (for PostgREST transpilation)
- **Authentication**: `jsonwebtoken`, `argon2`

## 🤝 Roadmap (Phase 2)
- [ ] Realtime WebSockets (`axum::ws` + logical replication)
- [ ] Edge Functions support via `v8` isolate or WebAssembly
- [ ] Local UI Dashboard

## License
MIT License
