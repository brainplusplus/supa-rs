use serde_json::Value;
use sea_query::{
    Alias, Condition, Expr, ExprTrait, Order, PostgresQueryBuilder, Query, SimpleExpr,
};
use sea_query::extension::postgres::PgExpr;
use sea_query_binder::{SqlxBinder, SqlxValues};

use crate::parser::filter::{Filter, FilterValue, Operator};
use crate::parser::select::SelectNode;
use crate::sql::ast::QueryAst;

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn json_to_sea_value(v: &Value) -> sea_query::Value {
    match v {
        Value::Null => sea_query::Value::String(None),
        Value::Bool(b) => sea_query::Value::Bool(Some(*b)),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                sea_query::Value::BigInt(Some(i))
            } else if let Some(f) = n.as_f64() {
                sea_query::Value::Double(Some(f))
            } else {
                sea_query::Value::String(Some(Box::new(n.to_string())))
            }
        }
        Value::String(s) => sea_query::Value::String(Some(Box::new(s.clone()))),
        Value::Array(_) | Value::Object(_) => {
            sea_query::Value::String(Some(Box::new(v.to_string())))
        }
    }
}

fn wrap_json_agg(inner_sql: &str, limit: i64, offset: i64) -> String {
    format!(
        "SELECT COALESCE(json_agg(row_to_json(_t)), '[]'::json)\nFROM (\n  {} LIMIT {} OFFSET {}\n) _t",
        inner_sql, limit, offset
    )
}

// ---------------------------------------------------------------------------
// Filter → SeaQuery expression
// ---------------------------------------------------------------------------

fn filter_to_condition(filter: &Filter) -> Result<SimpleExpr, String> {
    match filter {
        Filter::And(filters) => {
            let mut cond = Condition::all();
            for f in filters {
                cond = cond.add(filter_to_condition(f)?);
            }
            Ok(SimpleExpr::from(cond))
        }
        Filter::Or(filters) => {
            let mut cond = Condition::any();
            for f in filters {
                cond = cond.add(filter_to_condition(f)?);
            }
            Ok(SimpleExpr::from(cond))
        }
        Filter::Column(c) => {
            // Use Expr::cust so JSON path columns (e.g. "data->>key") are preserved literally.
            let col_expr = Expr::cust(format!("\"{}\"", c.column));

            let expr: SimpleExpr = match &c.value {
                FilterValue::Null => {
                    match (&c.operator, c.negated) {
                        (Operator::Is, false) | (Operator::IsNot, true) => col_expr.is_null(),
                        (Operator::IsNot, false) | (Operator::Is, true) => col_expr.is_not_null(),
                        _ => col_expr.is_null(),
                    }
                }

                FilterValue::List(vals) => {
                    let sea_vals: Vec<sea_query::Value> = vals
                        .iter()
                        .map(|v| sea_query::Value::String(Some(Box::new(v.clone()))))
                        .collect();

                    let use_not_in =
                        c.negated || matches!(c.operator, Operator::NotIn);

                    if use_not_in {
                        col_expr.is_not_in(sea_vals)
                    } else {
                        col_expr.is_in(sea_vals)
                    }
                }

                FilterValue::Single(val) => {
                    let sea_val =
                        sea_query::Value::String(Some(Box::new(val.clone())));

                    match (&c.operator, c.negated) {
                        (Operator::Eq, false) | (Operator::Neq, true) => col_expr.eq(sea_val),
                        (Operator::Eq, true) | (Operator::Neq, false) => col_expr.ne(sea_val),
                        (Operator::Lt, false) | (Operator::Gte, true) => col_expr.lt(sea_val),
                        (Operator::Lte, false) | (Operator::Gt, true) => col_expr.lte(sea_val),
                        (Operator::Gt, false) | (Operator::Lte, true) => col_expr.gt(sea_val),
                        (Operator::Gte, false) | (Operator::Lt, true) => col_expr.gte(sea_val),
                        (Operator::Like, false) => col_expr.like(val.as_str()),
                        (Operator::Like, true) => col_expr.not_like(val.as_str()),
                        (Operator::Ilike, false) => col_expr.ilike(val.as_str()),
                        (Operator::Ilike, true) => col_expr.not_ilike(val.as_str()),
                        // Fall back to eq for anything not explicitly handled
                        _ => col_expr.eq(sea_val),
                    }
                }
            };

            Ok(expr)
        }
    }
}

// ---------------------------------------------------------------------------
// Projection helper
// ---------------------------------------------------------------------------

fn apply_projection(stmt: &mut sea_query::SelectStatement, nodes: &[SelectNode]) {
    if nodes.is_empty() {
        stmt.column(sea_query::Asterisk);
        return;
    }
    for node in nodes {
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
        stmt.expr(Expr::cust(&col_expr));
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn build_select(ast: &QueryAst) -> Result<(String, SqlxValues), String> {
    let mut stmt = Query::select();
    stmt.from((Alias::new(&ast.schema), Alias::new(&ast.table)));

    apply_projection(&mut stmt, &ast.select);

    for filter in &ast.filters {
        let expr = filter_to_condition(filter)?;
        stmt.and_where(expr);
    }

    // ORDER BY — use raw custom expressions to support NULLS FIRST/LAST
    for node in &ast.order {
        let dir_str = match node.direction {
            crate::parser::Direction::Asc => "ASC",
            crate::parser::Direction::Desc => "DESC",
        };
        let order_sql = match &node.nulls {
            None => format!("\"{}\" {}", node.column, dir_str),
            Some(crate::parser::NullsOrder::First) => {
                format!("\"{}\" {} NULLS FIRST", node.column, dir_str)
            }
            Some(crate::parser::NullsOrder::Last) => {
                format!("\"{}\" {} NULLS LAST", node.column, dir_str)
            }
        };
        // Emit as a custom expression in ORDER BY position
        stmt.order_by(Alias::new(&order_sql), Order::Asc);
    }

    let limit = ast.limit.unwrap_or(1000);
    let offset = ast.offset.unwrap_or(0);

    let (inner_sql, values) = stmt.build_sqlx(PostgresQueryBuilder);
    let final_sql = wrap_json_agg(&inner_sql, limit, offset);

    Ok((final_sql, values))
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

    let mut ins = Query::insert();
    ins.into_table((Alias::new(schema), Alias::new(table)));
    ins.columns(columns.iter().map(|c| Alias::new(c)));

    for row in &rows {
        let obj = row.as_object().ok_or("Payload must be an array of objects")?;
        let row_vals: Vec<SimpleExpr> = columns
            .iter()
            .map(|col| {
                SimpleExpr::Value(json_to_sea_value(obj.get(col).unwrap_or(&Value::Null)))
            })
            .collect();
        ins.values(row_vals).map_err(|e| e.to_string())?;
    }

    // Conflict resolution
    if let Some(res) = resolution {
        if res == "ignore-duplicates" {
            ins.on_conflict(
                sea_query::OnConflict::new().do_nothing().to_owned(),
            );
        } else if res == "merge-duplicates" {
            return Err("merge-duplicates resolution requires ON CONFLICT DO UPDATE which needs a unique constraint specified".to_string());
        }
    }

    ins.returning_all();

    let (inner_sql, values) = ins.build_sqlx(PostgresQueryBuilder);

    let final_sql = if return_minimal {
        format!("WITH _insert AS ({}) SELECT '[]'::json", inner_sql)
    } else {
        format!(
            "SELECT COALESCE(json_agg(row_to_json(_t)), '[]'::json) FROM ({}) _t",
            inner_sql
        )
    };

    Ok((final_sql, values))
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

    let mut upd = Query::update();
    upd.table((Alias::new(schema), Alias::new(table)));

    for (k, v) in obj {
        upd.value(Alias::new(k), json_to_sea_value(v));
    }

    for filter in filters {
        let expr = filter_to_condition(filter)?;
        upd.and_where(expr);
    }

    upd.returning_all();

    let (inner_sql, values) = upd.build_sqlx(PostgresQueryBuilder);

    let final_sql = if return_minimal {
        format!("WITH _update AS ({}) SELECT '[]'::json", inner_sql)
    } else {
        format!(
            "SELECT COALESCE(json_agg(row_to_json(_t)), '[]'::json) FROM ({}) _t",
            inner_sql
        )
    };

    Ok((final_sql, values))
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

    let mut del = Query::delete();
    del.from_table((Alias::new(schema), Alias::new(table)));

    for filter in filters {
        let expr = filter_to_condition(filter)?;
        del.and_where(expr);
    }

    del.returning_all();

    let (inner_sql, values) = del.build_sqlx(PostgresQueryBuilder);

    let final_sql = if return_minimal {
        format!("WITH _delete AS ({}) SELECT '[]'::json", inner_sql)
    } else {
        format!(
            "SELECT COALESCE(json_agg(row_to_json(_t)), '[]'::json) FROM ({}) _t",
            inner_sql
        )
    };

    Ok((final_sql, values))
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::filter::parse_filter;
    use crate::parser::order::parse_order;
    use crate::sql::ast::{CountMethod, Operation};

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
        assert!(sql.contains("json_agg"), "missing json_agg: {}", sql);
        assert!(sql.contains("\"users\""), "missing table: {}", sql);
        assert!(sql.contains("LIMIT 1000 OFFSET 0"), "missing limit: {}", sql);
    }

    #[test]
    fn test_build_select_with_eq_filter() {
        let ast = make_ast("users", &["age.eq.25"]);
        let (sql, _) = build_select(&ast).unwrap();
        assert!(sql.contains("$1"), "missing param placeholder: {}", sql);
        assert!(sql.contains("\"age\""), "missing column: {}", sql);
    }

    #[test]
    fn test_build_select_with_in_filter() {
        let ast = make_ast("users", &["status.in.(active,inactive)"]);
        let (sql, _) = build_select(&ast).unwrap();
        assert!(sql.contains("\"status\""), "missing column: {}", sql);
        let sql_upper = sql.to_uppercase();
        assert!(sql_upper.contains("IN"), "missing IN: {}", sql);
    }

    #[test]
    fn test_build_select_with_null_filter() {
        let ast = make_ast("users", &["deleted_at.is.null"]);
        let (sql, _) = build_select(&ast).unwrap();
        assert!(
            sql.to_uppercase().contains("IS NULL"),
            "missing IS NULL: {}",
            sql
        );
    }

    #[test]
    fn test_build_select_with_order() {
        let mut ast = make_ast("users", &[]);
        ast.order = parse_order("name.asc.nullslast").unwrap();
        let (sql, _) = build_select(&ast).unwrap();
        assert!(sql.contains("\"name\""), "missing order col: {}", sql);
        assert!(sql.to_uppercase().contains("ASC"), "missing ASC: {}", sql);
    }

    #[test]
    fn test_build_insert_returns_json_agg() {
        let body = serde_json::json!({"name": "Alice", "age": 30});
        let (sql, _) = build_insert("public", "users", &body, false, None).unwrap();
        assert!(sql.contains("json_agg"), "missing json_agg: {}", sql);
        assert!(sql.contains("\"users\""), "missing table: {}", sql);
    }

    #[test]
    fn test_build_insert_return_minimal() {
        let body = serde_json::json!({"name": "Bob"});
        let (sql, _) = build_insert("public", "users", &body, true, None).unwrap();
        assert!(sql.contains("_insert"), "missing _insert CTE: {}", sql);
        assert!(!sql.contains("json_agg"), "should not have json_agg: {}", sql);
    }

    #[test]
    fn test_build_update_requires_filter() {
        let body = serde_json::json!({"name": "Alice"});
        assert!(build_update("public", "users", &body, &[], false).is_err());
    }

    #[test]
    fn test_build_delete_requires_filter() {
        assert!(build_delete("public", "users", &[], false).is_err());
    }
}
