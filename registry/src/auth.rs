//! Authentication: API keys → JWT, ICE server config.

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use base64::{engine::general_purpose, Engine};
use hmac::{Hmac, Mac};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use sha1::Sha1;
use std::collections::HashMap;
use std::sync::RwLock;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::info;

use crate::AppState;

type HmacSha1 = Hmac<Sha1>;

/// JWT claims.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    pub sub: String, // peer_id
    pub role: String,
    pub exp: usize,
    pub iat: usize,
}

/// API key entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyEntry {
    pub peer_id: String,
    pub role: String,
}

pub struct AuthState {
    secret: String,
    api_keys: RwLock<HashMap<String, ApiKeyEntry>>,
}

impl AuthState {
    pub fn new(secret: &str) -> Self {
        // Load API keys from file or defaults
        let keys = Self::load_api_keys();
        Self {
            secret: secret.to_string(),
            api_keys: RwLock::new(keys),
        }
    }

    fn load_api_keys() -> HashMap<String, ApiKeyEntry> {
        let path = std::env::var("API_KEYS_FILE").unwrap_or_else(|_| "api_keys.json".into());
        if let Ok(data) = std::fs::read_to_string(&path) {
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&data) {
                if let Some(keys) = parsed.get("keys").and_then(|k| k.as_object()) {
                    return keys
                        .iter()
                        .filter_map(|(k, v)| {
                            let entry: ApiKeyEntry = serde_json::from_value(v.clone()).ok()?;
                            Some((k.clone(), entry))
                        })
                        .collect();
                }
            }
        }

        // Default development keys
        let mut m = HashMap::new();
        m.insert(
            "ak_dev_001".into(),
            ApiKeyEntry {
                peer_id: "agent-001".into(),
                role: "agent".into(),
            },
        );
        m
    }

    pub fn issue_token(&self, peer_id: &str, role: &str) -> Result<String, String> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as usize;

        let claims = Claims {
            sub: peer_id.to_string(),
            role: role.to_string(),
            iat: now,
            exp: now + 300, // 5 minutes
        };

        encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(self.secret.as_bytes()),
        )
        .map_err(|e| e.to_string())
    }

    pub fn validate_token(&self, token: &str) -> Result<Claims, String> {
        decode::<Claims>(
            token,
            &DecodingKey::from_secret(self.secret.as_bytes()),
            &Validation::default(),
        )
        .map(|td| td.claims)
        .map_err(|e| e.to_string())
    }

    pub fn lookup_api_key(&self, key: &str) -> Option<ApiKeyEntry> {
        self.api_keys.read().ok()?.get(key).cloned()
    }

    /// Return the set of all peer_ids that have static API keys (legacy agents).
    pub fn api_key_peer_ids(&self) -> std::collections::HashSet<String> {
        self.api_keys
            .read()
            .map(|keys| keys.values().map(|e| e.peer_id.clone()).collect())
            .unwrap_or_default()
    }
}

/// POST /api/token — exchange API key or device token for JWT.
pub async fn exchange_token(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // Try API key first (existing path)
    if let Some(api_key) = headers.get("x-api-key").and_then(|v| v.to_str().ok()) {
        if let Some(entry) = state.auth.lookup_api_key(api_key) {
            let token = state
                .auth
                .issue_token(&entry.peer_id, &entry.role)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

            info!("[AUTH] issued token for peer_id={} (api_key)", entry.peer_id);

            return Ok(Json(serde_json::json!({
                "token": token,
                "peer_id": entry.peer_id,
                "role": entry.role
            })));
        }
    }

    // Fallback: device token (for paired agents)
    if let Some(device_token) = headers.get("x-device-token").and_then(|v| v.to_str().ok()) {
        let entry = state
            .pairing
            .validate_device_token(device_token)
            .ok_or(StatusCode::UNAUTHORIZED)?;

        let token = state
            .auth
            .issue_token(&entry.peer_id, "agent")
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        info!("[AUTH] issued token for peer_id={} (device_token)", entry.peer_id);

        return Ok(Json(serde_json::json!({
            "token": token,
            "peer_id": entry.peer_id,
            "role": "agent"
        })));
    }

    Err(StatusCode::UNAUTHORIZED)
}

/// GET /api/config — return ICE server configuration (STUN + optional TURN).
pub async fn ice_config() -> Json<serde_json::Value> {
    let mut servers = vec![serde_json::json!({
        "urls": ["stun:stun.l.google.com:19302"]
    })];

    if let Ok(turn_url) = std::env::var("TURN_URL") {
        if let Ok(secret) = std::env::var("TURN_SECRET") {
            let (username, credential) = generate_turn_credentials(&secret, 86400);
            servers.push(serde_json::json!({
                "urls": [turn_url],
                "username": username,
                "credential": credential
            }));
        }
    }

    Json(serde_json::json!({ "iceServers": servers }))
}

/// Generate ephemeral TURN credentials (coturn use-auth-secret mode).
fn generate_turn_credentials(secret: &str, ttl_secs: u64) -> (String, String) {
    let expiry = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + ttl_secs;
    let username = format!("{}:mesh", expiry);
    let mut mac =
        HmacSha1::new_from_slice(secret.as_bytes()).expect("HMAC accepts keys of any size");
    mac.update(username.as_bytes());
    let password = general_purpose::STANDARD.encode(mac.finalize().into_bytes());
    (username, password)
}
