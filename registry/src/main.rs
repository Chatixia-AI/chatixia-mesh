//! Chatixia Registry — signaling server + agent registry + hub API.
//!
//! Combines three roles:
//! 1. **Signaling**: WebSocket relay for WebRTC SDP offers/answers and ICE candidates
//! 2. **Registry**: Agent discovery — tracks who's online, what skills they have
//! 3. **Hub API**: Task queue, monitoring, topology for the dashboard

mod auth;
mod hub;
mod pairing;
mod registry;
mod signaling;
mod topology;

use std::net::SocketAddr;
use std::sync::Arc;

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
use tracing::{error, info};

// futures-util used by sidecar, not directly by registry but kept for potential future use

use auth::AuthState;
use hub::HubState;
use pairing::PairingState;
use registry::RegistryState;
use signaling::SignalingState;

/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    pub auth: Arc<AuthState>,
    pub signaling: Arc<SignalingState>,
    pub registry: Arc<RegistryState>,
    pub hub: Arc<HubState>,
    pub pairing: Arc<PairingState>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let signaling_secret =
        std::env::var("SIGNALING_SECRET").unwrap_or_else(|_| "dev-secret-change-me".into());

    let state = AppState {
        auth: Arc::new(AuthState::new(&signaling_secret)),
        signaling: Arc::new(SignalingState::new()),
        registry: Arc::new(RegistryState::new()),
        hub: Arc::new(HubState::new()),
        pairing: Arc::new(PairingState::new()),
    };

    // Spawn background tasks
    let reg = state.registry.clone();
    tokio::spawn(async move { reg.health_check_loop().await });

    let hub = state.hub.clone();
    tokio::spawn(async move { hub.expire_tasks_loop().await });

    let pairing = state.pairing.clone();
    tokio::spawn(async move { pairing.cleanup_loop().await });

    let app = Router::new()
        // Auth
        .route("/api/token", post(auth::exchange_token))
        // Signaling
        .route("/ws", get(ws_upgrade))
        // Registry
        .route("/api/registry/agents", get(registry::list_agents))
        .route("/api/registry/agents", post(registry::register_agent))
        .route("/api/registry/agents/{agent_id}", get(registry::get_agent).delete(registry::delete_agent))
        .route("/api/registry/route", get(registry::route_by_skill))
        // Hub — tasks
        .route("/api/hub/tasks", post(hub::submit_task))
        .route("/api/hub/tasks/all", get(hub::list_tasks))
        .route("/api/hub/tasks/{task_id}", get(hub::get_task))
        .route("/api/hub/tasks/{task_id}", post(hub::update_task))
        // Hub — monitoring
        .route("/api/hub/heartbeat", post(registry::heartbeat))
        .route("/api/hub/network/topology", get(topology::network_topology))
        // Pairing + approval
        .route("/api/pairing/generate-code", post(pairing::generate_code_handler))
        .route("/api/pairing/pair", post(pairing::pair_handler))
        .route("/api/pairing/pending", get(pairing::list_pending_handler))
        .route("/api/pairing/all", get(pairing::list_all_handler))
        .route("/api/pairing/{id}/approve", post(pairing::approve_handler))
        .route("/api/pairing/{id}/reject", post(pairing::reject_handler))
        .route("/api/pairing/{id}/revoke", post(pairing::revoke_handler))
        // ICE config (STUN/TURN)
        .route("/api/config", get(auth::ice_config))
        // Static files (hub dashboard + web client)
        .fallback_service(ServeDir::new("hub/dist").append_index_html_on_directories(true))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], 8080));
    info!("registry listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await?;

    Ok(())
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
