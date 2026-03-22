//! Chatixia Sidecar — WebRTC mesh peer with IPC bridge to Python agent.
//!
//! Each Python agent spawns one sidecar process. The sidecar:
//! 1. Connects to the registry via WebSocket for signaling
//! 2. Establishes WebRTC DataChannels with other sidecars (full mesh)
//! 3. Bridges messages between the DataChannel mesh and the Python agent via IPC

mod ipc;
mod mesh;
mod protocol;
mod signaling;
mod webrtc_peer;

use std::sync::Arc;

use anyhow::Result;
use tokio::sync::mpsc;
use tracing::{error, info};

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let signaling_url =
        std::env::var("SIGNALING_URL").unwrap_or_else(|_| "ws://localhost:8080/ws".into());
    let api_key = std::env::var("API_KEY").unwrap_or_else(|_| "ak_dev_001".into());
    let token_url =
        std::env::var("TOKEN_URL").unwrap_or_else(|_| "http://localhost:8080/api/token".into());
    let ipc_socket_path =
        std::env::var("IPC_SOCKET").unwrap_or_else(|_| "/tmp/chatixia-sidecar.sock".into());

    // Exchange API key for JWT
    let token = signaling::exchange_token(&token_url, &api_key).await?;
    info!("[MAIN] authenticated as peer_id={}", token.peer_id);

    // Create mesh manager
    let mesh = Arc::new(mesh::MeshManager::new(token.peer_id.clone()));

    // Channel for outbound signaling messages
    let (sig_tx, sig_rx) = mpsc::unbounded_channel::<String>();

    // Channel for messages from mesh → IPC (to Python agent)
    let (to_agent_tx, to_agent_rx) = mpsc::unbounded_channel::<protocol::IpcMessage>();

    // Start IPC server (Unix socket) — connects sidecar to Python agent
    let ipc_mesh = mesh.clone();
    let ipc_to_agent_tx = to_agent_tx.clone();
    let ipc_handle = tokio::spawn(async move {
        if let Err(e) = ipc::serve(&ipc_socket_path, to_agent_rx, ipc_mesh, ipc_to_agent_tx).await {
            error!("[IPC] server error: {}", e);
        }
    });

    // Connect to signaling server
    let ws_url = format!("{}?token={}", signaling_url, token.token);
    let mesh_for_sig = mesh.clone();
    let sig_handle = tokio::spawn(async move {
        if let Err(e) = signaling::run(
            &ws_url,
            &token.peer_id,
            sig_tx,
            sig_rx,
            mesh_for_sig,
            to_agent_tx,
        )
        .await
        {
            error!("[SIG] connection error: {}", e);
        }
    });

    // Send register message
    info!("[MAIN] sidecar ready, waiting for connections");

    tokio::select! {
        _ = ipc_handle => { error!("[MAIN] IPC server exited"); }
        _ = sig_handle => { error!("[MAIN] signaling connection exited"); }
    }

    Ok(())
}
