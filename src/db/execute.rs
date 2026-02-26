use sqlx::{PgPool, Postgres, Row, types::Json};
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
            let stmt = format!("SET LOCAL ROLE {}", val);
            sqlx::query(&stmt).execute(&mut *tx).await?;
        } else {
            // Identifier yg punya titik butuh escaped double quotes ("key")
            // Single quotes pada string value (val) di-escape secara standar pg (ganti ' jadi '')
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

    // Execute query and extract native JSON mapping
    let row = q.fetch_one(&mut *tx).await?;
    let json_result: Json<Value> = row.try_get(0)?;

    tx.commit().await?;

    Ok(json_result.0)
}