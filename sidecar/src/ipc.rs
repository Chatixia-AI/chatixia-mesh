//! IPC server — Unix domain socket bridge between Rust sidecar and Python agent.
//!
//! Protocol: JSON lines (one JSON object per line, newline-delimited).
//!
//! Agent → Sidecar:
//!   {"type": "send", "payload": {"target_peer": "peer-abc", "message": {...}}}
//!   {"type": "broadcast", "payload": {"message": {...}}}
//!   {"type": "list_peers", "payload": {}}
//!   {"type": "connect", "payload": {"target_peer_id": "peer-abc"}}
//!
//! Sidecar → Agent:
//!   {"type": "message", "payload": {"from_peer": "peer-abc", "message": {...}}}
//!   {"type": "peer_connected", "payload": {"peer_id": "peer-abc"}}
//!   {"type": "peer_disconnected", "payload": {"peer_id": "peer-abc"}}
//!   {"type": "peer_list", "payload": {"peers": ["peer-abc", "peer-def"]}}

use std::sync::Arc;

use anyhow::Result;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use crate::mesh::MeshManager;
use crate::protocol::{IpcMessage, MeshMessage, ipc_types};

/// Start the IPC server on a Unix domain socket.
pub async fn serve(
    socket_path: &str,
    mut to_agent_rx: mpsc::UnboundedReceiver<IpcMessage>,
    mesh: Arc<MeshManager>,
) -> Result<()> {
    // Remove old socket file if it exists
    let _ = tokio::fs::remove_file(socket_path).await;

    let listener = UnixListener::bind(socket_path)?;
    info!("[IPC] listening on {}", socket_path);

    // Accept a single connection (one agent per sidecar)
    let (stream, _) = listener.accept().await?;
    info!("[IPC] agent connected");

    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);

    // Task: forward sidecar→agent events
    let write_task = tokio::spawn(async move {
        while let Some(msg) = to_agent_rx.recv().await {
            let mut line = serde_json::to_string(&msg).unwrap();
            line.push('\n');
            if writer.write_all(line.as_bytes()).await.is_err() {
                break;
            }
        }
    });

    // Read agent→sidecar commands
    let mut line_buf = String::new();
    loop {
        line_buf.clear();
        match reader.read_line(&mut line_buf).await {
            Ok(0) => {
                info!("[IPC] agent disconnected");
                break;
            }
            Ok(_) => {
                let trimmed = line_buf.trim();
                if trimmed.is_empty() {
                    continue;
                }
                match serde_json::from_str::<IpcMessage>(trimmed) {
                    Ok(msg) => handle_agent_command(msg, &mesh).await,
                    Err(e) => warn!("[IPC] failed to parse: {}", e),
                }
            }
            Err(e) => {
                error!("[IPC] read error: {}", e);
                break;
            }
        }
    }

    write_task.abort();
    Ok(())
}

/// Handle a command from the Python agent.
async fn handle_agent_command(msg: IpcMessage, mesh: &Arc<MeshManager>) {
    match msg.msg_type.as_str() {
        ipc_types::SEND => {
            // Send to specific peer
            let target = msg
                .payload
                .get("target_peer")
                .and_then(|t| t.as_str())
                .unwrap_or("");
            if let Some(message) = msg.payload.get("message") {
                if let Ok(mesh_msg) = serde_json::from_value::<MeshMessage>(message.clone()) {
                    if let Err(e) = mesh.send_to(target, &mesh_msg).await {
                        warn!("[IPC] send to {} failed: {}", target, e);
                    }
                }
            }
        }
        ipc_types::BROADCAST => {
            // Broadcast to all peers
            if let Some(message) = msg.payload.get("message") {
                if let Ok(mesh_msg) = serde_json::from_value::<MeshMessage>(message.clone()) {
                    mesh.broadcast(&mesh_msg).await;
                }
            }
        }
        ipc_types::LIST_PEERS => {
            // List connected peers (response sent back via to_agent channel)
            let peers = mesh.connected_peers();
            info!("[IPC] list_peers: {:?}", peers);
            // Note: response goes through the to_agent_tx channel in mesh manager
        }
        other => {
            warn!("[IPC] unknown command: {}", other);
        }
    }
}
