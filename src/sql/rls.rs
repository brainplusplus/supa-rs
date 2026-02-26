use serde_json::Value;

pub struct RlsContext {
    pub role: String,           // "anon" or "authenticated"
    pub jwt_claims: Value,
    pub method: String,         // "GET", "POST", etc.
    pub path: String,           // request path
}

impl RlsContext {
    // Returns Vec of (setting_name, value) pairs to SET LOCAL
    pub fn to_set_local_statements(&self) -> Vec<(String, String)> {
        vec![
            ("role".to_string(), self.role.clone()),
            ("request.jwt.claims".to_string(), self.jwt_claims.to_string()),
        ]
    }
}
