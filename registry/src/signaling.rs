//! WebSocket signaling relay for WebRTC — supports N:N mesh topology.

use std::collections::HashSet;

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::{error, info, warn};

/// Signaling message exchanged over WebSocket.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SignalingMessage {
    #[serde(rename = "type")]
    pub msg_type: String,
    pub peer_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_id: Option<String>,
    #[serde(default)]
    pub payload: serde_json::Value,
}

pub struct SignalingState {
    /// peer_id → sender channel for forwarding messages.
    peers: DashMap<String, mpsc::UnboundedSender<String>>,
}

impl SignalingState {
    pub fn new() -> Self {
        Self {
            peers: DashMap::new(),
        }
    }

    /// Register a peer's WebSocket sender.
    pub fn add_peer(&self, peer_id: &str, tx: mpsc::UnboundedSender<String>) {
        self.peers.insert(peer_id.to_string(), tx);
        info!("[SIG] peer registered: {} (total: {})", peer_id, self.peers.len());
    }

    /// Remove a peer.
    pub fn remove_peer(&self, peer_id: &str) {
        self.peers.remove(peer_id);
        info!("[SIG] peer removed: {} (total: {})", peer_id, self.peers.len());
    }

    /// Get list of all connected peer IDs.
    pub fn connected_peers(&self) -> Vec<String> {
        self.peers.iter().map(|e| e.key().clone()).collect()
    }

    /// Handle an incoming signaling message — relay to target or broadcast.
    ///
    /// `approved_peers` — peer_ids approved through the pairing system.
    /// `legacy_peers` — peer_ids with static API keys (auto-approved).
    /// Peers in either set are considered authorized; others get an empty peer_list.
    pub fn handle_message(
        &self,
        msg: SignalingMessage,
        approved_peers: &HashSet<String>,
        legacy_peers: &HashSet<String>,
    ) {
        let is_authorized =
            |pid: &str| approved_peers.contains(pid) || legacy_peers.contains(pid);

        match msg.msg_type.as_str() {
            "register" => {
                info!("[SIG] register from peer_id={}", msg.peer_id);
                // Only authorized peers see other authorized peers
                let peers: Vec<String> = if is_authorized(&msg.peer_id) {
                    self.connected_peers()
                        .into_iter()
                        .filter(|p| p != &msg.peer_id && is_authorized(p))
                        .collect()
                } else {
                    // Pending/unknown peer gets empty list (can't communicate yet)
                    vec![]
                };
                if let Some(sender) = self.peers.get(&msg.peer_id) {
                    let response = serde_json::json!({
                        "type": "peer_list",
                        "peer_id": "registry",
                        "payload": { "peers": peers }
                    });
                    let _ = sender.send(serde_json::to_string(&response).unwrap());
                }
            }
            "offer" | "answer" | "ice_candidate" => {
                // Relay to target peer
                if let Some(target_id) = &msg.target_id {
                    if let Some(sender) = self.peers.get(target_id) {
                        let json = serde_json::to_string(&msg).unwrap();
                        if sender.send(json).is_err() {
                            warn!("[SIG] failed to relay {} to {}", msg.msg_type, target_id);
                        } else {
                            info!(
                                "[SIG] relayed {} from {} → {}",
                                msg.msg_type, msg.peer_id, target_id
                            );
                        }
                    } else {
                        warn!("[SIG] target peer not found: {}", target_id);
                    }
                } else {
                    error!("[SIG] {} message without target_id", msg.msg_type);
                }
            }
            "heartbeat" => {
                // Keep-alive, no action needed
            }
            other => {
                warn!("[SIG] unknown message type: {}", other);
            }
        }
    }
}
