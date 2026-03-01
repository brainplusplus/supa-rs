# SeaQuery Refactor Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace string-concatenated SQL in `SqlBuilder` with SeaQuery AST-based query construction, eliminating SQL injection surface while keeping all 21 Vitest tests green.

**Architecture:** The existing parser layer (`src/parser/`) stays untouched — it already produces clean `Filter`, `SelectNode`, and `OrderNode` structs. Only `src/sql/builder.rs` and `src/db/execute.rs` change: `builder.rs` switches from string concatenation to SeaQuery `SelectStatement`/`UpdateStatement`/etc., and `execute.rs` switches from `sqlx::query()` with manual `.bind()` loops to `sqlx::query_with()` using SeaQuery-produced `Values`. Static auth/storage queries in `src/api/auth.rs` and `src/api/storage.rs` are **not touched**.

**Tech Stack:** `sea-query 0.32`, `sea-query-binder 0.7` (SeaQuery → SQLx bridge), existing `sqlx 0.8`, `nom 7` (parser, unchanged)

---

## Background: What Exists Today

```
src/sql/
  ast.rs       — QueryAst, Operation, CountMethod structs (read-only, no change)
  rls.rs       — RlsContext, SET LOCAL statements (no change)
  builder.rs   — SqlBuilder: builds SQL strings + Vec<serde_json::Value> params ← REPLACE
  mod.rs       — re-exports

src/db/
  execute.rs   — execute_query(): binds Vec<Value> to sqlx::query ← REPLACE binding layer
```

**Current problem:** `builder.rs` builds SQL via `format!()` string concatenation. Column names are quoted (`"col"`) but operator selection and array casts go through raw string interpolation. The `execute.rs` binding loop converts `Value::String(s) => s` and falls back to `.to_string()` for everything else — meaning booleans, numbers, and JSON all become strings without proper PostgreSQL type casting.

**Target:** SeaQuery builds a typed AST → emits `(sql_string, Values)` → `sqlx::query_with(sql, values)` binds correctly typed PG parameters.

---

## SeaQuery Crash Course (Read Before Starting)

SeaQuery uses **identifier enums** to name tables/columns. Since SupaRust works with *arbitrary user tables*, we use `Alias::new("col_name")` everywhere instead of deriving `Iden`.

```rust
use sea_query::{Alias, Expr, Query, PostgresQueryBuilder, Condition, Order};
use sea_query_binder::SqlxBinder;

// Build query
let (sql, values) = Query::select()
    .from(Alias::new("users"))          // table
    .column(Alias::new("id"))           // column
    .and_where(Expr::col(Alias::new("age")).gt(18i64))
    .build_sqlx(PostgresQueryBuilder);  // → (String, SqlxValues)

// Execute
let rows = sqlx::query_with(&sql, values).fetch_all(&pool).await?;
```

`build_sqlx` returns `SqlxValues` — pass it directly to `sqlx::query_with`. No manual `.bind()` loop needed.

For wrapping queries (json_agg pattern), we still use a raw `format!()` outer wrapper — SeaQuery does not natively support `json_agg(row_to_json(...))`. That's fine: the inner SELECT from SeaQuery is already safe; the outer wrap has no user input.

---

## Task 1: Add SeaQuery Dependencies

**Files:**
- Modify: `Cargo.toml`

**Step 1: Add dependencies**

```toml
# In [dependencies] section, after the existing sqlx line:
sea-query = { version = "0.32", features = ["backend-postgres"] }
sea-query-binder = { version = "0.7", features = ["sqlx-postgres"] }
```

**Step 2: Verify it compiles**

```bash
cd D:/Rust/SupaRust
cargo build 2>&1 | head -30
```

Expected: compiles successfully, no errors. SeaQuery will appear in the dependency tree.

**Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "chore(deps): add sea-query and sea-query-binder"
```

---

## Task 2: Replace `execute_query` Signature to Accept `SqlxValues`

**Files:**
- Modify: `src/db/execute.rs`

**Context:** Today `execute_query` takes `Vec<serde_json::Value>` and manually binds each one. We replace it to accept SeaQuery's `sea_query_binder::SqlxValues` (which is `sqlx::postgres::PgArguments` under the hood). This is a breaking change to the signature — Task 3 updates all callers.

**Step 1: Rewrite `execute.rs`**

Replace the entire file content:

```rust
use sqlx::{PgPool, Row, types::Json};
use serde_json::Value;
use sea_query_binder::SqlxValues;
use crate::sql::rls::RlsContext;

pub async fn execute_query(
    pool: &PgPool,
    sql: &str,
    values: SqlxValues,
    rls: &RlsContext,
) -> Result<Value, sqlx::Error> {
    let mut tx = pool.begin().await?;

    for (name, val) in rls.to_set_local_statements() {
        if name.to_lowercase() == "role" {
            let stmt = format!("SET LOCAL ROLE {}", val);
            sqlx::query(&stmt).execute(&mut *tx).await?;
        } else {
            let stmt = format!("SET LOCAL \"{}\" TO '{}'", name, val.replace("'", "''"));
            sqlx::query(&stmt).execute(&mut *tx).await?;
        }
    }

    let result = sqlx::query_with(sql, values)
        .fetch_optional(&mut *tx)
        .await?;

    let output = match result {
        Some(row) => {
            let json_result: Json<Value> = row.try_get(0)?;
            json_result.0
        }
        None => serde_json::json!([]),
    };

    tx.commit().await?;

    Ok(output)
}
```

**Step 2: Verify module compiles in isolation**

```bash
cargo check --message-format short 2>&1 | grep "execute.rs"
```

Expected: errors about callers (`rest.rs` still passes `Vec<Value>`) — that's expected. No errors *inside* execute.rs itself.

---

## Task 3: Rewrite `SqlBuilder` — WHERE Clause via SeaQuery

**Files:**
- Modify: `src/sql/builder.rs`

**Context:** This is the core refactor. We rewrite `SqlBuilder` to produce `(String, SqlxValues)` using SeaQuery's `SelectStatement`, `UpdateStatement`, `InsertStatement`, `DeleteStatement`. The column-identifier quoting, operator mapping, and parameterization all move to SeaQuery's type system.

**Step 1: Understand the mapping**

| Current code | SeaQuery equivalent |
|---|---|
| `format!("\"{}\" = ${}", col, n)` | `Expr::col(Alias::new(col)).eq(val)` |
| `format!("\"{}\" != ${}", col, n)` | `Expr::col(Alias::new(col)).ne(val)` |
| `format!("\"{}\" LIKE ${}", col, n)` | `Expr::col(Alias::new(col)).like(val)` |
| `format!("\"{}\" IS NULL")` | `Expr::col(Alias::new(col)).is_null()` |
| `format!("\"{}\" IS NOT NULL")` | `Expr::col(Alias::new(col)).is_not_null()` |
| `= ANY(CAST($n AS text[]))` | `Expr::col(..).is_in(vec![...])` |
| `!= ALL(CAST($n AS text[]))` | `Expr::col(..).is_not_in(vec![...])` |
| `(A AND B)` | `Condition::all().add(A).add(B)` |
| `(A OR B)` | `Condition::any().add(A).add(B)` |

**Step 2: Replace entire `builder.rs`**

```rust
use serde_json::Value;
use sea_query::{
    Alias, Condition, DeleteStatement, Expr, Iden, InsertStatement,
    OnConflict, Order, PostgresQueryBuilder, Query, QueryBuilder,
    SelectStatement, SimpleExpr, UpdateStatement,
};
use sea_query_binder::{SqlxBinder, SqlxValues};

use crate::parser::filter::{Filter, FilterValue, Operator, ColumnFilter};
use crate::parser::select::SelectNode;
use crate::parser::order::{OrderNode, Direction, NullsOrder};
use crate::sql::ast::QueryAst;

// ── helpers ──────────────────────────────────────────────────────────────────

fn table_ref(schema: &str, table: &str) -> (Alias, Alias) {
    (Alias::new(schema), Alias::new(table))
}

fn col(name: &str) -> Alias {
    Alias::new(name)
}

/// Convert our Filter AST into a SeaQuery Condition.
fn build_condition(filter: &Filter) -> Result<SimpleExpr, String> {
    match filter {
        Filter::Column(c) => column_filter_to_expr(c),
        Filter::And(filters) => {
            let mut cond = Condition::all();
            for f in filters {
                cond = cond.add(build_condition(f)?);
            }
            Ok(cond.into())
        }
        Filter::Or(filters) => {
            let mut cond = Condition::any();
            for f in filters {
                cond = cond.add(build_condition(f)?);
            }
            Ok(cond.into())
        }
    }
}

fn column_filter_to_expr(c: &ColumnFilter) -> Result<SimpleExpr, String> {
    let expr = Expr::col(col(&c.column));

    match &c.value {
        FilterValue::Null => {
            let e = if matches!(c.operator, Operator::IsNot) || c.negated {
                expr.is_not_null()
            } else {
                expr.is_null()
            };
            Ok(e)
        }
        FilterValue::Single(val) => {
            // All filter values arrive as strings from request params.
            // We bind as text; PostgreSQL coerces via its implicit cast rules.
            let v = val.clone();
            let e: SimpleExpr = match (&c.operator, c.negated) {
                (Operator::Eq, false)  => expr.eq(v),
                (Operator::Eq, true)   => expr.ne(v),
                (Operator::Neq, false) => expr.ne(v),
                (Operator::Neq, true)  => expr.eq(v),
                (Operator::Lt, false)  => expr.lt(v),
                (Operator::Lt, true)   => expr.gte(v),
                (Operator::Lte, false) => expr.lte(v),
                (Operator::Lte, true)  => expr.gt(v),
                (Operator::Gt, false)  => expr.gt(v),
                (Operator::Gt, true)   => expr.lte(v),
                (Operator::Gte, false) => expr.gte(v),
                (Operator::Gte, true)  => expr.lt(v),
                (Operator::Like, false)  => expr.like(v),
                (Operator::Like, true)   => expr.not_like(v),
                (Operator::Ilike, false) => expr.ilike(v),
                (Operator::Ilike, true)  => expr.not_ilike(v),
                (Operator::Is, _) => expr.eq(v),   // fallback for non-null IS
                _ => return Err(format!("Unsupported operator {:?} with single value", c.operator)),
            };
            Ok(e)
        }
        FilterValue::List(vals) => {
            let items: Vec<sea_query::Value> = vals
                .iter()
                .map(|s| sea_query::Value::String(Some(Box::new(s.clone()))))
                .collect();
            let e = if c.negated || matches!(c.operator, Operator::NotIn) {
                expr.is_not_in(items)
            } else {
                expr.is_in(items)
            };
            Ok(e)
        }
    }
}

fn apply_filters(
    stmt: &mut SelectStatement,
    filters: &[Filter],
) -> Result<(), String> {
    for f in filters {
        let expr = build_condition(f)?;
        stmt.and_where(expr);
    }
    Ok(())
}

fn build_projection(stmt: &mut SelectStatement, nodes: &[SelectNode]) {
    if nodes.is_empty() {
        stmt.column(sea_query::Asterisk);
        return;
    }
    for node in nodes {
        // json_path and cast are appended as raw SQL fragments since SeaQuery
        // doesn't model them natively. Column name is always identifier-quoted.
        let mut col_expr = format!("\"{}\"", node.name);
        if let Some(jp) = &node.json_path {
            col_expr.push_str(jp);
        }
        if let Some(cast) = &node.cast {
            col_expr = format!("{}::{}", col_expr, cast);
        }
        if let Some(alias) = &node.alias {
            col_expr = format!("{} AS \"{}\"", col_expr, alias);
        }
        stmt.expr(sea_query::Expr::cust(&col_expr));
    }
}

fn apply_order(stmt: &mut SelectStatement, nodes: &[OrderNode]) {
    for node in nodes {
        let dir = match node.direction {
            Direction::Asc  => Order::Asc,
            Direction::Desc => Order::Desc,
        };
        let mut ord = sea_query::OrderExpr {
            expr: sea_query::Expr::col(col(&node.column)).into(),
            order: dir,
            nulls: None,
        };
        ord.nulls = match node.nulls {
            Some(NullsOrder::First) => Some(sea_query::NullOrdering::First),
            Some(NullsOrder::Last)  => Some(sea_query::NullOrdering::Last),
            None => None,
        };
        stmt.order_by_with_nulls(
            col(&node.column),
            dir,
            ord.nulls,
        );
    }
}

// ── public API ────────────────────────────────────────────────────────────────

/// Wraps an inner SQL string with json_agg + LIMIT/OFFSET.
/// The inner SQL comes from SeaQuery and is already injection-safe.
fn wrap_json_agg(inner: &str, limit: i64, offset: i64) -> String {
    format!(
        "SELECT COALESCE(json_agg(row_to_json(_t)), '[]'::json)\nFROM (\n  {} LIMIT {} OFFSET {}\n) _t",
        inner, limit, offset
    )
}

pub fn build_select(ast: &QueryAst) -> Result<(String, SqlxValues), String> {
    let mut stmt = Query::select();
    stmt.from((Alias::new(&ast.schema), Alias::new(&ast.table)));

    build_projection(&mut stmt, &ast.select);
    apply_filters(&mut stmt, &ast.filters)?;
    apply_order(&mut stmt, &ast.order);

    let (inner_sql, _) = stmt.build(PostgresQueryBuilder);

    let limit  = ast.limit.unwrap_or(1000);
    let offset = ast.offset.unwrap_or(0);
    let final_sql = wrap_json_agg(&inner_sql, limit, offset);

    // Re-build with binder to get SqlxValues for the WHERE params
    let (_, values) = stmt.build_sqlx(PostgresQueryBuilder);

    // Rebuild outer query string — params go to inner WHERE; LIMIT/OFFSET are literals
    let (inner_sql2, values2) = stmt.build_sqlx(PostgresQueryBuilder);
    let final_sql2 = wrap_json_agg(&inner_sql2, limit, offset);

    Ok((final_sql2, values2))
}

pub fn build_insert(
    schema: &str,
    table: &str,
    body: &Value,
    return_minimal: bool,
    resolution: Option<&String>,
) -> Result<(String, SqlxValues), String> {
    let rows = if let Value::Array(arr) = body {
        arr.clone()
    } else {
        vec![body.clone()]
    };

    if rows.is_empty() {
        return Err("Empty payload for insert".into());
    }

    let first_obj = rows[0].as_object().ok_or("Payload must be JSON objects")?;
    if first_obj.is_empty() {
        return Err("Empty JSON object in insert payload".into());
    }

    let mut columns: Vec<String> = first_obj.keys().cloned().collect();
    columns.sort();

    let mut stmt = Query::insert();
    stmt.into_table((Alias::new(schema), Alias::new(table)));
    for col_name in &columns {
        stmt.column(Alias::new(col_name));
    }

    for row in &rows {
        let obj = row.as_object().ok_or("Payload must be an array of objects")?;
        let mut row_vals: Vec<sea_query::Value> = Vec::new();
        for col_name in &columns {
            let val = obj.get(col_name).unwrap_or(&Value::Null);
            row_vals.push(json_to_sea_value(val));
        }
        stmt.values(row_vals).map_err(|e| e.to_string())?;
    }

    if let Some(res) = resolution {
        match res.as_str() {
            "ignore-duplicates" => {
                stmt.on_conflict(OnConflict::new().do_nothing().to_owned());
            }
            "merge-duplicates" => {
                return Err("merge-duplicates requires a named unique constraint".into());
            }
            _ => {}
        }
    }

    if !return_minimal {
        stmt.returning_all();
        let (inner_sql, values) = stmt.build_sqlx(PostgresQueryBuilder);
        let sql = format!(
            "SELECT COALESCE(json_agg(row_to_json(_t)), '[]'::json) FROM ({}) _t",
            inner_sql
        );
        Ok((sql, values))
    } else {
        let (inner_sql, values) = stmt.build_sqlx(PostgresQueryBuilder);
        let sql = format!("WITH _insert AS ({}) SELECT '[]'::json", inner_sql);
        Ok((sql, values))
    }
}

pub fn build_update(
    schema: &str,
    table: &str,
    body: &Value,
    filters: &[Filter],
    return_minimal: bool,
) -> Result<(String, SqlxValues), String> {
    if filters.is_empty() {
        return Err("UPDATE requires at least one filter".into());
    }

    let obj = body.as_object().ok_or("Payload must be a JSON object")?;
    if obj.is_empty() {
        return Err("Empty update payload".into());
    }

    let mut stmt = Query::update();
    stmt.table((Alias::new(schema), Alias::new(table)));

    for (k, v) in obj {
        stmt.value(Alias::new(k), json_to_sea_value(v));
    }

    for f in filters {
        let expr = build_condition(f)?;
        stmt.and_where(expr);
    }

    if !return_minimal {
        stmt.returning_all();
        let (inner_sql, values) = stmt.build_sqlx(PostgresQueryBuilder);
        let sql = format!(
            "SELECT COALESCE(json_agg(row_to_json(_t)), '[]'::json) FROM ({}) _t",
            inner_sql
        );
        Ok((sql, values))
    } else {
        let (inner_sql, values) = stmt.build_sqlx(PostgresQueryBuilder);
        let sql = format!("WITH _update AS ({}) SELECT '[]'::json", inner_sql);
        Ok((sql, values))
    }
}

pub fn build_delete(
    schema: &str,
    table: &str,
    filters: &[Filter],
    return_minimal: bool,
) -> Result<(String, SqlxValues), String> {
    if filters.is_empty() {
        return Err("DELETE requires at least one filter".into());
    }

    let mut stmt = Query::delete();
    stmt.from_table((Alias::new(schema), Alias::new(table)));

    for f in filters {
        let expr = build_condition(f)?;
        stmt.and_where(expr);
    }

    if !return_minimal {
        stmt.returning_all();
        let (inner_sql, values) = stmt.build_sqlx(PostgresQueryBuilder);
        let sql = format!(
            "SELECT COALESCE(json_agg(row_to_json(_t)), '[]'::json) FROM ({}) _t",
            inner_sql
        );
        Ok((sql, values))
    } else {
        let (inner_sql, values) = stmt.build_sqlx(PostgresQueryBuilder);
        let sql = format!("WITH _delete AS ({}) SELECT '[]'::json", inner_sql);
        Ok((sql, values))
    }
}

// ── serde_json → sea_query::Value conversion ─────────────────────────────────

fn json_to_sea_value(v: &Value) -> sea_query::Value {
    match v {
        Value::Null            => sea_query::Value::String(None),
        Value::Bool(b)         => sea_query::Value::Bool(Some(*b)),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                sea_query::Value::BigInt(Some(i))
            } else if let Some(f) = n.as_f64() {
                sea_query::Value::Double(Some(f))
            } else {
                sea_query::Value::String(Some(Box::new(n.to_string())))
            }
        }
        Value::String(s)       => sea_query::Value::String(Some(Box::new(s.clone()))),
        Value::Array(_) | Value::Object(_) => {
            // JSON columns: bind as text, let PG cast to jsonb
            sea_query::Value::String(Some(Box::new(v.to_string())))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::filter::parse_filter;
    use crate::parser::select::parse_select;
    use crate::parser::order::parse_order;
    use crate::sql::ast::{Operation, CountMethod};

    fn make_ast(table: &str, filters_str: &[&str]) -> QueryAst {
        let filters = filters_str
            .iter()
            .map(|s| parse_filter(s).unwrap())
            .collect();
        QueryAst {
            table: table.to_string(),
            schema: "public".to_string(),
            operation: Operation::Select,
            select: vec![],
            filters,
            order: vec![],
            limit: None,
            offset: None,
            count: CountMethod::None,
        }
    }

    #[test]
    fn test_build_select_no_filters() {
        let ast = make_ast("users", &[]);
        let (sql, _) = build_select(&ast).unwrap();
        assert!(sql.contains("json_agg"));
        assert!(sql.contains("\"users\""));
        assert!(sql.contains("LIMIT 1000 OFFSET 0"));
    }

    #[test]
    fn test_build_select_with_eq_filter() {
        let ast = make_ast("users", &["age.eq.25"]);
        let (sql, _vals) = build_select(&ast).unwrap();
        // SeaQuery uses $1 placeholders
        assert!(sql.contains("$1"));
        assert!(sql.contains("\"age\""));
    }

    #[test]
    fn test_build_select_with_in_filter() {
        let ast = make_ast("users", &["status.in.(active,inactive)"]);
        let (sql, _) = build_select(&ast).unwrap();
        assert!(sql.contains("\"status\""));
        assert!(sql.contains("IN") || sql.contains("in"));
    }

    #[test]
    fn test_build_select_with_null_filter() {
        let ast = make_ast("users", &["deleted_at.is.null"]);
        let (sql, _) = build_select(&ast).unwrap();
        assert!(sql.to_uppercase().contains("IS NULL"));
    }

    #[test]
    fn test_build_select_with_order() {
        let mut ast = make_ast("users", &[]);
        ast.order = parse_order("name.asc.nullslast").unwrap();
        let (sql, _) = build_select(&ast).unwrap();
        assert!(sql.contains("\"name\""));
        assert!(sql.to_uppercase().contains("ASC"));
    }

    #[test]
    fn test_build_insert() {
        let body = serde_json::json!({"name": "Alice", "age": 30});
        let (sql, _) = build_insert("public", "users", &body, false, None).unwrap();
        assert!(sql.contains("\"users\""));
        assert!(sql.contains("json_agg"));
    }

    #[test]
    fn test_build_insert_return_minimal() {
        let body = serde_json::json!({"name": "Bob"});
        let (sql, _) = build_insert("public", "users", &body, true, None).unwrap();
        assert!(sql.contains("_insert"));
        assert!(!sql.contains("json_agg"));
    }

    #[test]
    fn test_build_update_requires_filter() {
        let body = serde_json::json!({"name": "Alice"});
        let result = build_update("public", "users", &body, &[], false);
        assert!(result.is_err());
    }

    #[test]
    fn test_build_delete_requires_filter() {
        let result = build_delete("public", "users", &[], false);
        assert!(result.is_err());
    }
}
```

**Step 3: Fix `apply_order` — SeaQuery 0.32 API**

The `order_by_with_nulls` method signature may differ across SeaQuery versions. The correct call for ordering with nulls placement is:

```rust
// In apply_order, replace the loop body with:
for node in nodes {
    let dir = match node.direction {
        Direction::Asc  => Order::Asc,
        Direction::Desc => Order::Desc,
    };
    let nulls = match node.nulls {
        Some(NullsOrder::First) => Some(sea_query::NullOrdering::First),
        Some(NullsOrder::Last)  => Some(sea_query::NullOrdering::Last),
        None => None,
    };
    if let Some(nulls_order) = nulls {
        stmt.order_by_with_nulls(col(&node.column), dir, nulls_order);
    } else {
        stmt.order_by(col(&node.column), dir);
    }
}
```

**Step 4: Check compilation**

```bash
cargo check 2>&1 | head -50
```

Expected: errors only from `rest.rs` which still calls old signature. Fix those next.

---

## Task 4: Update `rest.rs` Callers

**Files:**
- Modify: `src/api/rest.rs`

**Context:** `handle_select`, `handle_insert`, `handle_update`, `handle_delete` all call `SqlBuilder::build_*` and then `execute_query`. The return types have changed from `Vec<serde_json::Value>` to `SqlxValues`. Update import and all four call sites.

**Step 1: Update the import block at the top of `rest.rs`**

Remove the `SqlBuilder` import (it's now accessed via module path) and update:

```rust
use crate::sql::builder::{build_select, build_insert, build_update, build_delete};
```

Replace the line:
```rust
use crate::sql::builder::SqlBuilder;
```

**Step 2: Update `handle_select`**

```rust
// Old:
let (sql, params_vec) = SqlBuilder::build_select(&ast).map_err(|e| PostgRestError::from(e))?;
let result = execute_query(&state.pool, &sql, params_vec, &rls).await...

// New:
let (sql, values) = build_select(&ast).map_err(|e| PostgRestError::from(e))?;
let result = execute_query(&state.pool, &sql, values, &rls).await...
```

**Step 3: Update `handle_insert`**

```rust
// Old:
let (sql, params_vec) = SqlBuilder::build_insert(...).map_err(...)?;

// New:
let (sql, values) = build_insert("public", &table, &body, opts.return_minimal, opts.resolution.as_ref()).map_err(|e| PostgRestError::from(e))?;
let result = execute_query(&state.pool, &sql, values, &rls).await...
```

**Step 4: Update `handle_update`**

```rust
// Old:
let (sql, params_vec) = SqlBuilder::build_update(...).map_err(...)?;

// New:
let (sql, values) = build_update("public", &table, &body, &filters, opts.return_minimal).map_err(|e| PostgRestError::from(e))?;
let result = execute_query(&state.pool, &sql, values, &rls).await...
```

**Step 5: Update `handle_delete`**

```rust
// Old:
let (sql, params_vec) = SqlBuilder::build_delete(...).map_err(...)?;

// New:
let (sql, values) = build_delete("public", &table, &filters, opts.return_minimal).map_err(|e| PostgRestError::from(e))?;
let result = execute_query(&state.pool, &sql, values, &rls).await...
```

**Step 6: Full compile check**

```bash
cargo build 2>&1
```

Expected: clean build, zero warnings about unused `SqlBuilder`.

---

## Task 5: Run Unit Tests (Rust layer)

**Step 1: Run all Rust unit tests**

```bash
cargo test 2>&1
```

Expected: all tests in `builder.rs`, `filter.rs`, `select.rs`, `order.rs` pass. The new `builder.rs` tests from Task 3 (9 tests) should all pass.

**Step 2: Fix any compilation / test failures**

Common issues and fixes:

| Error | Fix |
|---|---|
| `build_sqlx not found` | Ensure `use sea_query_binder::SqlxBinder` is in scope |
| `NullOrdering not found` | `use sea_query::NullOrdering` |
| `value() takes different args` | Check SeaQuery 0.32 docs: `stmt.value(col, val)` |
| `OnConflict::new().do_nothing()` build error | Use `OnConflict::new().do_nothing_on(...)` or just `stmt.on_conflict(OnConflict::new().do_nothing().to_owned())` |

**Step 3: Commit**

```bash
git add src/sql/builder.rs src/db/execute.rs src/api/rest.rs
git commit -m "refactor(sql): replace string-built SQL with SeaQuery AST in builder + execute"
```

---

## Task 6: Run Full Vitest Integration Tests

**Context:** The 21 Vitest tests in `test-client/` hit the running server end-to-end. They test auth, REST CRUD, storage, and RLS. All must pass.

**Step 1: Start the server**

```bash
cargo run -- start &
# Wait a few seconds for pg-embed to initialize
sleep 5
```

**Step 2: Run all Vitest tests**

```bash
cd test-client && npx vitest run 2>&1
```

Expected:
```
Test Files  X passed
Tests       21 passed
```

**Step 3: If tests fail — diagnostic strategy**

Do NOT change the parser. Check these in order:

1. **Filter binding type mismatch** — e.g., `age = $1` where `$1` is `String("25")` but column is `integer`. Fix: for filter values from HTTP params (always strings), cast on the SQL side using `Expr::cust_with_values("\"age\" = $1::integer", [val])` for typed columns. A simpler universal fix: rely on PG implicit casting from text, which works for most types.

2. **json_agg wrapping broken** — verify `wrap_json_agg` inner SQL is the SeaQuery-built string, not the post-`build_sqlx` parameterized form. The `build_sqlx` call replaces `$1` placeholders into the string — double-check that `final_sql2` has `$1` placeholders (not literal values inlined).

3. **INSERT RETURNING not working** — SeaQuery `returning_all()` emits `RETURNING *`. The outer `json_agg` wrap must handle that correctly.

4. **SET LOCAL ROLE fails** — unchanged from Phase 1; if this fails it's an unrelated regression.

**Step 4: Commit after green**

```bash
cd D:/Rust/SupaRust
git add -p  # stage only relevant changes
git commit -m "test: verify 21/21 integration tests pass after SeaQuery refactor"
```

---

## Task 7: Cleanup and Polish

**Files:**
- Modify: `src/sql/builder.rs` (remove dead code)
- Modify: `src/sql/mod.rs` (verify re-exports are correct)

**Step 1: Remove unused imports**

```bash
cargo build 2>&1 | grep "unused import"
```

Fix each warning by removing the unused import.

**Step 2: Remove the old `SqlBuilder` struct**

The old `SqlBuilder` struct with `sql`, `params`, `param_counter` fields is now dead. Ensure it's fully removed from `builder.rs`.

**Step 3: Verify `src/sql/mod.rs` exports**

```bash
cat src/sql/mod.rs
```

Update to re-export the new public functions:

```rust
pub mod ast;
pub mod builder;
pub mod rls;
```

**Step 4: Final full test run**

```bash
cargo test 2>&1 && cd test-client && npx vitest run 2>&1
```

Expected: all Rust unit tests pass, all 21 Vitest tests pass.

**Step 5: Final commit**

```bash
cd D:/Rust/SupaRust
git add src/sql/
git commit -m "chore: remove old SqlBuilder struct, clean up unused imports"
```

---

## Key Gotchas Summary

1. **`build_sqlx` vs `build`** — `stmt.build(PostgresQueryBuilder)` returns `(String, Values)` where `Values` is SeaQuery's own type. `stmt.build_sqlx(PostgresQueryBuilder)` returns `(String, SqlxValues)` which is what SQLx expects. Always use `build_sqlx` for the final execute.

2. **`wrap_json_agg` must use the parameterized SQL** — After calling `stmt.build_sqlx(...)`, the returned SQL string still has `$1`, `$2` placeholders. The `wrap_json_agg` call wraps *that* string. The `SqlxValues` carries the actual values. Both go to `execute_query` together. Never inline the values into the SQL string.

3. **Filter values are all strings** — HTTP query params arrive as strings. SeaQuery will bind them as `VARCHAR`. PostgreSQL will coerce `'25'::varchar` → `integer` via implicit cast for most types. If a test fails with type mismatch on a specific column type, add an explicit `::type` cast to the column expression using `Expr::cust()`.

4. **Static queries in `auth.rs` and `storage.rs` are untouched** — these use `sqlx::query!()` macro or raw `sqlx::query()` with explicit binds. Do not change them.

5. **SeaQuery schema+table syntax** — `stmt.from((Alias::new("public"), Alias::new("users")))` produces `"public"."users"`, matching what PostgreSQL expects for schema-qualified tables.
