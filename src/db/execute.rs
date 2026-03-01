use sqlx::{PgPool, Row, types::Json};
use serde_json::Value;
use crate::sql::rls::RlsContext;

pub async fn execute_query(
    pool: &PgPool,
    sql: &str,
    params: Vec<Value>,
    rls: &RlsContext,
) -> Result<Value, sqlx::Error> {
    let mut tx = pool.begin().await?;

    for (name, val) in rls.to_set_local_statements() {
        if name.to_lowercase() == "role" {
            // service_role has BYPASSRLS attribute — SET LOCAL ROLE so PG enforces it
            // anon / authenticated — normal RLS applies
            let stmt = format!("SET LOCAL ROLE {}", val);
            sqlx::query(&stmt).execute(&mut *tx).await?;
        } else {
            let stmt = format!("SET LOCAL \"{}\" TO '{}'", name, val.replace("'", "''"));
            sqlx::query(&stmt).execute(&mut *tx).await?;
        }
    }

    // Construct the query with bindings
    let mut q = sqlx::query(sql);

    // Bind parameters
    for param in params {
        let param_val = match param {
            Value::String(s) => s,
            _ => param.to_string(), // Simplified for now
        };
        q = q.bind(param_val);
    }

    // Gunakan fetch_optional, bukan fetch_one
    let result = q.fetch_optional(&mut *tx).await?;

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
