//! IPC server — Unix domain socket bridge between Rust sidecar and Python agent.
//!
//! Protocol: JSON lines (one JSON object per line, newline-delimited).
//!
//! Agent → Sidecar:
//!   {"type": "send", "payload": {"target_peer": "peer-abc", "message": {...}}}
//!   {"type": "broadcast", "payload": {"message": {...}}}
//!   {"type": "list_peers", "payload": {}}
//!   {"type": "connect", "payload": {"peer_id": "peer-abc"}}   (also accepts "target_peer_id")
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
use crate::protocol::{ipc_types, IpcMessage, MeshMessage};
use crate::webrtc_peer;

/// Start the IPC server on a Unix domain socket.
pub async fn serve(
    socket_path: &str,
    mut to_agent_rx: mpsc::UnboundedReceiver<IpcMessage>,
    mesh: Arc<MeshManager>,
    to_agent_tx: mpsc::UnboundedSender<IpcMessage>,
) -> Result<()> {
    // Remove old socket file if it exists
    let _ = tokio::fs::remove_file(socket_path).await;

    let listener = UnixListener::bind(socket_path)?;
    info!("[IPC] listening on {}", socket_path);

    // One agent per sidecar, but the agent may disconnect and reconnect:
    // serve connections one at a time, forever. The sidecar→agent receiver
    // lives here so queued events survive across reconnects.
    loop {
        let (stream, _) = listener.accept().await?;
        info!("[IPC] agent connected");

        let (reader, mut writer) = stream.into_split();
        let mut lines = BufReader::new(reader).lines();

        loop {
            tokio::select! {
                // Agent → sidecar commands
                line = lines.next_line() => match line {
                    Ok(Some(line)) => {
                        let trimmed = line.trim();
                        if trimmed.is_empty() {
                            continue;
                        }
                        match serde_json::from_str::<IpcMessage>(trimmed) {
                            Ok(msg) => handle_agent_command(msg, &mesh, &to_agent_tx).await,
                            Err(e) => warn!("[IPC] failed to parse: {}", e),
                        }
                    }
                    Ok(None) => {
                        info!("[IPC] agent disconnected");
                        break;
                    }
                    Err(e) => {
                        error!("[IPC] read error: {}", e);
                        break;
                    }
                },
                // Sidecar → agent events
                msg = to_agent_rx.recv() => match msg {
                    Some(msg) => {
                        let mut line = serde_json::to_string(&msg).unwrap();
                        line.push('\n');
                        if let Err(e) = writer.write_all(line.as_bytes()).await {
                            error!("[IPC] write error: {}", e);
                            break;
                        }
                    }
                    None => {
                        // All senders dropped — nothing more to deliver, ever.
                        info!("[IPC] event channel closed, shutting down");
                        return Ok(());
                    }
                },
            }
        }

        info!("[IPC] waiting for agent to reconnect");
    }
}

/// Handle a command from the Python agent.
async fn handle_agent_command(
    msg: IpcMessage,
    mesh: &Arc<MeshManager>,
    to_agent_tx: &mpsc::UnboundedSender<IpcMessage>,
) {
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
            let peers = mesh.connected_peers();
            info!("[IPC] list_peers: {:?}", peers);
            let _ = to_agent_tx.send(IpcMessage {
                msg_type: ipc_types::PEER_LIST.into(),
                payload: serde_json::json!({ "peers": peers }),
            });
        }
        ipc_types::CONNECT => {
            // Initiate a WebRTC offer to a specific peer
            let target = msg
                .payload
                .get("peer_id")
                .or_else(|| msg.payload.get("target_peer_id"))
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_string();
            if target.is_empty() || target == mesh.local_peer_id {
                warn!("[IPC] connect: invalid target peer_id {:?}", target);
                return;
            }
            if mesh.is_connected(&target) {
                info!("[IPC] connect: already connected to {}", target);
                return;
            }
            let Some(sig_tx) = mesh.signaling_tx() else {
                warn!(
                    "[IPC] connect to {} failed: signaling not connected",
                    target
                );
                return;
            };
            info!("[IPC] connect: initiating connection to {}", target);
            let local_id = mesh.local_peer_id.clone();
            let mesh = mesh.clone();
            let to_agent = to_agent_tx.clone();
            tokio::spawn(async move {
                if let Err(e) =
                    webrtc_peer::initiate_connection(&local_id, &target, sig_tx, mesh, to_agent)
                        .await
                {
                    error!("[IPC] failed to initiate connection to {}: {}", target, e);
                }
            });
        }
        other => {
            warn!("[IPC] unknown command: {}", other);
        }
    }
}
