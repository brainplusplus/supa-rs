use serde_json::Value;

use crate::parser::filter::{Filter, FilterValue, Operator};
use crate::parser::select::SelectNode;
use crate::parser::order::OrderNode;
use crate::sql::ast::QueryAst;

pub struct SqlBuilder {
    pub sql: String,
    pub params: Vec<serde_json::Value>,
    param_counter: usize,
}

impl SqlBuilder {
    pub fn new() -> Self {
        Self { sql: String::new(), params: Vec::new(), param_counter: 0 }
    }

    pub fn next_param(&mut self, value: serde_json::Value) -> String {
        self.param_counter += 1;
        self.params.push(value);
        format!("${}", self.param_counter)
    }

    pub fn build_select(ast: &QueryAst) -> Result<(String, Vec<serde_json::Value>), String> {
        let mut builder = SqlBuilder::new();

        let projection = Self::build_projection(&ast.select);
        let mut inner_sql = format!("SELECT {} FROM {}.\"{}\"", projection, ast.schema, ast.table);

        if !ast.filters.is_empty() {
            let where_clause = builder.build_where_internal(&ast.filters)?;
            if !where_clause.is_empty() {
                inner_sql.push_str(" WHERE ");
                inner_sql.push_str(&where_clause);
            }
        }

        let order_clause = Self::build_order(&ast.order);
        if !order_clause.is_empty() {
            inner_sql.push_str(" ORDER BY ");
            inner_sql.push_str(&order_clause);
        }

        let limit = ast.limit.unwrap_or(1000);
        let offset = ast.offset.unwrap_or(0);

        let wrapped = Self::wrap_json_agg(&inner_sql, limit, offset);
        builder.sql = wrapped;

        Ok((builder.sql, builder.params))
    }

    fn build_projection(nodes: &[SelectNode]) -> String {
        if nodes.is_empty() {
            return "*".to_string();
        }

        nodes
            .iter()
            .map(|node| {
                let mut col = format!("\"{}\"", node.name);
                if let Some(json) = &node.json_path {
                    col = format!("{}{}", col, json);
                }
                if let Some(cast) = &node.cast {
                    col = format!("{}::{}", col, cast);
                }
                if let Some(alias) = &node.alias {
                    col = format!("{} AS \"{}\"", col, alias);
                }
                col
            })
            .collect::<Vec<String>>()
            .join(", ")
    }

    pub fn build_where(&mut self, filters: &[Filter]) -> Result<String, String> {
        self.build_where_internal(filters)
    }

    fn build_where_internal(&mut self, filters: &[Filter]) -> Result<String, String> {
        if filters.is_empty() {
            return Ok(String::new());
        }

        let mut clauses = Vec::new();
        for filter in filters {
            clauses.push(self.build_filter(filter)?);
        }

        Ok(clauses.join(" AND "))
    }

    fn build_filter(&mut self, filter: &Filter) -> Result<String, String> {
        match filter {
            Filter::Column(c) => {
                let col = format!("\"{}\"", c.column);
                let op_str = Self::operator_to_sql(&c.operator, c.negated);

                match &c.value {
                    FilterValue::Null => {
                        Ok(format!("{} {}", col, op_str))
                    }
                    FilterValue::Single(val) => {
                        let p = self.next_param(serde_json::Value::String(val.clone()));
                        Ok(format!("{} {} {}", col, op_str, p))
                    }
                    FilterValue::List(vals) => {
                        if matches!(c.operator, Operator::In | Operator::NotIn) {
                            // Postgres syntax: column IN (val1, val2) or column != ALL(ARRAY[val1, val2])
                            // Let's use ANY and ALL with arrays.
                            if c.negated || matches!(c.operator, Operator::NotIn) {
                                let p = self.next_param(serde_json::Value::Array(vals.iter().map(|v| serde_json::Value::String(v.clone())).collect()));
                                Ok(format!("{} != ALL(CAST({} AS text[]))", col, p)) // Simplified type handling for strings
                            } else {
                                let p = self.next_param(serde_json::Value::Array(vals.iter().map(|v| serde_json::Value::String(v.clone())).collect()));
                                Ok(format!("{} = ANY(CAST({} AS text[]))", col, p))
                            }
                        } else {
                            Err(format!("List value not supported for operator {:?}", c.operator))
                        }
                    }
                }
            }
            Filter::And(filters) => {
                let mut clauses = Vec::new();
                for f in filters {
                    clauses.push(self.build_filter(f)?);
                }
                Ok(format!("({})", clauses.join(" AND ")))
            }
            Filter::Or(filters) => {
                let mut clauses = Vec::new();
                for f in filters {
                    clauses.push(self.build_filter(f)?);
                }
                Ok(format!("({})", clauses.join(" OR ")))
            }
        }
    }

    fn operator_to_sql(op: &Operator, negated: bool) -> &'static str {
        if negated {
             match op {
                Operator::Eq => "!=",
                Operator::Neq => "=",
                Operator::Lt => ">=",
                Operator::Lte => ">",
                Operator::Gt => "<=",
                Operator::Gte => "<",
                Operator::Like => "NOT LIKE",
                Operator::Ilike => "NOT ILIKE",
                Operator::Is => "IS NOT",
                Operator::IsNot => "IS",
                Operator::In => "!= ALL", // Approximate
                _ => "!="
            }
        } else {
            match op {
                Operator::Eq => "=",
                Operator::Neq => "!=",
                Operator::Lt => "<",
                Operator::Lte => "<=",
                Operator::Gt => ">",
                Operator::Gte => ">=",
                Operator::Like => "LIKE",
                Operator::Ilike => "ILIKE",
                Operator::Is => "IS",
                Operator::IsNot => "IS NOT",
                Operator::In => "= ANY", // Approximate
                Operator::Contains => "@>",
                Operator::ContainedBy => "<@",
                Operator::Fts => "@@ to_tsquery",
                _ => "="
            }
        }
    }

    fn build_order(nodes: &[OrderNode]) -> String {
        nodes
            .iter()
            .map(|node| {
                let dir = match node.direction {
                    crate::parser::Direction::Asc => "ASC",
                    crate::parser::Direction::Desc => "DESC",
                };
                let nulls = match node.nulls {
                    Some(crate::parser::NullsOrder::First) => "NULLS FIRST",
                    Some(crate::parser::NullsOrder::Last) => "NULLS LAST",
                    None => "",
                };
                let mut sql = format!("\"{}\" {}", node.column, dir);
                if !nulls.is_empty() {
                    sql = format!("{} {}", sql, nulls);
                }
                sql
            })
            .collect::<Vec<String>>()
            .join(", ")
    }

    fn wrap_json_agg(inner_sql: &str, limit: i64, offset: i64) -> String {
        format!(
            "SELECT COALESCE(json_agg(row_to_json(_t)), '[]'::json)\nFROM (\n  {} LIMIT {} OFFSET {}\n) _t",
            inner_sql, limit, offset
        )
    }

    pub fn build_insert(
        schema: &str,
        table: &str,
        body: &Value,
        return_minimal: bool,
        resolution: Option<&String>
    ) -> Result<(String, Vec<serde_json::Value>), String> {
        let mut builder = SqlBuilder::new();
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
        // keep columns order stable
        columns.sort();

        let mut values_clauses = Vec::new();
        for row in rows {
            let obj = row.as_object().ok_or("Payload must be an array of objects")?;
            let mut row_vals = Vec::new();
            for col in &columns {
                let val = obj.get(col).unwrap_or(&Value::Null);
                row_vals.push(builder.next_param(val.clone()));
            }
            values_clauses.push(format!("({})", row_vals.join(", ")));
        }

        let mut sql = format!(
            "INSERT INTO {}.\"{}\" ({}) VALUES {}",
            schema, table,
            columns.iter().map(|c| format!("\"{}\"", c)).collect::<Vec<_>>().join(", "),
            values_clauses.join(", ")
        );

        if let Some(res) = resolution {
            if res == "merge-duplicates" || res == "ignore-duplicates" {
                // Approximate for merge-duplicates: on conflict do update. We need a primary key, but usually PostgREST uses ON CONFLICT ON CONSTRAINT or something. But here we can use ON CONFLICT DO NOTHING for ignore, DO UPDATE SET for merge if columns are provided.
                // For simplicity, do nothing if ignore. If merge-duplicates, PostgREST does upsert.
                // We'll just do ON CONFLICT DO UPDATE SET for all columns if merge, DO NOTHING for ignore
                 if res == "ignore-duplicates" {
                     sql.push_str(" ON CONFLICT DO NOTHING");
                 } else {
                     return Err("merge-duplicates resolution requires ON CONFLICT DO UPDATE which needs a unique constraint specified".to_string());
                 }
            }
        }

        if !return_minimal {
            sql.push_str(" RETURNING *");
            sql = format!("SELECT COALESCE(json_agg(row_to_json(_t)), '[]'::json) FROM ({}) _t", sql);
        } else {
            // we return empty array
            sql = format!("WITH _insert AS ({}) SELECT '[]'::json", sql);
        }

        builder.sql = sql;
        Ok((builder.sql, builder.params))
    }

    pub fn build_update(
        schema: &str,
        table: &str,
        body: &Value,
        filters: &[Filter],
        return_minimal: bool
    ) -> Result<(String, Vec<serde_json::Value>), String> {
        if filters.is_empty() {
            return Err("UPDATE requires at least one filter".into());
        }

        let mut builder = SqlBuilder::new();
        let obj = body.as_object().ok_or("Payload must be a JSON object")?;

        let mut set_clauses = Vec::new();
        for (k, v) in obj {
            let p = builder.next_param(v.clone());
            set_clauses.push(format!("\"{}\" = {}", k, p));
        }

        if set_clauses.is_empty() {
            return Err("Empty update payload".into());
        }

        let where_clause = builder.build_where_internal(filters)?;

        let mut sql = format!(
            "UPDATE {}.\"{}\" SET {} WHERE {}",
            schema, table,
            set_clauses.join(", "),
            where_clause
        );

        if !return_minimal {
            sql.push_str(" RETURNING *");
            sql = format!("SELECT COALESCE(json_agg(row_to_json(_t)), '[]'::json) FROM ({}) _t", sql);
        } else {
            sql = format!("WITH _update AS ({}) SELECT '[]'::json", sql);
        }

        builder.sql = sql;
        Ok((builder.sql, builder.params))
    }

    pub fn build_delete(
        schema: &str,
        table: &str,
        filters: &[Filter],
        return_minimal: bool
    ) -> Result<(String, Vec<serde_json::Value>), String> {
        if filters.is_empty() {
            return Err("DELETE requires at least one filter".into());
        }

        let mut builder = SqlBuilder::new();
        let where_clause = builder.build_where_internal(filters)?;

        let mut sql = format!("DELETE FROM {}.\"{}\" WHERE {}", schema, table, where_clause);

        if !return_minimal {
            sql.push_str(" RETURNING *");
            sql = format!("SELECT COALESCE(json_agg(row_to_json(_t)), '[]'::json) FROM ({}) _t", sql);
        } else {
            sql = format!("WITH _delete AS ({}) SELECT '[]'::json", sql);
        }

        builder.sql = sql;
        Ok((builder.sql, builder.params))
    }
}
