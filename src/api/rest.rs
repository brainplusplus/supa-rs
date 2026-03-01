use axum::{
    Router,
    routing::{get, post, patch, delete},
    extract::{State, Path, Query},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response, Json},
};
use std::collections::HashMap;
use serde_json::{Value, json};
use sqlx::PgPool;

use crate::sql::rls::RlsContext;
use crate::sql::builder::{build_select, build_insert, build_update, build_delete};
use crate::sql::ast::{QueryAst, Operation, CountMethod};
use crate::db::execute::execute_query;
use crate::parser::select::parse_select;
use crate::parser::order::parse_order;
use crate::parser::filter::parse_filter;

pub fn router(pool: PgPool, jwt_secret: String) -> axum::Router {
    Router::new()
        .route("/{table}", get(handle_select))
        .route("/{table}", post(handle_insert))
        .route("/{table}", patch(handle_update))
        .route("/{table}", delete(handle_delete))
        .with_state(AppState { pool, jwt_secret })
}

#[derive(Clone)]
struct AppState {
    pool: PgPool,
    jwt_secret: String,
}

#[derive(serde::Serialize)]
struct PostgRestError {
    code: String,
    details: Option<String>,
    hint: Option<String>,
    message: String,
}

impl IntoResponse for PostgRestError {
    fn into_response(self) -> Response {
        (StatusCode::BAD_REQUEST, Json(self)).into_response()
    }
}

impl From<String> for PostgRestError {
    fn from(err: String) -> Self {
        PostgRestError {
            code: "PGRST_ERR".to_string(),
            details: None,
            hint: None,
            message: err,
        }
    }
}

fn sanitize_table_name(name: &str) -> Result<String, PostgRestError> {
    if name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        Ok(name.to_string())
    } else {
        Err(PostgRestError {
            code: "PGRST100".into(),
            message: format!("Invalid table name: {}", name),
            details: None,
            hint: None,
        })
    }
}

struct PreferOptions {
    return_minimal: bool,
    count: CountMethod,
    resolution: Option<String>,
}

fn parse_prefer(headers: &HeaderMap) -> PreferOptions {
    let mut opts = PreferOptions {
        return_minimal: false,
        count: CountMethod::None,
        resolution: None,
    };

    if let Some(prefer) = headers.get("prefer") {
        if let Ok(prefer_str) = prefer.to_str() {
            for part in prefer_str.split(',') {
                let p = part.trim();
                if p == "return=minimal" {
                    opts.return_minimal = true;
                } else if p == "return=representation" {
                    opts.return_minimal = false;
                } else if p == "count=exact" {
                    opts.count = CountMethod::Exact;
                } else if p == "count=planned" {
                    opts.count = CountMethod::Planned;
                } else if p == "count=estimated" {
                    opts.count = CountMethod::Estimated;
                } else if p.starts_with("resolution=") {
                    opts.resolution = Some(p.trim_start_matches("resolution=").to_string());
                }
            }
        }
    }
    opts
}

fn extract_jwt(headers: &HeaderMap, params: &HashMap<String, String>, secret: &str) -> (String, Value) {
    let mut token_str = String::new();

    if let Some(auth) = headers.get("authorization") {
        if let Ok(a) = auth.to_str() {
            if let Some(t) = a.strip_prefix("Bearer ") {
                token_str = t.to_string();
            }
        }
    }

    if token_str.is_empty() {
        if let Some(api) = headers.get("apikey") {
            if let Ok(a) = api.to_str() {
                token_str = a.to_string();
            }
        }
    }

    if token_str.is_empty() {
        if let Some(api) = params.get("apikey") {
            token_str = api.clone();
        }
    }

    if token_str.is_empty() {
        return ("anon".to_string(), json!({}));
    }

    // Decode JWT payload without verifying signature (jsonwebtoken v9 API)
    use jsonwebtoken::{decode, DecodingKey, Validation, Algorithm};

    let mut validation = Validation::new(Algorithm::HS256);
        validation.validate_exp = false;
    validation.validate_nbf = false;
    validation.required_spec_claims = std::collections::HashSet::new();

    let decoded = decode::<serde_json::Value>(
        &token_str,
        &DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    );

    match decoded {
        Ok(token_data) => {
            let claims = token_data.claims;
            let role = claims.get("role")
                .and_then(|r| r.as_str())
                .unwrap_or("anon")
                .to_string();
            (role, claims)
        }
        Err(e) => {
            tracing::warn!("JWT decode failed: {}", e);
            ("anon".to_string(), json!({}))
        }
    }
}

fn parse_query_params(params: &HashMap<String, String>) -> Result<(Vec<crate::parser::filter::Filter>, Option<i64>, Option<i64>, Vec<crate::parser::select::SelectNode>, Vec<crate::parser::order::OrderNode>), PostgRestError> {
    let mut filters = Vec::new();
    let mut limit = None;
    let mut offset = None;
    let mut select = vec![];
    let mut order = vec![];

    for (k, v) in params {
        match k.as_str() {
            "select" => {
                select = parse_select(v).map_err(|e| PostgRestError::from(e.to_string()))?;
            }
            "order" => {
                order = parse_order(v).map_err(|e| PostgRestError::from(e.to_string()))?;
            }
            "limit" => {
                limit = Some(v.parse::<i64>().map_err(|_| PostgRestError::from("Invalid limit".to_string()))?);
            }
            "offset" => {
                offset = Some(v.parse::<i64>().map_err(|_| PostgRestError::from("Invalid offset".to_string()))?);
            }
            "apikey" => {}
            _ => {
                // supabase-js sends: ?email=eq.test@suparust.dev
                // key="email", value="eq.test@suparust.dev"
                // parser expects: "email.eq.test@suparust.dev"
                let filter_str = format!("{}.{}", k, v);
                let filter = parse_filter(&filter_str).map_err(|e| PostgRestError::from(e.to_string()))?;
                filters.push(filter);
            }
        }
    }

    Ok((filters, limit, offset, select, order))
}

async fn handle_select(
    State(state): State<AppState>,
    Path(table): Path<String>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Result<Response, PostgRestError> {
    let table = sanitize_table_name(&table)?;
    let (role, jwt_claims) = extract_jwt(&headers, &params, &state.jwt_secret);

    let rls = RlsContext {
        role,
        jwt_claims,
        method: "GET".to_string(),
        path: format!("/rest/v1/{}", table),
    };

    let (filters, limit, offset, select, order) = parse_query_params(&params)?;
    let opts = parse_prefer(&headers);

    let ast = QueryAst {
        table: table.clone(),
        schema: "public".to_string(),
        operation: Operation::Select,
        select,
        filters,
        order,
        limit,
        offset,
        count: opts.count,
    };

    let (sql, params_vec) = build_select(&ast).map_err(|e| PostgRestError::from(e))?;

    let result = execute_query(&state.pool, &sql, params_vec, &rls).await.map_err(|e| PostgRestError::from(e.to_string()))?;

    let output = result;

    let response = Json(output).into_response();
    Ok(response)
}

async fn handle_insert(
    State(state): State<AppState>,
    Path(table): Path<String>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
    axum::extract::Json(body): axum::extract::Json<Value>,
) -> Result<Response, PostgRestError> {
    let table = sanitize_table_name(&table)?;
    let (role, jwt_claims) = extract_jwt(&headers, &params, &state.jwt_secret);

    let rls = RlsContext {
        role,
        jwt_claims,
        method: "POST".to_string(),
        path: format!("/rest/v1/{}", table),
    };

    let opts = parse_prefer(&headers);

    let (sql, params_vec) = build_insert("public", &table, &body, opts.return_minimal, opts.resolution.as_ref()).map_err(|e| PostgRestError::from(e))?;

    let result = execute_query(&state.pool, &sql, params_vec, &rls).await.map_err(|e| PostgRestError::from(e.to_string()))?;

    if opts.return_minimal {
        Ok(StatusCode::NO_CONTENT.into_response())
    } else {
        Ok((StatusCode::CREATED, Json(result)).into_response())
    }
}

async fn handle_update(
    State(state): State<AppState>,
    Path(table): Path<String>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
    axum::extract::Json(body): axum::extract::Json<Value>,
) -> Result<Response, PostgRestError> {
    let table = sanitize_table_name(&table)?;
    let (role, jwt_claims) = extract_jwt(&headers, &params, &state.jwt_secret);

    let rls = RlsContext {
        role,
        jwt_claims,
        method: "PATCH".to_string(),
        path: format!("/rest/v1/{}", table),
    };

    let (filters, _, _, _, _) = parse_query_params(&params)?;
    if filters.is_empty() {
        return Err(PostgRestError::from("UPDATE requires at least one filter".to_string()));
    }

    let opts = parse_prefer(&headers);

    let (sql, params_vec) = build_update("public", &table, &body, &filters, opts.return_minimal).map_err(|e| PostgRestError::from(e))?;

    let result = execute_query(&state.pool, &sql, params_vec, &rls).await.map_err(|e| PostgRestError::from(e.to_string()))?;

    if opts.return_minimal {
        Ok(StatusCode::NO_CONTENT.into_response())
    } else {
        Ok((StatusCode::OK, Json(result)).into_response())
    }
}

async fn handle_delete(
    State(state): State<AppState>,
    Path(table): Path<String>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Result<Response, PostgRestError> {
    let table = sanitize_table_name(&table)?;
    let (role, jwt_claims) = extract_jwt(&headers, &params, &state.jwt_secret);

    let rls = RlsContext {
        role,
        jwt_claims,
        method: "DELETE".to_string(),
        path: format!("/rest/v1/{}", table),
    };

    let (filters, _, _, _, _) = parse_query_params(&params)?;
    if filters.is_empty() {
        return Err(PostgRestError::from("DELETE requires at least one filter".to_string()));
    }

    let opts = parse_prefer(&headers);

    let (sql, params_vec) = build_delete("public", &table, &filters, opts.return_minimal).map_err(|e| PostgRestError::from(e))?;

    let result = execute_query(&state.pool, &sql, params_vec, &rls).await.map_err(|e| PostgRestError::from(e.to_string()))?;

    if opts.return_minimal {
        Ok(StatusCode::NO_CONTENT.into_response())
    } else {
        Ok((StatusCode::OK, Json(result)).into_response())
    }
}
