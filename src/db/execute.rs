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
