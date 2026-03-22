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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_msg(msg_type: &str, peer_id: &str, target_id: Option<&str>) -> SignalingMessage {
        SignalingMessage {
            msg_type: msg_type.to_string(),
            peer_id: peer_id.to_string(),
            target_id: target_id.map(String::from),
            payload: serde_json::json!({}),
        }
    }

    #[test]
    fn test_add_and_remove_peer() {
        let state = SignalingState::new();
        let (tx, _rx) = mpsc::unbounded_channel();
        state.add_peer("p1", tx);
        assert_eq!(state.connected_peers().len(), 1);
        state.remove_peer("p1");
        assert!(state.connected_peers().is_empty());
    }

    #[test]
    fn test_connected_peers() {
        let state = SignalingState::new();
        let (tx1, _rx1) = mpsc::unbounded_channel();
        let (tx2, _rx2) = mpsc::unbounded_channel();
        state.add_peer("p1", tx1);
        state.add_peer("p2", tx2);
        let mut peers = state.connected_peers();
        peers.sort();
        assert_eq!(peers, vec!["p1", "p2"]);
    }

    #[test]
    fn test_handle_register_authorized() {
        let state = SignalingState::new();
        let (tx1, mut rx1) = mpsc::unbounded_channel();
        let (tx2, _rx2) = mpsc::unbounded_channel();
        state.add_peer("p1", tx1);
        state.add_peer("p2", tx2);

        let approved: HashSet<String> = ["p1", "p2"].iter().map(|s| s.to_string()).collect();
        let legacy = HashSet::new();

        state.handle_message(make_msg("register", "p1", None), &approved, &legacy);

        let response = rx1.try_recv().unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&response).unwrap();
        assert_eq!(parsed["type"], "peer_list");
        let peers = parsed["payload"]["peers"].as_array().unwrap();
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0], "p2");
    }

    #[test]
    fn test_handle_register_unauthorized() {
        let state = SignalingState::new();
        let (tx1, mut rx1) = mpsc::unbounded_channel();
        state.add_peer("p1", tx1);

        let approved = HashSet::new();
        let legacy = HashSet::new();

        state.handle_message(make_msg("register", "p1", None), &approved, &legacy);

        let response = rx1.try_recv().unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&response).unwrap();
        let peers = parsed["payload"]["peers"].as_array().unwrap();
        assert!(peers.is_empty());
    }

    #[test]
    fn test_handle_offer_relayed() {
        let state = SignalingState::new();
        let (tx1, _rx1) = mpsc::unbounded_channel();
        let (tx2, mut rx2) = mpsc::unbounded_channel();
        state.add_peer("p1", tx1);
        state.add_peer("p2", tx2);

        let approved = HashSet::new();
        let legacy = HashSet::new();

        state.handle_message(make_msg("offer", "p1", Some("p2")), &approved, &legacy);

        let relayed = rx2.try_recv().unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&relayed).unwrap();
        assert_eq!(parsed["type"], "offer");
        assert_eq!(parsed["peer_id"], "p1");
    }

    #[test]
    fn test_handle_offer_missing_target() {
        let state = SignalingState::new();
        let (tx, _rx) = mpsc::unbounded_channel();
        state.add_peer("p1", tx);
        // No target_id — should not panic
        state.handle_message(
            make_msg("offer", "p1", None),
            &HashSet::new(),
            &HashSet::new(),
        );
    }

    #[test]
    fn test_handle_offer_unknown_target() {
        let state = SignalingState::new();
        let (tx, _rx) = mpsc::unbounded_channel();
        state.add_peer("p1", tx);
        // Target not connected — should not panic
        state.handle_message(
            make_msg("offer", "p1", Some("ghost")),
            &HashSet::new(),
            &HashSet::new(),
        );
    }

    #[test]
    fn test_handle_heartbeat() {
        let state = SignalingState::new();
        // No peers, just ensure no panic
        state.handle_message(
            make_msg("heartbeat", "p1", None),
            &HashSet::new(),
            &HashSet::new(),
        );
    }

    #[test]
    fn test_handle_unknown_type() {
        let state = SignalingState::new();
        state.handle_message(
            make_msg("foobar", "p1", None),
            &HashSet::new(),
            &HashSet::new(),
        );
    }
}
