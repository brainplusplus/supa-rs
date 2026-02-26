use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{PgPool, Row, Transaction, Postgres};
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use jsonwebtoken::{encode, EncodingKey, Header};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone)]
pub struct AuthState {
    pub pool: PgPool,
    pub jwt_secret: String,
}

#[derive(Serialize)]
struct AuthErrorMsg {
    code: u16,
    msg: String,
}

#[derive(Serialize, Debug)]
pub struct AuthError {
    error: String,
    error_description: String,
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        (StatusCode::BAD_REQUEST, Json(self)).into_response()
    }
}

impl AuthError {
    fn unauthorized(desc: &str) -> (StatusCode, Json<Self>) {
        (
            StatusCode::UNAUTHORIZED,
            Json(Self {
                error: "invalid_grant".into(),
                error_description: desc.into(),
            })
        )
    }
}

fn bad_request(msg: &str) -> AuthError {
    AuthError {
        error: "invalid_request".to_string(),
        error_description: msg.to_string(),
    }
}

#[derive(Deserialize)]
struct TokenParams {
    grant_type: String,
}

#[derive(Deserialize)]
struct TokenBody {
    email: Option<String>,
    password: Option<String>,
    refresh_token: Option<String>,
}

#[derive(Serialize)]
struct TokenResponse {
    access_token: String,
    token_type: String,
    expires_in: i64,
    refresh_token: String,
    user: UserObj,
}

#[derive(Serialize)]
struct UserObj {
    id: String,
    aud: String,
    role: String,
    email: String,
    email_confirmed_at: Option<String>,
    phone: String,
    confirmation_sent_at: Option<String>,
    confirmed_at: Option<String>,
    last_sign_in_at: Option<String>,
    app_metadata: Value,
    user_metadata: Value,
    identities: Vec<Value>,
    created_at: String,
    updated_at: String,
}

#[derive(Serialize, Deserialize)]
struct Claims {
    sub: String,
    email: String,
    role: String,
    aal: String,
    session_id: String,
    app_metadata: Value,
    user_metadata: Value,
    iat: u64,
    exp: u64,
}

fn generate_refresh_token() -> String {
    use rand::Rng;
    let token_bytes: Vec<u8> = rand::thread_rng()
        .sample_iter(&rand::distributions::Standard)
        .take(40)
        .collect();
    // Using base64 crate formatting config (version 0.21+)
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    URL_SAFE_NO_PAD.encode(token_bytes)
}

// Helper per generate response obj
async fn sign_in_user(
    pool: &PgPool,
    jwt_secret: &str,
    user_id: String,
    email: String,
    role: String,
    app_meta: Value,
    user_meta: Value,
    email_confirmed_at: Option<String>,
) -> Result<Json<TokenResponse>, Response> {
    // 1. Insert session
    let user_uuid = match uuid::Uuid::parse_str(&user_id) {
        Ok(u) => u,
        Err(_) => return Err(bad_request("Invalid user_id").into_response()),
    };

    let session_row = sqlx::query("INSERT INTO auth.sessions (user_id) VALUES ($1) RETURNING id")
        .bind(&user_uuid)
        .fetch_one(pool)
        .await
        .map_err(|_| bad_request("Failed to create session").into_response())?;

    let session_id: uuid::Uuid = session_row.try_get("id").map_err(|_| bad_request("Row error").into_response())?;

    // 2. Generate and Insert refresh token
    let refresh_token = generate_refresh_token();
    let parent: Option<String> = None;

    sqlx::query(
        "INSERT INTO auth.refresh_tokens (token, user_id, session_id, parent, revoked) VALUES ($1, $2, $3, $4, false)"
    )
    .bind(&refresh_token)
    .bind(&user_uuid)
    .bind(&session_id)
    .bind(&parent)
    .execute(pool)
    .await
    .map_err(|_| bad_request("Failed to create refresh token").into_response())?;

    // 3. Generate JWT
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    let exp = now + 3600;

    let claims = Claims {
        sub: user_id.clone(),
        email: email.clone(),
        role: role.clone(),
        aal: "aal1".to_string(),
        session_id: session_id.to_string(),
        app_metadata: app_meta.clone(),
        user_metadata: user_meta.clone(),
        iat: now,
        exp,
    };

    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(jwt_secret.as_bytes()),
    )
    .map_err(|_| bad_request("Failed to sign token").into_response())?;

    // 4. Return response
    Ok(Json(TokenResponse {
        access_token: token,
        token_type: "bearer".to_string(),
        expires_in: 3600,
        refresh_token,
        user: UserObj {
            id: user_id,
            aud: "authenticated".to_string(),
            role,
            email,
            email_confirmed_at,
            phone: "".to_string(),
            confirmation_sent_at: None,
            confirmed_at: None,
            last_sign_in_at: None,
            app_metadata: app_meta,
            user_metadata: user_meta,
            identities: vec![],
            created_at: "".to_string(),
            updated_at: "".to_string(),
        }
    }))
}

async fn token_password(
    State(state): State<AuthState>,
    Json(body): Json<TokenBody>,
) -> Result<Json<TokenResponse>, Response> {
    let email = body.email.ok_or_else(|| bad_request("email required").into_response())?;
    let password = body.password.ok_or_else(|| bad_request("password required").into_response())?;

    let row = sqlx::query("SELECT id, encrypted_password, role, email, raw_app_meta_data, raw_user_meta_data, CAST(email_confirmed_at AS TEXT) as email_confirmed_at FROM auth.users WHERE email = $1")
        .bind(&email)
        .fetch_optional(&state.pool)
        .await
        .map_err(|_| bad_request("Database error").into_response())?
        .ok_or_else(|| bad_request("Invalid login credentials").into_response())?;

    let encrypted_password: String = row.try_get("encrypted_password").map_err(|_| bad_request("Data read Err").into_response())?;

    // Verify password
    let parsed_hash = PasswordHash::new(&encrypted_password)
        .map_err(|_| bad_request("Invalid hash in database").into_response())?;
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed_hash)
        .map_err(|_| bad_request("Invalid login credentials").into_response())?;

    let id: uuid::Uuid = row.try_get("id").unwrap_or_default();
    let r_email: Option<String> = row.try_get("email").unwrap_or(None);
    let r_role: Option<String> = row.try_get("role").unwrap_or(None);
    let r_app_meta: Option<Value> = row.try_get("raw_app_meta_data").unwrap_or(None);
    let r_user_meta: Option<Value> = row.try_get("raw_user_meta_data").unwrap_or(None);
    let r_email_conf: Option<String> = row.try_get("email_confirmed_at").unwrap_or(None);

    sign_in_user(
        &state.pool,
        &state.jwt_secret,
        id.to_string(),
        r_email.unwrap_or_default(),
        r_role.unwrap_or("authenticated".to_string()),
        r_app_meta.unwrap_or(serde_json::json!({})),
        r_user_meta.unwrap_or(serde_json::json!({})),
        r_email_conf
    ).await
}

async fn token_refresh(
    State(state): State<AuthState>,
    Json(body): Json<TokenBody>,
) -> Result<Json<TokenResponse>, Response> {
    let refresh_token = body.refresh_token.clone().ok_or_else(|| bad_request("refresh_token required").into_response())?;

    let mut tx = state.pool.begin().await.map_err(|_| bad_request("DB Error").into_response())?;

    let row_result = sqlx::query(
        r#"
        SELECT rt.id, rt.revoked, rt.user_id, rt.session_id, u.email, u.role, u.raw_app_meta_data, u.raw_user_meta_data
        FROM auth.refresh_tokens rt
        JOIN auth.users u ON u.id = rt.user_id
        WHERE rt.token = $1
        "#
    )
    .bind(&refresh_token)
    .fetch_optional(&mut *tx)
    .await;

    let row = match row_result {
        Ok(Some(r)) => r,
        _ => return Err(bad_request("Invalid Refresh Token: Refresh Token Not Found").into_response()),
    };

    let revoked: bool = row.try_get("revoked").unwrap_or(false);
    let session_id: uuid::Uuid = row.try_get("session_id").unwrap_or_default();
    let r_id: i64 = row.try_get("id").unwrap_or_default();
    let user_id: uuid::Uuid = row.try_get("user_id").unwrap_or_default();
    let email: Option<String> = row.try_get("email").unwrap_or(None);
    let role: Option<String> = row.try_get("role").unwrap_or(None);
    let app_meta: Option<Value> = row.try_get("raw_app_meta_data").unwrap_or(None);
    let user_meta: Option<Value> = row.try_get("raw_user_meta_data").unwrap_or(None);

    if revoked {
        // Revoke family
        sqlx::query("UPDATE auth.refresh_tokens SET revoked = true WHERE session_id = $1")
            .bind(&session_id)
            .execute(&mut *tx)
            .await
            .ok();
        tx.commit().await.ok();
        return Err(AuthError::unauthorized("Invalid Refresh Token: Already Revoked").into_response());
    }

    // Revoke current
    sqlx::query("UPDATE auth.refresh_tokens SET revoked = true WHERE id = $1")
        .bind(&r_id)
        .execute(&mut *tx)
        .await
        .map_err(|_| bad_request("DB error").into_response())?;

    // Insert new
    let new_refresh_token = generate_refresh_token();

    sqlx::query(
        "INSERT INTO auth.refresh_tokens (token, user_id, session_id, parent, revoked) VALUES ($1, $2, $3, $4, false)"
    )
    .bind(&new_refresh_token)
    .bind(&user_id)
    .bind(&session_id)
    .bind(&refresh_token)
    .execute(&mut *tx)
    .await
    .map_err(|_| bad_request("Failed to insert new refresh token").into_response())?;

    tx.commit().await.map_err(|_| bad_request("Failed to commit").into_response())?;

    // Generate JWT
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    let exp = now + 3600;

    let app_m = app_meta.unwrap_or(serde_json::json!({}));
    let user_m = user_meta.unwrap_or(serde_json::json!({}));
    let claim_email = email.unwrap_or_default();
    let claim_role = role.unwrap_or("authenticated".to_string());

    let claims = Claims {
        sub: user_id.to_string(),
        email: claim_email.clone(),
        role: claim_role.clone(),
        aal: "aal1".to_string(),
        session_id: session_id.to_string(),
        app_metadata: app_m.clone(),
        user_metadata: user_m.clone(),
        iat: now,
        exp,
    };

    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(state.jwt_secret.as_bytes()),
    )
    .map_err(|_| bad_request("Failed to sign token").into_response())?;

    Ok(Json(TokenResponse {
        access_token: token,
        token_type: "bearer".to_string(),
        expires_in: 3600,
        refresh_token: new_refresh_token,
        user: UserObj {
            id: user_id.to_string(),
            aud: "authenticated".to_string(),
            role: claim_role,
            email: claim_email,
            email_confirmed_at: None,
            phone: "".to_string(),
            confirmation_sent_at: None,
            confirmed_at: None,
            last_sign_in_at: None,
            app_metadata: app_m,
            user_metadata: user_m,
            identities: vec![],
            created_at: "".to_string(),
            updated_at: "".to_string(),
        }
    }))
}

async fn create_token(
    State(state): State<AuthState>,
    query: Query<TokenParams>,
    Json(body): Json<TokenBody>,
) -> Response {
    if query.grant_type == "password" {
        token_password(State(state), Json(body)).await.into_response()
    } else if query.grant_type == "refresh_token" {
        token_refresh(State(state), Json(body)).await.into_response()
    } else {
        bad_request("unsupported_grant_type").into_response()
    }
}

async fn signup(
    State(state): State<AuthState>,
    Json(body): Json<TokenBody>,
) -> Response {
    let email = match body.email {
        Some(e) => e,
        None => return bad_request("email required").into_response(),
    };
    let password = match body.password {
        Some(p) => p,
        None => return bad_request("password required").into_response(),
    };

    // Check if exists
    let existing = sqlx::query("SELECT 1 as x FROM auth.users WHERE email = $1")
        .bind(&email)
        .fetch_optional(&state.pool)
        .await
        .unwrap_or(None);

    if existing.is_some() {
        return (StatusCode::BAD_REQUEST, Json(AuthErrorMsg { code: 400, msg: "User already registered".to_string()})).into_response();
    }

    // Hash password
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let password_hash = match argon2.hash_password(password.as_bytes(), &salt) {
        Ok(h) => h.to_string(),
        Err(_) => return bad_request("Failed to hash password").into_response(),
    };

    let app_meta = serde_json::json!({"provider": "email", "providers": ["email"]});
    let user_meta = serde_json::json!({});

    let row_res = sqlx::query(
        "INSERT INTO auth.users (email, encrypted_password, raw_app_meta_data, raw_user_meta_data) VALUES ($1, $2, $3, $4) RETURNING id, email, CAST(created_at AS TEXT) as created_at"
    )
    .bind(&email)
    .bind(&password_hash)
    .bind(&app_meta)
    .bind(&user_meta)
    .fetch_one(&state.pool)
    .await;

    let row = match row_res {
        Ok(r) => r,
        Err(_) => return bad_request("Failed to insert user").into_response(),
    };

    let row_id: uuid::Uuid = row.try_get("id").unwrap_or_default();

    let identity_data = serde_json::json!({"sub": row_id.to_string(), "email": email});
    let identity_res = sqlx::query(
        "INSERT INTO auth.identities (id, user_id, identity_data, provider) VALUES ($1, $2, $3, 'email')"
    )
    .bind(&row_id.to_string()) // Identities string ID
    .bind(&row_id)
    .bind(&identity_data)
    .execute(&state.pool)
    .await;

    if identity_res.is_err() {
         return bad_request("Failed to insert identity").into_response();
    }

    sign_in_user(&state.pool, &state.jwt_secret, row_id.to_string(), email, "authenticated".to_string(), app_meta, user_meta, None).await.into_response()
}

fn extract_jwt(headers: &HeaderMap) -> Option<String> {
    headers.get("Authorization")?.to_str().ok()?
        .strip_prefix("Bearer ")
        .map(|s| s.to_string())
}

fn unsafe_decode_jwt(token: &str) -> Option<Claims> {
    use jsonwebtoken::{decode, DecodingKey, Validation, Algorithm};
    let mut validation = Validation::new(Algorithm::HS256);
    validation.insecure_disable_signature_validation();
    validation.validate_exp = false;
    validation.validate_nbf = false;
    validation.required_spec_claims = std::collections::HashSet::new();

    let decoded = decode::<Claims>(
        &token,
        &DecodingKey::from_secret(b""),
        &validation,
    );
    decoded.map(|d| d.claims).ok()
}

async fn logout(
    State(state): State<AuthState>,
    headers: HeaderMap,
) -> Response {
    let token = match extract_jwt(&headers) {
        Some(t) => t,
        None => return StatusCode::NO_CONTENT.into_response(),
    };

    let claims = match unsafe_decode_jwt(&token) {
        Some(c) => c,
        None => return StatusCode::NO_CONTENT.into_response(),
    };

    let session_uuid = match uuid::Uuid::parse_str(&claims.session_id) {
        Ok(u) => u,
        Err(_) => return StatusCode::NO_CONTENT.into_response(),
    };

    sqlx::query("DELETE FROM auth.sessions WHERE id = $1")
        .bind(&session_uuid)
        .execute(&state.pool)
        .await
        .ok();

    StatusCode::NO_CONTENT.into_response()
}

async fn get_user(
    State(state): State<AuthState>,
    headers: HeaderMap,
) -> Response {
    let token = match extract_jwt(&headers) {
        Some(t) => t,
        None => return AuthError::unauthorized("Missing token").1.into_response(),
    };

    let claims = match unsafe_decode_jwt(&token) {
        Some(c) => c,
        None => return AuthError::unauthorized("Invalid token").1.into_response(),
    };

    let user_uuid = match uuid::Uuid::parse_str(&claims.sub) {
        Ok(u) => u,
        Err(_) => return AuthError::unauthorized("Invalid sub in token").1.into_response(),
    };

    let row = match sqlx::query(
        "SELECT id, email, role, raw_app_meta_data, raw_user_meta_data, CAST(created_at AS TEXT) as created_at, CAST(updated_at AS TEXT) as updated_at FROM auth.users WHERE id = $1"
    )
    .bind(&user_uuid)
    .fetch_optional(&state.pool)
    .await {
        Ok(Some(r)) => r,
        _ => return bad_request("User not found").into_response(),
    };

    let id: uuid::Uuid = row.try_get("id").unwrap_or_default();
    let email: Option<String> = row.try_get("email").unwrap_or(None);
    let role: Option<String> = row.try_get("role").unwrap_or(None);
    let app_meta: Option<Value> = row.try_get("raw_app_meta_data").unwrap_or(None);
    let user_meta: Option<Value> = row.try_get("raw_user_meta_data").unwrap_or(None);
    let created_at: Option<String> = row.try_get("created_at").unwrap_or(None);
    let updated_at: Option<String> = row.try_get("updated_at").unwrap_or(None);

    Json(UserObj {
        id: id.to_string(),
        aud: "authenticated".to_string(),
        role: role.unwrap_or("authenticated".to_string()),
        email: email.unwrap_or_default(),
        email_confirmed_at: None,
        phone: "".to_string(),
        confirmation_sent_at: None,
        confirmed_at: None,
        last_sign_in_at: None,
        app_metadata: app_meta.unwrap_or(serde_json::json!({})),
        user_metadata: user_meta.unwrap_or(serde_json::json!({})),
        identities: vec![],
        created_at: created_at.unwrap_or_default(),
        updated_at: updated_at.unwrap_or_default(),
    })
    .into_response()
}

#[derive(Deserialize)]
struct UpdateUserBody {
    data: Value,
}

async fn update_user(
    State(state): State<AuthState>,
    headers: HeaderMap,
    Json(body): Json<UpdateUserBody>,
) -> Response {
    let token = match extract_jwt(&headers) {
        Some(t) => t,
        None => return AuthError::unauthorized("Missing token").1.into_response(),
    };

    let claims = match unsafe_decode_jwt(&token) {
        Some(c) => c,
        None => return AuthError::unauthorized("Invalid token").1.into_response(),
    };

    let user_uuid = match uuid::Uuid::parse_str(&claims.sub) {
        Ok(u) => u,
        Err(_) => return AuthError::unauthorized("Invalid sub in token").1.into_response(),
    };

    let row = match sqlx::query(
        "UPDATE auth.users SET raw_user_meta_data = raw_user_meta_data || $1, updated_at = now() WHERE id = $2 RETURNING id, email, role, raw_app_meta_data, raw_user_meta_data, CAST(created_at AS TEXT) as created_at, CAST(updated_at AS TEXT) as updated_at"
    )
    .bind(&body.data)
    .bind(&user_uuid)
    .fetch_optional(&state.pool)
    .await {
        Ok(Some(r)) => r,
        _ => return bad_request("Failed to update user").into_response(),
    };

    let id: uuid::Uuid = row.try_get("id").unwrap_or_default();
    let email: Option<String> = row.try_get("email").unwrap_or(None);
    let role: Option<String> = row.try_get("role").unwrap_or(None);
    let app_meta: Option<Value> = row.try_get("raw_app_meta_data").unwrap_or(None);
    let user_meta: Option<Value> = row.try_get("raw_user_meta_data").unwrap_or(None);
    let created_at: Option<String> = row.try_get("created_at").unwrap_or(None);
    let updated_at: Option<String> = row.try_get("updated_at").unwrap_or(None);

    Json(UserObj {
        id: id.to_string(),
        aud: "authenticated".to_string(),
        role: role.unwrap_or("authenticated".to_string()),
        email: email.unwrap_or_default(),
        email_confirmed_at: None,
        phone: "".to_string(),
        confirmation_sent_at: None,
        confirmed_at: None,
        last_sign_in_at: None,
        app_metadata: app_meta.unwrap_or(serde_json::json!({})),
        user_metadata: user_meta.unwrap_or(serde_json::json!({})),
        identities: vec![],
        created_at: created_at.unwrap_or_default(),
        updated_at: updated_at.unwrap_or_default(),
    })
    .into_response()
}

pub fn router(pool: PgPool, jwt_secret: String) -> Router {
    let state = AuthState { pool, jwt_secret };
    Router::new()
        .route("/token", post(create_token))
        .route("/signup", post(signup))
        .route("/logout", post(logout))
        .route("/user", get(get_user).put(update_user))
        .with_state(state)
}