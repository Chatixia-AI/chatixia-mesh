//! Mesh manager — tracks multiple WebRTC peer connections and DataChannels.

use std::sync::Arc;

use dashmap::DashMap;
use tracing::{error, info, warn};
use webrtc::data_channel::RTCDataChannel;
use webrtc::peer_connection::RTCPeerConnection;

use crate::protocol::MeshMessage;

/// A connected peer in the mesh.
#[allow(dead_code)]
pub struct MeshPeer {
    pub peer_id: String,
    pub pc: Arc<RTCPeerConnection>,
    pub dc: Option<Arc<RTCDataChannel>>,
}

/// Manages the full mesh of WebRTC connections.
pub struct MeshManager {
    /// Our peer ID.
    #[allow(dead_code)]
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
        info!(
            "[MESH] peer added: {} (total: {})",
            peer_id,
            self.peers.len()
        );
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
        info!(
            "[MESH] peer removed: {} (total: {})",
            peer_id,
            self.peers.len()
        );
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
        self.channels
            .iter()
            .map(
                |e: dashmap::mapref::multiple::RefMulti<'_, String, Arc<RTCDataChannel>>| {
                    e.key().clone()
                },
            )
            .collect()
    }

    /// Remove all peers and close their connections.
    ///
    /// Called before signaling reconnect to ensure stale WebRTC connections
    /// are cleaned up and fresh ones can be established.
    pub async fn clear_all_peers(&self) {
        let peer_ids: Vec<String> = self.peers.iter().map(|e| e.key().clone()).collect();
        for pid in &peer_ids {
            if let Some((_, peer)) = self.peers.remove(pid) {
                let _ = peer.pc.close().await;
            }
            self.channels.remove(pid);
        }
        if !peer_ids.is_empty() {
            info!(
                "[MESH] cleared {} stale peers for reconnect",
                peer_ids.len()
            );
        }
    }

    /// Check if we have a connection to a peer.
    pub fn is_connected(&self, peer_id: &str) -> bool {
        self.channels.contains_key(peer_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_mesh_manager() {
        let mgr = MeshManager::new("local-1".into());
        assert_eq!(mgr.local_peer_id, "local-1");
        assert!(mgr.connected_peers().is_empty());
    }

    #[test]
    fn test_connected_peers_empty() {
        let mgr = MeshManager::new("local-2".into());
        let peers = mgr.connected_peers();
        assert!(peers.is_empty());
    }

    #[test]
    fn test_is_connected_empty() {
        let mgr = MeshManager::new("local-3".into());
        assert!(!mgr.is_connected("nonexistent"));
        assert!(!mgr.is_connected(""));
    }

    #[test]
    fn test_remove_nonexistent_peer() {
        let mgr = MeshManager::new("local-4".into());
        // Should not panic when removing a peer that was never added.
        mgr.remove_peer("ghost-peer");
        assert!(mgr.connected_peers().is_empty());
    }

    #[test]
    fn test_add_peer_and_get_pc() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let mgr = MeshManager::new("local-5".into());
            let api = webrtc::api::APIBuilder::new().build();
            let pc = std::sync::Arc::new(
                api.new_peer_connection(webrtc::peer_connection::configuration::RTCConfiguration::default())
                    .await
                    .unwrap(),
            );
            mgr.add_peer("remote-1", pc.clone());
            assert!(mgr.get_pc("remote-1").is_some());
            assert!(mgr.get_pc("nonexistent").is_none());
            // Peer was added but no channel — connected_peers should be empty
            assert!(mgr.connected_peers().is_empty());
            pc.close().await.unwrap();
        });
    }

    #[test]
    fn test_remove_peer_cleans_up() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let mgr = MeshManager::new("local-6".into());
            let api = webrtc::api::APIBuilder::new().build();
            let pc = std::sync::Arc::new(
                api.new_peer_connection(webrtc::peer_connection::configuration::RTCConfiguration::default())
                    .await
                    .unwrap(),
            );
            mgr.add_peer("remote-2", pc.clone());
            assert!(mgr.get_pc("remote-2").is_some());
            mgr.remove_peer("remote-2");
            assert!(mgr.get_pc("remote-2").is_none());
            pc.close().await.unwrap();
        });
    }

    #[test]
    fn test_set_channel_and_connected_peers() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let mgr = MeshManager::new("local-7".into());
            let api = webrtc::api::APIBuilder::new().build();
            let pc = std::sync::Arc::new(
                api.new_peer_connection(webrtc::peer_connection::configuration::RTCConfiguration::default())
                    .await
                    .unwrap(),
            );
            let dc = pc.create_data_channel("test", None).await.unwrap();
            mgr.add_peer("remote-3", pc.clone());
            mgr.set_channel("remote-3", dc);
            assert!(mgr.is_connected("remote-3"));
            assert_eq!(mgr.connected_peers(), vec!["remote-3".to_string()]);
            pc.close().await.unwrap();
        });
    }

    #[test]
    fn test_clear_all_peers() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let mgr = MeshManager::new("local-8".into());
            let api = webrtc::api::APIBuilder::new().build();
            let pc1 = std::sync::Arc::new(
                api.new_peer_connection(webrtc::peer_connection::configuration::RTCConfiguration::default())
                    .await
                    .unwrap(),
            );
            let pc2 = std::sync::Arc::new(
                api.new_peer_connection(webrtc::peer_connection::configuration::RTCConfiguration::default())
                    .await
                    .unwrap(),
            );
            let dc1 = pc1.create_data_channel("c1", None).await.unwrap();
            let dc2 = pc2.create_data_channel("c2", None).await.unwrap();
            mgr.add_peer("p1", pc1);
            mgr.add_peer("p2", pc2);
            mgr.set_channel("p1", dc1);
            mgr.set_channel("p2", dc2);
            assert_eq!(mgr.connected_peers().len(), 2);
            mgr.clear_all_peers().await;
            assert!(mgr.connected_peers().is_empty());
            assert!(mgr.get_pc("p1").is_none());
            assert!(mgr.get_pc("p2").is_none());
        });
    }

    #[test]
    fn test_multiple_peers_ordering() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let mgr = MeshManager::new("local-9".into());
            let api = webrtc::api::APIBuilder::new().build();
            for i in 0..5 {
                let pc = std::sync::Arc::new(
                    api.new_peer_connection(webrtc::peer_connection::configuration::RTCConfiguration::default())
                        .await
                        .unwrap(),
                );
                let dc = pc.create_data_channel("ch", None).await.unwrap();
                mgr.add_peer(&format!("peer-{}", i), pc);
                mgr.set_channel(&format!("peer-{}", i), dc);
            }
            let peers = mgr.connected_peers();
            assert_eq!(peers.len(), 5);
            for i in 0..5 {
                assert!(mgr.is_connected(&format!("peer-{}", i)));
            }
        });
    }
}
