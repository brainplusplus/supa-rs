use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::PgPool;
use std::path::Component;
use tokio::fs;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation, Algorithm};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone)]
pub struct StorageState {
    pub pool: PgPool,
    pub storage_root: String,
    pub jwt_secret: String,
}

#[derive(Serialize)]
struct StorageError {
    message: String,
    error: String,
}

impl StorageError {
    fn not_found(msg: &str) -> Response {
        (StatusCode::NOT_FOUND, Json(Self {
            message: msg.to_string(),
            error: "NotFound".to_string(),
        })).into_response()
    }
    fn unauthorized(msg: &str) -> Response {
        (StatusCode::FORBIDDEN, Json(Self {
            message: msg.to_string(),
            error: "Unauthorized".to_string(),
        })).into_response()
    }
    fn bad_request(msg: &str) -> Response {
        (StatusCode::BAD_REQUEST, Json(Self {
            message: msg.to_string(),
            error: "InvalidInput".to_string(),
        })).into_response()
    }
    fn internal(msg: &str) -> Response {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(Self {
            message: msg.to_string(),
            error: "InternalError".to_string(),
        })).into_response()
    }
}

pub fn router(pool: PgPool, storage_root: String, jwt_secret: String) -> Router {
    let state = StorageState { pool, storage_root, jwt_secret };
    Router::new()
        // Bucket routes
        .route("/bucket",     post(create_bucket).get(list_buckets))
        .route("/bucket/{id}", get(get_bucket).delete(delete_bucket))
        // Signed URL serve (before wildcard to avoid conflict)
        .route("/object/signedURL/{token}", get(serve_signed_url))
        // Public download (no JWT)
        .route("/object/public/{bucket}/{*path}", get(download_public))
        // Sign URL generation
        .route("/object/sign/{bucket}/{*path}", post(create_signed_url))
        // Authenticated object operations
        .route("/object/{bucket}/{*path}",
            post(upload_object)
            .get(download_object)
            .delete(delete_object)
        )
        .with_state(state)
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn extract_jwt_role(headers: &HeaderMap, secret: &str) -> (String, Value) {
    let token_str = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|s| s.to_string());

    let token_str = match token_str {
        Some(t) => t,
        None => return ("anon".to_string(), json!({})),
    };

    let mut validation = Validation::new(Algorithm::HS256);
        validation.validate_exp = false;
    validation.validate_nbf = false;
    validation.required_spec_claims = std::collections::HashSet::new();

    match decode::<Value>(&token_str, &DecodingKey::from_secret(secret.as_bytes()), &validation) {
        Ok(data) => {
            let role = data.claims
                .get("role")
                .and_then(|r| r.as_str())
                .unwrap_or("anon")
                .to_string();
            (role, data.claims)
        }
        Err(_) => ("anon".to_string(), json!({})),
    }
}

fn sanitize_path(bucket: &str, path: &str) -> Result<String, Response> {
    // Validate bucket name
    if !bucket.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err(StorageError::bad_request("Invalid bucket name"));
    }
    // Validate path components
    for component in std::path::Path::new(path).components() {
        match component {
            Component::Normal(_) => {} // OK
            _ => return Err(StorageError::bad_request("Invalid path: directory traversal detected")),
        }
    }
    Ok(format!("{}/{}", bucket, path))
}

fn physical_path(storage_root: &str, bucket: &str, path: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(storage_root).join(bucket).join(path)
}

async fn run_rls_query(
    pool: &PgPool,
    role: &str,
    jwt_claims: &Value,
    method: &str,
    path: &str,
    sql: &str,
    params: Vec<Value>,
) -> Result<Value, Response> {
    use crate::sql::rls::RlsContext;
    use crate::db::execute::execute_query;

    let rls = RlsContext {
        role: role.to_string(),
        jwt_claims: jwt_claims.clone(),
        method: method.to_string(),
        path: path.to_string(),
    };

    execute_query(pool, sql, params, &rls)
        .await
        .map_err(|e| {
            tracing::error!("RLS query error: {}", e);
            StorageError::unauthorized("Access denied")
        })
}

// ── Bucket Handlers ───────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct CreateBucketBody {
    id: String,
    name: String,
    #[serde(default)]
    public: bool,
}

async fn create_bucket(
    State(state): State<StorageState>,
    headers: HeaderMap,
    Json(body): Json<CreateBucketBody>,
) -> Response {
    let (role, claims) = extract_jwt_role(&headers, &state.jwt_secret);
    let owner = claims.get("sub").and_then(|s| s.as_str()).unwrap_or("");

    let sql = "WITH r AS (INSERT INTO storage.buckets (id, name, owner, public) \
         VALUES ($1, $2, $3::uuid, $4) RETURNING id, name, public) \
         SELECT COALESCE(json_agg(row_to_json(r)), '[]'::json) FROM r";

    match run_rls_query(
        &state.pool, &role, &claims, "POST", "/storage/v1/bucket",
        sql,
        vec![
            json!(body.id), json!(body.name),
            json!(owner), json!(body.public),
        ],
    ).await {
        Ok(_) => (StatusCode::OK, Json(json!({"name": body.name}))).into_response(),
        Err(e) => e,
    }
}

async fn list_buckets(
    State(state): State<StorageState>,
    headers: HeaderMap,
) -> Response {
    let (role, claims) = extract_jwt_role(&headers, &state.jwt_secret);
    let sql = "SELECT COALESCE(json_agg(row_to_json(r)), '[]'::json) FROM \
        (SELECT id, name, public, created_at FROM storage.buckets) r";

    match run_rls_query(
        &state.pool, &role, &claims, "GET", "/storage/v1/bucket",
        sql, vec![],
    ).await {
        Ok(v) => (StatusCode::OK, Json(v)).into_response(),
        Err(e) => e,
    }
}

async fn get_bucket(
    State(state): State<StorageState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    let (role, claims) = extract_jwt_role(&headers, &state.jwt_secret);
    let sql = "SELECT COALESCE(json_agg(row_to_json(r)), '[]'::json) FROM \
        (SELECT id, name, public, created_at FROM storage.buckets WHERE id = $1) r";

    match run_rls_query(
        &state.pool, &role, &claims, "GET",
        &format!("/storage/v1/bucket/{}", id),
        sql, vec![json!(id)],
    ).await {
        Ok(v) => {
            // Return first element or 404
            if let Some(arr) = v.as_array() {
                if arr.is_empty() {
                    return StorageError::not_found("Bucket not found");
                }
                return (StatusCode::OK, Json(arr[0].clone())).into_response();
            }
            StorageError::not_found("Bucket not found")
        }
        Err(e) => e,
    }
}

async fn delete_bucket(
    State(state): State<StorageState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    let (role, claims) = extract_jwt_role(&headers, &state.jwt_secret);

    // Check if empty first (no RLS needed for count check by owner)
    let count_sql = "SELECT COALESCE(json_agg(row_to_json(r)), '[]'::json) FROM \
        (SELECT COUNT(*) as cnt FROM storage.objects WHERE bucket_id = $1) r";

    match run_rls_query(
        &state.pool, &role, &claims, "GET",
        &format!("/storage/v1/bucket/{}", id),
        count_sql, vec![json!(id)],
    ).await {
        Ok(v) => {
            let cnt = v.as_array()
                .and_then(|a| a.first())
                .and_then(|r| r.get("cnt"))
                .and_then(|c| c.as_i64())
                .unwrap_or(0);
            if cnt > 0 {
                return StorageError::bad_request("Bucket is not empty");
            }
        }
        Err(e) => return e,
    }

    let delete_sql = "SELECT COALESCE(json_agg(row_to_json(r)), '[]'::json) FROM \
        (DELETE FROM storage.buckets WHERE id = $1 RETURNING id) r";

    match run_rls_query(
        &state.pool, &role, &claims, "DELETE",
        &format!("/storage/v1/bucket/{}", id),
        delete_sql, vec![json!(id)],
    ).await {
        Ok(_) => (StatusCode::OK, Json(json!({"message": "Successfully deleted"}))).into_response(),
        Err(e) => e,
    }
}

// ── Object Handlers ───────────────────────────────────────────────────────────

async fn upload_object(
    State(state): State<StorageState>,
    headers: HeaderMap,
    Path((bucket, path)): Path<(String, String)>,
    body: Bytes,
) -> Response {
    let (role, claims) = extract_jwt_role(&headers, &state.jwt_secret);

    // Sanitize path
    let _safe = match sanitize_path(&bucket, &path) {
        Ok(p) => p,
        Err(e) => return e,
    };

    let owner = claims.get("sub").and_then(|s| s.as_str()).unwrap_or("");
    let content_type = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();
    let size = body.len() as i64;
    let metadata = json!({ "mimetype": content_type, "size": size });

    // 1. RLS check via metadata INSERT
    let meta_sql = "WITH r AS (INSERT INTO storage.objects (bucket_id, name, owner, metadata) \
         VALUES ($1, $2, $3::uuid, $4) \
         ON CONFLICT (bucket_id, name) DO UPDATE \
         SET metadata = EXCLUDED.metadata, updated_at = now() \
         RETURNING id) \
         SELECT COALESCE(json_agg(row_to_json(r)), '[]'::json) FROM r";

    let result = run_rls_query(
        &state.pool, &role, &claims, "POST",
        &format!("/storage/v1/object/{}/{}", bucket, path),
        meta_sql,
        vec![json!(bucket), json!(path), json!(owner), metadata],
    ).await;

    let object_id = match result {
        Ok(v) => v.as_array()
            .and_then(|a| a.first())
            .and_then(|r| r.get("id"))
            .and_then(|id| id.as_str())
            .unwrap_or("")
            .to_string(),
        Err(e) => return e,
    };

    // 2. Write to disk ONLY after DB success
    let file_path = physical_path(&state.storage_root, &bucket, &path);
    if let Some(parent) = file_path.parent() {
        if let Err(e) = fs::create_dir_all(parent).await {
            tracing::error!("Failed to create storage dir: {}", e);
            return StorageError::internal("Failed to create storage directory");
        }
    }

    if let Err(e) = fs::write(&file_path, &body).await {
        tracing::error!("Failed to write file: {}", e);
        return StorageError::internal("Failed to write file");
    }

    (StatusCode::OK, Json(json!({
        "Key": format!("{}/{}", bucket, path),
        "Id": object_id
    }))).into_response()
}

async fn download_object(
    State(state): State<StorageState>,
    headers: HeaderMap,
    Path((bucket, path)): Path<(String, String)>,
) -> Response {
    let (role, claims) = extract_jwt_role(&headers, &state.jwt_secret);

    let _safe = match sanitize_path(&bucket, &path) {
        Ok(p) => p,
        Err(e) => return e,
    };

    // 1. RLS check via SELECT
    let sql = "SELECT COALESCE(json_agg(row_to_json(r)), '[]'::json) FROM \
        (SELECT id, metadata FROM storage.objects \
         WHERE bucket_id = $1 AND name = $2) r";

    let result = run_rls_query(
        &state.pool, &role, &claims, "GET",
        &format!("/storage/v1/object/{}/{}", bucket, path),
        sql, vec![json!(bucket), json!(path)],
    ).await;

    let meta = match result {
        Ok(v) => {
            let arr = v.as_array().cloned().unwrap_or_default();
            if arr.is_empty() {
                return StorageError::not_found("Object not found");
            }
            arr.into_iter().next().unwrap_or(json!({}))
        }
        Err(e) => return e,
    };

    // 2. Read from disk
    let file_path = physical_path(&state.storage_root, &bucket, &path);
    match fs::read(&file_path).await {
        Ok(bytes) => {
            let mime = meta.get("metadata")
                .and_then(|m| m.get("mimetype"))
                .and_then(|t| t.as_str())
                .unwrap_or("application/octet-stream")
                .to_string();

            (
                StatusCode::OK,
                [("content-type", mime)],
                bytes,
            ).into_response()
        }
        Err(_) => StorageError::not_found("File not found on disk"),
    }
}

async fn download_public(
    State(state): State<StorageState>,
    Path((bucket, path)): Path<(String, String)>,
) -> Response {
    let _safe = match sanitize_path(&bucket, &path) {
        Ok(p) => p,
        Err(e) => return e,
    };

    // No RLS — direct pool query, but explicit public check
    use sqlx::Row;
    let row = sqlx::query(
        "SELECT b.public, o.metadata FROM storage.buckets b \
         JOIN storage.objects o ON o.bucket_id = b.id \
         WHERE b.id = $1 AND o.name = $2"
    )
    .bind(&bucket)
    .bind(&path)
    .fetch_optional(&state.pool)
    .await;

    match row {
        Ok(Some(r)) => {
            let is_public: bool = r.try_get("public").unwrap_or(false);
            if !is_public {
                return StorageError::unauthorized("Bucket is not public");
            }
            let meta: Value = r.try_get("metadata").unwrap_or(json!({}));
            let mime = meta.get("mimetype")
                .and_then(|t| t.as_str())
                .unwrap_or("application/octet-stream")
                .to_string();

            let file_path = physical_path(&state.storage_root, &bucket, &path);
            match fs::read(&file_path).await {
                Ok(bytes) => (
                    StatusCode::OK,
                    [("content-type", mime)],
                    bytes,
                ).into_response(),
                Err(_) => StorageError::not_found("File not found"),
            }
        }
        Ok(None) => StorageError::not_found("Object not found"),
        Err(_) => StorageError::internal("Database error"),
    }
}

async fn delete_object(
    State(state): State<StorageState>,
    headers: HeaderMap,
    Path((bucket, path)): Path<(String, String)>,
) -> Response {
    let (role, claims) = extract_jwt_role(&headers, &state.jwt_secret);

    let _safe = match sanitize_path(&bucket, &path) {
        Ok(p) => p,
        Err(e) => return e,
    };

    // 1. RLS delete from DB first
    let sql = "SELECT COALESCE(json_agg(row_to_json(r)), '[]'::json) FROM \
        (DELETE FROM storage.objects \
         WHERE bucket_id = $1 AND name = $2 RETURNING name) r";

    match run_rls_query(
        &state.pool, &role, &claims, "DELETE",
        &format!("/storage/v1/object/{}/{}", bucket, path),
        sql, vec![json!(bucket), json!(path)],
    ).await {
        Ok(v) => {
            let arr = v.as_array().cloned().unwrap_or_default();
            if arr.is_empty() {
                return StorageError::not_found("Object not found");
            }
            // 2. Delete physical file
            let file_path = physical_path(&state.storage_root, &bucket, &path);
            let _ = fs::remove_file(&file_path).await; // best-effort

            (StatusCode::OK, Json(json!([{"name": path}]))).into_response()
        }
        Err(e) => e,
    }
}

// ── Signed URL ────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
struct SignedClaims {
    bucket: String,
    path: String,
    exp: u64,
}

#[derive(Deserialize)]
struct SignBody {
    #[serde(rename = "expiresIn")]
    expires_in: Option<u64>,
}

async fn create_signed_url(
    State(state): State<StorageState>,
    headers: HeaderMap,
    Path((bucket, path)): Path<(String, String)>,
    Json(body): Json<SignBody>,
) -> Response {
    let (role, claims) = extract_jwt_role(&headers, &state.jwt_secret);

    // Verify object exists and user has access
    let sql = "SELECT COALESCE(json_agg(row_to_json(r)), '[]'::json) FROM \
        (SELECT id FROM storage.objects WHERE bucket_id = $1 AND name = $2) r";

    match run_rls_query(
        &state.pool, &role, &claims, "GET",
        &format!("/storage/v1/object/{}/{}", bucket, path),
        sql, vec![json!(bucket), json!(path)],
    ).await {
        Ok(v) if v.as_array().map(|a| a.is_empty()).unwrap_or(true) => {
            return StorageError::not_found("Object not found");
        }
        Err(e) => return e,
        Ok(_) => {}
    }

    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    let expires_in = body.expires_in.unwrap_or(3600);

    let signed = SignedClaims {
        bucket: bucket.clone(),
        path: path.clone(),
        exp: now + expires_in,
    };

    match encode(
        &Header::default(),
        &signed,
        &EncodingKey::from_secret(state.jwt_secret.as_bytes()),
    ) {
        Ok(token) => (StatusCode::OK, Json(json!({
            "signedURL": format!("/storage/v1/object/signedURL/{}", token)
        }))).into_response(),
        Err(_) => StorageError::internal("Failed to generate signed URL"),
    }
}

async fn serve_signed_url(
    State(state): State<StorageState>,
    Path(token): Path<String>,
) -> Response {
    let mut validation = Validation::new(Algorithm::HS256);
    validation.validate_exp = true;
    validation.required_spec_claims = std::collections::HashSet::new();

    match decode::<SignedClaims>(
        &token,
        &DecodingKey::from_secret(state.jwt_secret.as_bytes()),
        &validation,
    ) {
        Ok(data) => {
            let SignedClaims { bucket, path, .. } = data.claims;
            let file_path = physical_path(&state.storage_root, &bucket, &path);
            match fs::read(&file_path).await {
                Ok(bytes) => (StatusCode::OK, bytes).into_response(),
                Err(_) => StorageError::not_found("File not found"),
            }
        }
        Err(_) => StorageError::unauthorized("Invalid or expired signed URL"),
    }
}