//! Admin credential, request guards, and the CORS allowlist (ADR-024).
//!
//! Two guards protect the HTTP API:
//! - [`RequireAdmin`]: the request carries the registry admin token in the
//!   `x-admin-token` header. Used by the pairing admin endpoints.
//! - [`RequireCaller`]: the admin token, a known API key (`x-api-key`) or an
//!   approved device token (`x-device-token`). Used by every other route that
//!   changes state (agent registration, heartbeat, deregistration, tasks).
//!
//! Read-only discovery routes stay open (see THREAT_MODEL.md T9).

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::{header, HeaderMap, HeaderName, HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use rand::RngExt;
use subtle::ConstantTimeEq;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tracing::warn;

use crate::AppState;

/// Header that carries the admin token.
pub const ADMIN_TOKEN_HEADER: &str = "x-admin-token";

/// Env var holding the admin token. Unset or empty → a random token is generated at startup.
pub const ADMIN_TOKEN_ENV: &str = "REGISTRY_ADMIN_TOKEN";

/// Env var holding the comma-separated CORS origin allowlist.
pub const ALLOWED_ORIGINS_ENV: &str = "REGISTRY_ALLOWED_ORIGINS";

/// Default CORS allowlist: the registry itself and the hub dev server on loopback.
/// The bundled hub is same-origin and needs no CORS at all; these cover local tooling.
pub const DEFAULT_ALLOWED_ORIGINS: &str =
    "http://localhost:8080,http://127.0.0.1:8080,http://localhost:5174,http://127.0.0.1:5174";

// ─── Admin token ────────────────────────────────────────────────────────────

pub struct AdminAuth {
    token: String,
}

impl AdminAuth {
    pub fn new(token: impl Into<String>) -> Self {
        Self {
            token: token.into(),
        }
    }

    /// Load the token from `REGISTRY_ADMIN_TOKEN`, or generate one.
    /// Returns the auth state and `true` when the token was generated.
    pub fn from_env() -> (Self, bool) {
        match std::env::var(ADMIN_TOKEN_ENV) {
            Ok(t) if !t.trim().is_empty() => (Self::new(t.trim()), false),
            _ => (Self::new(generate_secret("adm_")), true),
        }
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    /// Constant-time comparison against the configured token.
    pub fn verify(&self, candidate: &str) -> bool {
        constant_time_eq(candidate, &self.token)
    }
}

/// Compare two secrets without an early exit on the first differing byte.
/// (Only the length can leak, which reveals nothing useful for random tokens.)
pub fn constant_time_eq(a: &str, b: &str) -> bool {
    a.as_bytes().ct_eq(b.as_bytes()).into()
}

/// `prefix` + 64 hex chars (256-bit random).
pub fn generate_secret(prefix: &str) -> String {
    let bytes: [u8; 32] = rand::rng().random();
    let hex: String = bytes.iter().map(|b| format!("{:02x}", b)).collect();
    format!("{prefix}{hex}")
}

fn header_str<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|v| v.to_str().ok())
}

fn is_admin(headers: &HeaderMap, state: &AppState) -> bool {
    header_str(headers, ADMIN_TOKEN_HEADER).is_some_and(|t| state.admin.verify(t))
}

fn unauthorized(parts: &Parts, what: &str) -> Response {
    warn!(
        "[AUTH] rejected {} {}: {} required",
        parts.method,
        parts.uri.path(),
        what
    );
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({ "error": format!("{what} required") })),
    )
        .into_response()
}

// ─── Guards ─────────────────────────────────────────────────────────────────

/// Extractor: the request carries the admin token.
pub struct RequireAdmin;

impl FromRequestParts<AppState> for RequireAdmin {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Response> {
        if is_admin(&parts.headers, state) {
            Ok(RequireAdmin)
        } else {
            Err(unauthorized(parts, "admin token"))
        }
    }
}

/// Who made an authenticated request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Caller {
    Admin,
    /// Static API key; carries the key's `peer_id`.
    ApiKey(String),
    /// Approved device token; carries the paired `peer_id`.
    Device(String),
}

/// Extractor: admin token, known API key, or approved device token.
pub struct RequireCaller(pub Caller);

impl FromRequestParts<AppState> for RequireCaller {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Response> {
        let headers = &parts.headers;
        if is_admin(headers, state) {
            return Ok(RequireCaller(Caller::Admin));
        }
        if let Some(entry) =
            header_str(headers, "x-api-key").and_then(|k| state.auth.lookup_api_key(k))
        {
            return Ok(RequireCaller(Caller::ApiKey(entry.peer_id)));
        }
        if let Some(entry) = header_str(headers, "x-device-token")
            .and_then(|t| state.pairing.validate_device_token(t))
        {
            return Ok(RequireCaller(Caller::Device(entry.peer_id)));
        }
        Err(unauthorized(parts, "admin token, API key or device token"))
    }
}

// ─── CORS ───────────────────────────────────────────────────────────────────

/// Origins from `REGISTRY_ALLOWED_ORIGINS`, or the loopback defaults when unset.
/// Set it to an empty string to allow no cross-origin browser access at all.
pub fn allowed_origins_from_env() -> Vec<HeaderValue> {
    let raw = std::env::var(ALLOWED_ORIGINS_ENV).unwrap_or_else(|_| DEFAULT_ALLOWED_ORIGINS.into());
    parse_origins(&raw)
}

/// Parse a comma-separated origin list. Wildcards and invalid entries are dropped with a warning.
pub fn parse_origins(raw: &str) -> Vec<HeaderValue> {
    raw.split(',')
        .map(|o| o.trim().trim_end_matches('/'))
        .filter(|o| !o.is_empty())
        .filter_map(|o| {
            if o == "*" {
                warn!("[CORS] ignoring wildcard origin; list origins explicitly");
                return None;
            }
            match HeaderValue::from_str(o) {
                Ok(v) => Some(v),
                Err(_) => {
                    warn!("[CORS] ignoring invalid origin {:?}", o);
                    None
                }
            }
        })
        .collect()
}

/// CORS layer that only answers the listed origins.
pub fn cors_layer(origins: Vec<HeaderValue>) -> CorsLayer {
    CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_methods([Method::GET, Method::POST, Method::DELETE])
        .allow_headers([
            header::CONTENT_TYPE,
            HeaderName::from_static(ADMIN_TOKEN_HEADER),
            HeaderName::from_static("x-api-key"),
            HeaderName::from_static("x-device-token"),
            HeaderName::from_static("x-pairing-secret"),
        ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_verify_accepts_exact_token_only() {
        let admin = AdminAuth::new("adm_secret");
        assert!(admin.verify("adm_secret"));
        assert!(!admin.verify("adm_secreT"));
        assert!(!admin.verify("adm_secre"));
        assert!(!admin.verify(""));
    }

    #[test]
    fn test_generated_secret_format() {
        let s = generate_secret("adm_");
        assert!(s.starts_with("adm_"));
        assert_eq!(s.len(), 4 + 64);
        assert!(s[4..].chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(s, generate_secret("adm_"));
    }

    #[test]
    fn test_parse_origins() {
        let o = parse_origins(" https://a.example , http://localhost:5174/,,*");
        assert_eq!(o, vec!["https://a.example", "http://localhost:5174"]);
    }

    #[test]
    fn test_parse_origins_empty_means_none() {
        assert!(parse_origins("").is_empty());
    }

    #[test]
    fn test_default_origins_parse() {
        assert_eq!(parse_origins(DEFAULT_ALLOWED_ORIGINS).len(), 4);
    }
}
