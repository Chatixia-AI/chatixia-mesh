//! Chatixia Registry — signaling server + agent registry + hub API.
//!
//! Combines three roles:
//! 1. **Signaling**: WebSocket relay for WebRTC SDP offers/answers and ICE candidates
//! 2. **Registry**: Agent discovery — tracks who's online, what skills they have
//! 3. **Hub API**: Task queue, monitoring, topology for the dashboard

mod admin;
mod auth;
mod hub;
mod pairing;
mod registry;
mod signaling;
mod topology;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Query, State, WebSocketUpgrade};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::Router;
use serde::Deserialize;
use tokio::sync::mpsc;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;
use tracing::{error, info, warn};

use admin::AdminAuth;
use auth::AuthState;
use hub::HubState;
use pairing::PairingState;
use registry::RegistryState;
use signaling::SignalingState;

/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    pub auth: Arc<AuthState>,
    pub admin: Arc<AdminAuth>,
    pub signaling: Arc<SignalingState>,
    pub registry: Arc<RegistryState>,
    pub hub: Arc<HubState>,
    pub pairing: Arc<PairingState>,
}

impl AppState {
    fn new(signaling_secret: &str, admin: AdminAuth) -> Self {
        Self {
            auth: Arc::new(AuthState::new(signaling_secret)),
            admin: Arc::new(admin),
            signaling: Arc::new(SignalingState::new()),
            registry: Arc::new(RegistryState::new()),
            hub: Arc::new(HubState::new()),
            pairing: Arc::new(PairingState::new()),
        }
    }
}

/// Read a duration in seconds from an env var, falling back to `default_secs`.
fn env_secs(name: &str, default_secs: u64) -> Duration {
    let secs = std::env::var(name)
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(default_secs);
    Duration::from_secs(secs)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8080);

    // JWTs are only ever verified by this process, so an unset secret can be
    // random per run: sidecars fetch a fresh JWT on every (re)connect.
    let signaling_secret = match std::env::var("SIGNALING_SECRET") {
        Ok(s) if !s.trim().is_empty() => s,
        _ => {
            info!(
                "[AUTH] SIGNALING_SECRET not set; using a random JWT signing secret for this run"
            );
            admin::generate_secret("")
        }
    };

    let (admin_auth, generated) = AdminAuth::from_env();
    if generated {
        warn!(
            "[AUTH] {} is not set. Generated an admin token for this run:\n\n    {}\n\n    \
             Hub: http://localhost:{}/#admin_token={}\n    \
             API: send it in the {} header. Set {} to keep it across restarts.\n",
            admin::ADMIN_TOKEN_ENV,
            admin_auth.token(),
            port,
            admin_auth.token(),
            admin::ADMIN_TOKEN_HEADER,
            admin::ADMIN_TOKEN_ENV,
        );
    } else {
        info!("[AUTH] admin token loaded from {}", admin::ADMIN_TOKEN_ENV);
        if admin_auth.token().len() < 16 {
            warn!(
                "[AUTH] {} is shorter than 16 characters",
                admin::ADMIN_TOKEN_ENV
            );
        }
    }

    let origins = admin::allowed_origins_from_env();
    info!(
        "[CORS] allowed origins: {}",
        if origins.is_empty() {
            "(none)".to_string()
        } else {
            origins
                .iter()
                .filter_map(|o| o.to_str().ok())
                .collect::<Vec<_>>()
                .join(", ")
        }
    );

    let state = AppState::new(&signaling_secret, admin_auth);

    // Spawn background tasks (G3: each loop also evicts old entries)
    let task_retention = env_secs(
        "REGISTRY_TASK_RETENTION_SECS",
        hub::DEFAULT_TASK_RETENTION_SECS,
    );
    let agent_eviction = env_secs(
        "REGISTRY_AGENT_EVICTION_SECS",
        registry::DEFAULT_AGENT_EVICTION_SECS,
    );
    let onboarding_retention = env_secs(
        "REGISTRY_ONBOARDING_RETENTION_SECS",
        pairing::DEFAULT_ONBOARDING_RETENTION_SECS,
    );

    let reg = state.registry.clone();
    tokio::spawn(async move { reg.health_check_loop(agent_eviction).await });

    let hub = state.hub.clone();
    tokio::spawn(async move { hub.expire_tasks_loop(task_retention).await });

    let pairing = state.pairing.clone();
    tokio::spawn(async move { pairing.cleanup_loop(onboarding_retention).await });

    let app = build_router(state, admin::cors_layer(origins));

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    info!("registry listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;

    Ok(())
}

/// All routes. Auth is enforced per handler by the `admin::RequireAdmin` and
/// `admin::RequireCaller` extractors; the comments below say which applies.
fn build_router(state: AppState, cors: CorsLayer) -> Router {
    Router::new()
        // Auth (API key or device token in headers)
        .route("/api/token", post(auth::exchange_token))
        // Signaling (JWT in query)
        .route("/ws", get(ws_upgrade))
        // Registry — GET open, writes need a caller credential
        .route("/api/registry/agents", get(registry::list_agents))
        .route("/api/registry/agents", post(registry::register_agent))
        .route(
            "/api/registry/agents/{agent_id}",
            get(registry::get_agent).delete(registry::delete_agent),
        )
        .route("/api/registry/route", get(registry::route_by_skill))
        // Hub — tasks: GET open, writes need a caller credential
        .route("/api/hub/tasks", post(hub::submit_task))
        .route("/api/hub/tasks/all", get(hub::list_tasks))
        .route("/api/hub/tasks/{task_id}", get(hub::get_task))
        .route("/api/hub/tasks/{task_id}", post(hub::update_task))
        // Hub — monitoring
        .route("/api/hub/heartbeat", post(registry::heartbeat)) // caller credential
        .route("/api/hub/network/topology", get(topology::network_topology))
        // Pairing + approval
        .route(
            "/api/pairing/generate-code",
            post(pairing::generate_code_handler), // admin or API key
        )
        .route("/api/pairing/pair", post(pairing::pair_handler)) // invite code
        .route(
            "/api/pairing/{id}/status",
            get(pairing::status_handler), // x-pairing-secret from /pair
        )
        .route("/api/pairing/pending", get(pairing::list_pending_handler)) // admin
        .route("/api/pairing/all", get(pairing::list_all_handler)) // admin
        .route("/api/pairing/{id}/approve", post(pairing::approve_handler)) // admin
        .route("/api/pairing/{id}/reject", post(pairing::reject_handler)) // admin
        .route("/api/pairing/{id}/revoke", post(pairing::revoke_handler)) // admin
        // ICE config (STUN/TURN)
        .route("/api/config", get(auth::ice_config))
        // Static files (hub dashboard + web client)
        .fallback_service(
            ServeDir::new(std::env::var("HUB_DIST_DIR").unwrap_or_else(|_| "hub/dist".to_string()))
                .append_index_html_on_directories(true),
        )
        .layer(cors)
        .with_state(state)
}

/// WebSocket query parameters.
#[derive(Deserialize)]
struct WsParams {
    token: String,
}

/// WebSocket upgrade handler — validates JWT before upgrade.
async fn ws_upgrade(
    ws: WebSocketUpgrade,
    Query(params): Query<WsParams>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    // Validate JWT
    let claims = match state.auth.validate_token(&params.token) {
        Ok(c) => c,
        Err(e) => {
            error!("[WS] invalid token: {}", e);
            return StatusCode::UNAUTHORIZED.into_response();
        }
    };

    let peer_id = claims.sub.clone();
    info!("[WS] upgrade for peer_id={}", peer_id);

    ws.on_upgrade(move |socket| handle_ws(socket, peer_id, state))
        .into_response()
}

/// Handle a WebSocket connection — register peer and relay signaling messages.
async fn handle_ws(mut socket: WebSocket, peer_id: String, state: AppState) {
    // Create a channel for sending messages to this peer
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();

    // Register this peer's sender
    state.signaling.add_peer(&peer_id, tx);
    info!("[WS] peer connected: {}", peer_id);

    loop {
        tokio::select! {
            // Outbound: forward queued messages to WebSocket
            Some(msg) = rx.recv() => {
                if socket.send(Message::Text(msg.into())).await.is_err() {
                    break;
                }
            }
            // Inbound: process incoming WebSocket messages
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        let text_str: &str = text.as_ref();
                        if let Ok(sm) = serde_json::from_str::<signaling::SignalingMessage>(text_str) {
                            if sm.peer_id != peer_id {
                                error!("[WS] peer_id mismatch: expected={}, got={}", peer_id, sm.peer_id);
                                continue;
                            }
                            let approved = state.pairing.approved_peer_ids();
                            let legacy = state.auth.api_key_peer_ids();
                            state.signaling.handle_message(sm, &approved, &legacy);
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
        }
    }

    // Cleanup
    state.signaling.remove_peer(&peer_id);
    info!("[WS] peer disconnected: {}", peer_id);
}

#[cfg(test)]
mod tests {
    //! Router-level tests: auth guards and CORS as a client sees them.

    use super::*;
    use axum::body::Body;
    use axum::extract::connect_info::MockConnectInfo;
    use axum::http::{header, Method, Request};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    const ADMIN: &str = "adm_test_token";

    fn app() -> (Router, AppState) {
        let state = AppState::new("test-secret", AdminAuth::new(ADMIN));
        let cors = admin::cors_layer(admin::parse_origins("http://localhost:5174"));
        let router = build_router(state.clone(), cors)
            .layer(MockConnectInfo(SocketAddr::from(([127, 0, 0, 1], 5555))));
        (router, state)
    }

    fn req(method: Method, uri: &str, headers: &[(&str, &str)], body: &str) -> Request<Body> {
        let mut b = Request::builder().method(method).uri(uri);
        for (k, v) in headers {
            b = b.header(*k, *v);
        }
        if !body.is_empty() {
            b = b.header(header::CONTENT_TYPE, "application/json");
        }
        b.body(Body::from(body.to_string())).unwrap()
    }

    async fn call(router: &Router, r: Request<Body>) -> (StatusCode, serde_json::Value) {
        let resp = router.clone().oneshot(r).await.unwrap();
        let status = resp.status();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
        (status, json)
    }

    /// Redeem a fresh invite code; returns the /pair response.
    async fn pair_device(router: &Router) -> serde_json::Value {
        let (s, code) = call(
            router,
            req(
                Method::POST,
                "/api/pairing/generate-code",
                &[("x-admin-token", ADMIN)],
                "",
            ),
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        let body = serde_json::json!({ "code": code["code"], "agent_name": "dev" }).to_string();
        let (s, paired) = call(router, req(Method::POST, "/api/pairing/pair", &[], &body)).await;
        assert_eq!(s, StatusCode::OK);
        paired
    }

    #[tokio::test]
    async fn admin_routes_reject_missing_or_wrong_token() {
        let (router, _) = app();
        let paired = pair_device(&router).await;
        let id = paired["id"].as_str().unwrap();
        let routes = [
            (Method::GET, "/api/pairing/pending".to_string()),
            (Method::GET, "/api/pairing/all".to_string()),
            (Method::POST, format!("/api/pairing/{id}/approve")),
            (Method::POST, format!("/api/pairing/{id}/reject")),
            (Method::POST, format!("/api/pairing/{id}/revoke")),
        ];
        for (m, uri) in &routes {
            for hdrs in [vec![], vec![("x-admin-token", "adm_wrong")]] {
                let (s, _) = call(&router, req(m.clone(), uri, &hdrs, "")).await;
                assert_eq!(s, StatusCode::UNAUTHORIZED, "{m} {uri} {hdrs:?}");
            }
            // An agent API key is not an admin credential
            let (s, _) = call(
                &router,
                req(m.clone(), uri, &[("x-api-key", "ak_dev_001")], ""),
            )
            .await;
            assert_eq!(s, StatusCode::UNAUTHORIZED, "{m} {uri} with api key");
        }
    }

    #[tokio::test]
    async fn pairing_flow_works_with_admin_token() {
        let (router, state) = app();
        let paired = pair_device(&router).await;
        let id = paired["id"].as_str().unwrap();
        let secret = paired["pairing_secret"].as_str().unwrap();
        let admin = [("x-admin-token", ADMIN)];

        let (s, pending) = call(
            &router,
            req(Method::GET, "/api/pairing/pending", &admin, ""),
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(pending.as_array().unwrap().len(), 1);

        // Before approval: the device sees pending, no token
        let status_uri = format!("/api/pairing/{id}/status");
        let (s, st) = call(
            &router,
            req(
                Method::GET,
                &status_uri,
                &[("x-pairing-secret", secret)],
                "",
            ),
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(st["status"], "pending_approval");
        assert!(st["device_token"].is_null());

        let (s, approved) = call(
            &router,
            req(
                Method::POST,
                &format!("/api/pairing/{id}/approve"),
                &admin,
                "",
            ),
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        let token = approved["device_token"].as_str().unwrap();

        // After approval: the device fetches its token with its secret only
        let (s, _) = call(
            &router,
            req(
                Method::GET,
                &status_uri,
                &[("x-pairing-secret", "ps_nope")],
                "",
            ),
        )
        .await;
        assert_eq!(s, StatusCode::NOT_FOUND);
        let (_, st) = call(
            &router,
            req(
                Method::GET,
                &status_uri,
                &[("x-pairing-secret", secret)],
                "",
            ),
        )
        .await;
        assert_eq!(st["device_token"], token);

        // The device token now gets a JWT, and the peer counts as approved
        let (s, jwt) = call(
            &router,
            req(Method::POST, "/api/token", &[("x-device-token", token)], ""),
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(jwt["peer_id"], paired["peer_id"]);
        assert!(state
            .pairing
            .approved_peer_ids()
            .contains(paired["peer_id"].as_str().unwrap()));

        let (s, _) = call(
            &router,
            req(
                Method::POST,
                &format!("/api/pairing/{id}/revoke"),
                &admin,
                "",
            ),
        )
        .await;
        assert_eq!(s, StatusCode::OK);
    }

    #[tokio::test]
    async fn generate_code_needs_admin_or_api_key() {
        let (router, _) = app();
        let uri = "/api/pairing/generate-code";
        let (s, _) = call(&router, req(Method::POST, uri, &[], "")).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
        let (s, _) = call(
            &router,
            req(Method::POST, uri, &[("x-api-key", "ak_dev_001")], ""),
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        let (s, _) = call(
            &router,
            req(Method::POST, uri, &[("x-admin-token", ADMIN)], ""),
        )
        .await;
        assert_eq!(s, StatusCode::OK);
    }

    #[tokio::test]
    async fn agent_writes_need_a_caller_credential() {
        let (router, state) = app();
        let agent = r#"{"agent_id":"a1","hostname":"h"}"#;
        let hb = r#"{"agent_id":"a1"}"#;
        let task = r#"{"skill":"search"}"#;

        // Anonymous writes are refused
        for (m, uri, body) in [
            (Method::POST, "/api/registry/agents", agent),
            (Method::POST, "/api/hub/heartbeat", hb),
            (Method::POST, "/api/hub/tasks", task),
            (
                Method::POST,
                "/api/hub/tasks/t1",
                r#"{"state":"completed"}"#,
            ),
            (Method::DELETE, "/api/registry/agents/a1", ""),
        ] {
            let (s, _) = call(&router, req(m.clone(), uri, &[], body)).await;
            assert_eq!(s, StatusCode::UNAUTHORIZED, "{m} {uri}");
        }
        assert!(state.registry.list().is_empty());

        // The agent's API key keeps working (runner sends x-api-key)
        let key = [("x-api-key", "ak_dev_001")];
        let (s, _) = call(
            &router,
            req(Method::POST, "/api/registry/agents", &key, agent),
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        let (s, _) = call(&router, req(Method::POST, "/api/hub/heartbeat", &key, hb)).await;
        assert_eq!(s, StatusCode::OK);
        let (s, _) = call(&router, req(Method::POST, "/api/hub/tasks", &key, task)).await;
        assert_eq!(s, StatusCode::OK);

        // Reads stay open
        let (s, agents) = call(&router, req(Method::GET, "/api/registry/agents", &[], "")).await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(agents.as_array().unwrap().len(), 1);

        // The hub deletes with the admin token
        let (s, _) = call(
            &router,
            req(
                Method::DELETE,
                "/api/registry/agents/a1",
                &[("x-admin-token", ADMIN)],
                "",
            ),
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        assert!(state.registry.list().is_empty());
    }

    #[tokio::test]
    async fn cors_allows_listed_origin_only() {
        let (router, _) = app();
        let preflight = |origin: &str| {
            Request::builder()
                .method(Method::OPTIONS)
                .uri("/api/pairing/pending")
                .header(header::ORIGIN, origin)
                .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
                .header(header::ACCESS_CONTROL_REQUEST_HEADERS, "x-admin-token")
                .body(Body::empty())
                .unwrap()
        };

        let ok = router
            .clone()
            .oneshot(preflight("http://localhost:5174"))
            .await
            .unwrap();
        assert_eq!(
            ok.headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .unwrap(),
            "http://localhost:5174"
        );
        let allowed_headers = ok
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_HEADERS)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        assert!(allowed_headers.contains("x-admin-token"));

        let evil = router
            .clone()
            .oneshot(preflight("https://evil.example"))
            .await
            .unwrap();
        assert!(evil
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .is_none());

        // A simple cross-origin GET gets no CORS grant either
        let get = Request::builder()
            .uri("/api/registry/agents")
            .header(header::ORIGIN, "https://evil.example")
            .body(Body::empty())
            .unwrap();
        let resp = router.clone().oneshot(get).await.unwrap();
        assert!(resp
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .is_none());
    }

    #[test]
    fn env_secs_parses_and_falls_back() {
        assert_eq!(
            env_secs("CHATIXIA_TEST_UNSET_VAR_XYZ", 42),
            Duration::from_secs(42)
        );
    }
}
