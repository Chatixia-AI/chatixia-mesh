//! Mesh manager — tracks multiple WebRTC peer connections and DataChannels.

use std::sync::Arc;

use dashmap::DashMap;
use tokio::sync::mpsc;
use tracing::{error, info, warn};
use webrtc::data_channel::RTCDataChannel;
use webrtc::peer_connection::RTCPeerConnection;

use crate::protocol::MeshMessage;

/// A connected peer in the mesh.
pub struct MeshPeer {
    pub peer_id: String,
    pub pc: Arc<RTCPeerConnection>,
    pub dc: Option<Arc<RTCDataChannel>>,
}

/// Manages the full mesh of WebRTC connections.
pub struct MeshManager {
    /// Our peer ID.
    pub local_peer_id: String,
    /// peer_id → MeshPeer
    peers: DashMap<String, MeshPeer>,
    /// peer_id → DataChannel (shortcut for sending)
    channels: DashMap<String, Arc<RTCDataChannel>>,
}

impl MeshManager {
    pub fn new(local_peer_id: String) -> Self {
        Self {
            local_peer_id,
            peers: DashMap::new(),
            channels: DashMap::new(),
        }
    }

    /// Store a peer connection.
    pub fn add_peer(&self, peer_id: &str, pc: Arc<RTCPeerConnection>) {
        self.peers.insert(
            peer_id.to_string(),
            MeshPeer {
                peer_id: peer_id.to_string(),
                pc,
                dc: None,
            },
        );
        info!("[MESH] peer added: {} (total: {})", peer_id, self.peers.len());
    }

    /// Store a data channel for a peer.
    pub fn set_channel(&self, peer_id: &str, dc: Arc<RTCDataChannel>) {
        self.channels.insert(peer_id.to_string(), dc.clone());
        if let Some(mut peer) = self.peers.get_mut(peer_id) {
            peer.dc = Some(dc);
        }
        info!("[MESH] datachannel set for peer: {}", peer_id);
    }

    /// Remove a peer.
    pub fn remove_peer(&self, peer_id: &str) {
        self.peers.remove(peer_id);
        self.channels.remove(peer_id);
        info!("[MESH] peer removed: {} (total: {})", peer_id, self.peers.len());
    }

    /// Get the peer connection for a peer.
    pub fn get_pc(&self, peer_id: &str) -> Option<Arc<RTCPeerConnection>> {
        self.peers.get(peer_id).map(|p| p.pc.clone())
    }

    /// Send a message to a specific peer over their DataChannel.
    pub async fn send_to(&self, peer_id: &str, msg: &MeshMessage) -> anyhow::Result<()> {
        let dc = self
            .channels
            .get(peer_id)
            .ok_or_else(|| anyhow::anyhow!("no channel for peer: {}", peer_id))?;

        let json = serde_json::to_string(msg)?;
        dc.send_text(json).await?;
        Ok(())
    }

    /// Broadcast a message to all connected peers.
    pub async fn broadcast(&self, msg: &MeshMessage) {
        let json = match serde_json::to_string(msg) {
            Ok(j) => j,
            Err(e) => {
                error!("[MESH] failed to serialize broadcast: {}", e);
                return;
            }
        };

        for entry in self.channels.iter() {
            let peer_id = entry.key().clone();
            let dc = entry.value().clone();
            let json = json.clone();
            tokio::spawn(async move {
                if let Err(e) = dc.send_text(json).await {
                    warn!("[MESH] broadcast to {} failed: {}", peer_id, e);
                }
            });
        }
    }

    /// List all connected peer IDs.
    pub fn connected_peers(&self) -> Vec<String> {
        self.channels.iter().map(|e: dashmap::mapref::multiple::RefMulti<'_, String, Arc<RTCDataChannel>>| e.key().clone()).collect()
    }

    /// Check if we have a connection to a peer.
    pub fn is_connected(&self, peer_id: &str) -> bool {
        self.channels.contains_key(peer_id)
    }
}
