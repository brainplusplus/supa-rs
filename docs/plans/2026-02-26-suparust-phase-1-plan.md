# SupaRust Phase 1 Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Implement the core foundation of a Supabase-compatible native Rust binary featuring Embedded Postgres, GoTrue-compatible Auth schemas, and an RLS-enforcing JSON Aggregate REST engine.

**Architecture:** Axum router pointing to `sqlx` driving an embedded PostgreSQL database. Heavy reliance on `SET LOCAL` wrapping dynamic AST-to-SQL `json_agg` generation to natively support Postgres Row Level Security (RLS). Storage validates RLS metadata before touching files.

**Tech Stack:** Rust, Axum, SQLx, `postgresql_embedded`, `nom` (parsing), `argon2`, `jsonwebtoken`, `object_store`.

---

### Task 1: Project Scaffolding & Dependencies

**Files:**
- Modify: `Cargo.toml`
- Create: `src/main.rs`
- Create: `src/db/mod.rs`
- Create: `src/db/pool.rs`

**Step 1.1: Add core dependencies**
```toml
[dependencies]
axum = "0.7"
tokio = { version = "1.0", features = ["full"] }
sqlx = { version = "0.8", features = ["postgres", "runtime-tokio-rustls", "json", "uuid", "time", "chrono"] }
postgresql_embedded = "0.16"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
jsonwebtoken = "9"
argon2 = "0.5"
nom = "7.1"
object_store = "0.10"
uuid = { version = "1.8", features = ["v4", "serde"] }
```

**Step 1.2: scaffold embedded postgres pool initialization**
*Create `src/db/pool.rs` with embedded postgres startup.*
```rust
use sqlx::{PgPool, postgres::PgPoolOptions};
use postgresql_embedded::PostgreSQL;

pub async fn init_db() -> Result<PgPool, Box<dyn std::error::Error>> {
    let mut postgres = PostgreSQL::default();
    postgres.setup().await?;
    postgres.start().await?;
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(postgres.settings().url.as_str())
        .await?;
    Ok(pool)
}
```

**Step 1.3: Run `cargo check` to verify dependencies resolve**
Run: `cargo check`
Expected: PASS

**Step 1.4: Commit**
```bash
git init
echo "target/" > .gitignore
git add Cargo.toml src/ .gitignore
git commit -m "chore: scaffold project and add dependencies"
```

---

### Task 2: Strict Migration Ordering (Schema Initialization)

**Files:**
- Create: `migrations/001_roles.sql`
- Create: `migrations/002_auth_schema.sql`
- Create: `migrations/003_storage_schema.sql`
- Create: `migrations/004_public_views.sql`
- Create: `migrations/005_default_rls.sql`
- Modify: `src/main.rs`

**Step 2.1: Write 001_roles.sql**
```sql
CREATE ROLE anon NOLOGIN;
CREATE ROLE authenticated NOLOGIN;
CREATE ROLE service_role NOLOGIN BYPASSRLS;
GRANT USAGE ON SCHEMA public TO anon, authenticated;
GRANT ALL ON ALL TABLES IN SCHEMA public TO authenticated;
GRANT SELECT ON ALL TABLES IN SCHEMA public TO anon;
```

**Step 2.2: Write 002_auth_schema.sql**
*(Include complete auth.users, auth.sessions, auth.refresh_tokens, auth.identities schemas exactly as spec'd, plus `auth.uid()`, `auth.role()`, `auth.jwt()` functions).*

**Step 2.3: Write 003_storage_schema.sql**
*(Include complete storage.buckets, storage.objects with `path_tokens` generated column, `tus_uploads`, and functions `storage.foldername` etc).*

**Step 2.4: Write 004_public_views.sql & 005_default_rls.sql**
*(Include `CREATE VIEW public.users AS SELECT...` and `ALTER TABLE storage.objects ENABLE ROW LEVEL SECURITY;`)*

**Step 2.5: Implement sqlx migration runner in `main.rs`**
```rust
sqlx::migrate!("./migrations").run(&pool).await?;
```

**Step 2.6: Run to verify migrations pass**
Run: `cargo run`
Expected: PASS (migrations applied successfully to embedded postgres)

**Step 2.7: Commit**
```bash
git add migrations/ src/main.rs
git commit -m "feat: initialize strict postgres migration schemas"
```

---

### Task 3: REST API Parser Setup (Nom Hybrid)

**Files:**
- Create: `src/parser/mod.rs`
- Create: `src/parser/filter.rs`
- Create: `tests/parser_tests.rs`

**Step 3.1: Write failing test for basic filter parse**
*In `tests/parser_tests.rs`:*
```rust
use suparust::parser::filter::parse_operator;
#[test]
fn test_parse_eq_operator() {
    let result = parse_operator("eq.25");
    assert!(result.is_ok());
    // matching logic
}
```

**Step 3.2: Run test (fails)**
Run: `cargo test`
Expected: FAIL

**Step 3.3: Implement `nom` combinator for operators**
*In `src/parser/filter.rs`:*
```rust
use nom::{IResult, bytes::complete::tag, branch::alt, combinator::value};
#[derive(Debug, PartialEq)]
pub enum Operator { Eq, Neq, Like, In }

pub fn parse_operator(input: &str) -> IResult<&str, Operator> {
    alt((
        value(Operator::Eq, tag("eq")),
        value(Operator::Neq, tag("neq")),
        value(Operator::Like, tag("like")),
        value(Operator::In, tag("in")),
    ))(input)
}
```

**Step 3.4: Run test (passes)**
Run: `cargo test`
Expected: PASS

**Step 3.5: Commit**
```bash
git add src/parser/ tests/
git commit -m "feat: hybrid nom parser for REST URL parameters"
```

---

### Task 4: SQL AST Generator & RLS Transaction Wrapper

**Files:**
- Create: `src/sql/mod.rs`
- Create: `src/sql/builder.rs`
- Create: `src/sql/rls.rs`
- Modify: `src/db/execute.rs`

**Step 4.1: Create SQL AST structs (`src/sql/ast.rs`)**
Define `SelectNode`, `FilterNode` ensuring values are captured for `$1` parameterized bindings.

**Step 4.2: Implement transaction execution wrapper (`src/db/execute.rs`)**
```rust
use sqlx::{Transaction, Postgres};
use serde_json::Value;

pub async fn execute_with_rls(
    mut tx: Transaction<'_, Postgres>, 
    role: &str, 
    jwt_claims: &str, 
    query: &str, 
    params: Vec<Value> // use specific type binding later
) -> Result<String, sqlx::Error> {
    sqlx::query("SET LOCAL role = $1").bind(role).execute(&mut *tx).await?;
    sqlx::query("SET LOCAL request.jwt.claims = $2").bind(jwt_claims).execute(&mut *tx).await?;
    
    // Execute dynamic query with json_agg and bindings
    // Return raw String from Postgres
    tx.commit().await?;
    Ok("[]".to_string()) // placeholder
}
```

**Step 4.3: Write `json_agg` query test**
*(Test that `execute_with_rls` correctly applies RLS claims and returns valid JSON string.)*

**Step 4.4: Commit**
```bash
git add src/sql/ src/db/
git commit -m "feat: rls-enforcing sqlx transaction wrapper"
```

---

### Task 5: Auth Endpoints (JWT & Password)

**Files:**
- Create: `src/api/mod.rs`
- Create: `src/api/auth.rs`

**Step 5.1: Create JWT generation util**
Use `jsonwebtoken` to sign standard Supabase structure JWTs using a generated 256-bit hex config secret.

**Step 5.2: Create POST `/auth/v1/token?grant_type=password`**
Parse `email`/`password`, hash verify with `argon2` against `auth.users`, output `auth.sessions` row, and issue JWT.

**Step 5.3: Commit**
```bash
git add src/api/
git commit -m "feat: implement GoTrue compatible auth endpoints"
```

---

### Task 6: Storage Metadata & RLS Validation Endpoint

**Files:**
- Create: `src/api/storage.rs`

**Step 6.1: Implement Storage Handler RLS barrier**
Axum route for `GET /storage/v1/object/public/{bucket}/{path}` that skips RLS.
Axum route for `GET /storage/v1/object/authenticated/*` that forces the RLS transaction on `storage.objects` *before* utilizing `object_store::local::LocalFileSystem` to fetch bytes.

**Step 6.2: Commit**
```bash
git add src/api/storage.rs
git commit -m "feat: enforce RLS metadata validation before storage access"
```

