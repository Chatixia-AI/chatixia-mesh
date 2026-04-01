//! WebRTC peer connection management — create offers, handle offers, wire DataChannels.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use base64::{engine::general_purpose, Engine};
use hmac::{Hmac, Mac};
use sha1::Sha1;
use tokio::sync::mpsc;
use tracing::{info, warn};
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::MediaEngine;
use webrtc::api::APIBuilder;
use webrtc::data_channel::data_channel_message::DataChannelMessage;
use webrtc::data_channel::RTCDataChannel;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::interceptor::registry::Registry;
use webrtc::peer_connection::policy::ice_transport_policy::RTCIceTransportPolicy;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::ice_transport::ice_connection_state::RTCIceConnectionState;
use webrtc::ice_transport::ice_gatherer_state::RTCIceGathererState;
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::peer_connection::RTCPeerConnection;

use crate::mesh::MeshManager;
use crate::protocol::{ipc_types, IpcMessage, MeshMessage, SignalingMessage};

type HmacSha1 = Hmac<Sha1>;

/// Build ICE servers from environment.
fn ice_servers_from_env() -> Vec<RTCIceServer> {
    let mut servers = vec![RTCIceServer {
        urls: vec!["stun:stun.l.google.com:19302".into()],
        ..Default::default()
    }];

    if let Ok(turn_url) = std::env::var("TURN_URL") {
        let (username, credential) = if let Ok(secret) = std::env::var("TURN_SECRET") {
            generate_turn_credentials(&secret, 86400)
        } else {
            (
                std::env::var("TURN_USERNAME").unwrap_or_default(),
                std::env::var("TURN_PASSWORD").unwrap_or_default(),
            )
        };
        servers.push(RTCIceServer {
            urls: vec![turn_url],
            username,
            credential,
        });
    }

    servers
}

pub(crate) fn generate_turn_credentials(secret: &str, ttl_secs: u64) -> (String, String) {
    let expiry = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + ttl_secs;
    let username = format!("{}:mesh", expiry);
    let mut mac = HmacSha1::new_from_slice(secret.as_bytes()).expect("HMAC key");
    mac.update(username.as_bytes());
    let password = general_purpose::STANDARD.encode(mac.finalize().into_bytes());
    (username, password)
}

/// Create a new RTCPeerConnection.
async fn create_peer_connection() -> Result<Arc<RTCPeerConnection>> {
    let mut me = MediaEngine::default();
    me.register_default_codecs()?;

    let mut registry = Registry::new();
    registry = register_default_interceptors(registry, &mut me)?;

    let api = APIBuilder::new()
        .with_media_engine(me)
        .with_interceptor_registry(registry)
        .build();

    let ice_servers = ice_servers_from_env();
    for s in &ice_servers {
        info!("[ICE] configured server: {:?}", s.urls);
    }

    // ICE_TRANSPORT_POLICY=relay forces all traffic through TURN (useful for testing)
    let ice_transport_policy = match std::env::var("ICE_TRANSPORT_POLICY")
        .unwrap_or_default()
        .to_lowercase()
        .as_str()
    {
        "relay" => {
            info!("[ICE] transport policy: relay-only (forced via ICE_TRANSPORT_POLICY)");
            RTCIceTransportPolicy::Relay
        }
        _ => RTCIceTransportPolicy::All,
    };

    let config = RTCConfiguration {
        ice_servers,
        ice_transport_policy,
        ..Default::default()
    };

    Ok(Arc::new(api.new_peer_connection(config).await?))
}

/// Wire up ICE candidate forwarding and connection state tracking for a peer connection.
fn setup_ice_forwarding(
    pc: &Arc<RTCPeerConnection>,
    local_peer_id: &str,
    remote_peer_id: &str,
    sig_tx: mpsc::UnboundedSender<String>,
    to_agent_tx: mpsc::UnboundedSender<IpcMessage>,
    mesh: Arc<MeshManager>,
) {
    let pid = local_peer_id.to_string();
    let tid = remote_peer_id.to_string();
    pc.on_ice_candidate(Box::new(move |candidate| {
        let sig_tx = sig_tx.clone();
        let pid = pid.clone();
        let tid = tid.clone();
        Box::pin(async move {
            if let Some(c) = candidate {
                info!(
                    "[ICE] local candidate for {}: type={} proto={} addr={}:{}",
                    tid, c.typ, c.protocol, c.address, c.port
                );
                let init = c.to_json().unwrap();
                let msg = SignalingMessage {
                    msg_type: "ice_candidate".into(),
                    peer_id: pid,
                    target_id: Some(tid),
                    payload: serde_json::json!({
                        "candidate": init.candidate,
                        "sdpMid": init.sdp_mid,
                        "sdpMLineIndex": init.sdp_mline_index,
                    }),
                };
                let _ = sig_tx.send(serde_json::to_string(&msg).unwrap());
            }
        })
    }));

    let rpid = remote_peer_id.to_string();
    pc.on_peer_connection_state_change(Box::new(move |state: RTCPeerConnectionState| {
        info!("[WEBRTC] {} connection state: {}", rpid, state);
        match state {
            RTCPeerConnectionState::Failed
            | RTCPeerConnectionState::Disconnected
            | RTCPeerConnectionState::Closed => {
                mesh.remove_peer(&rpid);
                let _ = to_agent_tx.send(IpcMessage {
                    msg_type: ipc_types::PEER_DISCONNECTED.into(),
                    payload: serde_json::json!({ "peer_id": rpid }),
                });
            }
            _ => {}
        }
        Box::pin(async {})
    }));

    // ICE connection state — tracks the actual ICE transport (checking → connected → completed)
    let rpid_ice = remote_peer_id.to_string();
    pc.on_ice_connection_state_change(Box::new(move |state: RTCIceConnectionState| {
        info!("[ICE] {} ice state: {}", rpid_ice, state);
        Box::pin(async {})
    }));

    // ICE gathering state — tracks candidate gathering progress
    let rpid_gather = remote_peer_id.to_string();
    pc.on_ice_gathering_state_change(Box::new(move |state: RTCIceGathererState| {
        info!("[ICE] {} gathering state: {}", rpid_gather, state);
        Box::pin(async {})
    }));
}

/// Wire up DataChannel message handling — forwards messages to Python agent via IPC.
fn setup_datachannel_handler(
    dc: Arc<RTCDataChannel>,
    remote_peer_id: &str,
    _mesh: Arc<MeshManager>,
    to_agent_tx: mpsc::UnboundedSender<IpcMessage>,
) {
    let rpid = remote_peer_id.to_string();

    // Clone for on_open before on_message takes ownership
    let to_agent_for_open = to_agent_tx.clone();

    // Register on_message IMMEDIATELY (before on_open) to not miss early messages
    dc.on_message(Box::new(move |msg: DataChannelMessage| {
        let to_agent = to_agent_tx.clone();
        let from_peer = rpid.clone();
        Box::pin(async move {
            let text = String::from_utf8_lossy(&msg.data);
            match serde_json::from_str::<MeshMessage>(&text) {
                Ok(mesh_msg) => {
                    // Forward to Python agent via IPC
                    let ipc_msg = IpcMessage {
                        msg_type: ipc_types::MESSAGE.into(),
                        payload: serde_json::json!({
                            "from_peer": from_peer,
                            "message": mesh_msg,
                        }),
                    };
                    let _ = to_agent.send(ipc_msg);
                }
                Err(e) => {
                    warn!("[DC] failed to parse message from {}: {}", from_peer, e);
                }
            }
        })
    }));

    let rpid2 = remote_peer_id.to_string();
    let label = dc.label().to_owned();
    dc.on_open(Box::new(move || {
        info!("[DC] channel '{}' open with peer {}", label, rpid2);
        // Notify Python agent about new peer
        let _ = to_agent_for_open.send(IpcMessage {
            msg_type: ipc_types::PEER_CONNECTED.into(),
            payload: serde_json::json!({ "peer_id": rpid2 }),
        });
        Box::pin(async {})
    }));
}

/// Initiate a WebRTC connection to a remote peer (we are the offerer).
pub async fn initiate_connection(
    local_peer_id: &str,
    remote_peer_id: &str,
    sig_tx: mpsc::UnboundedSender<String>,
    mesh: Arc<MeshManager>,
    to_agent_tx: mpsc::UnboundedSender<IpcMessage>,
) -> Result<()> {
    let pc = create_peer_connection().await?;

    setup_ice_forwarding(
        &pc,
        local_peer_id,
        remote_peer_id,
        sig_tx.clone(),
        to_agent_tx.clone(),
        mesh.clone(),
    );

    // Create DataChannel
    let dc = pc.create_data_channel("mesh", None).await?;
    setup_datachannel_handler(dc.clone(), remote_peer_id, mesh.clone(), to_agent_tx);

    // Store peer connection and channel
    mesh.add_peer(remote_peer_id, pc.clone());
    mesh.set_channel(remote_peer_id, dc);

    // Create offer
    let offer = pc.create_offer(None).await?;
    let sdp = offer.sdp.clone();
    pc.set_local_description(offer).await?;

    // Send offer via signaling
    let msg = SignalingMessage {
        msg_type: "offer".into(),
        peer_id: local_peer_id.to_string(),
        target_id: Some(remote_peer_id.to_string()),
        payload: serde_json::json!({ "sdp": sdp }),
    };
    sig_tx.send(serde_json::to_string(&msg)?)?;

    info!("[WEBRTC] offer sent to {}", remote_peer_id);
    Ok(())
}

/// Handle an incoming offer from a remote peer (we are the answerer).
pub async fn handle_offer(
    local_peer_id: &str,
    remote_peer_id: &str,
    offer_sdp: &str,
    sig_tx: mpsc::UnboundedSender<String>,
    mesh: Arc<MeshManager>,
    to_agent_tx: mpsc::UnboundedSender<IpcMessage>,
) -> Result<()> {
    let pc = create_peer_connection().await?;

    setup_ice_forwarding(
        &pc,
        local_peer_id,
        remote_peer_id,
        sig_tx.clone(),
        to_agent_tx.clone(),
        mesh.clone(),
    );

    // Handle incoming data channels
    let rpid = remote_peer_id.to_string();
    let mesh_for_dc = mesh.clone();
    let to_agent = to_agent_tx.clone();
    pc.on_data_channel(Box::new(move |dc: Arc<RTCDataChannel>| {
        let rpid = rpid.clone();
        let mesh = mesh_for_dc.clone();
        let to_agent = to_agent.clone();

        // Set channel in mesh manager
        mesh.set_channel(&rpid, dc.clone());

        // Wire up message handling
        setup_datachannel_handler(dc, &rpid, mesh, to_agent);

        Box::pin(async {})
    }));

    // Set remote description (offer)
    let offer = RTCSessionDescription::offer(offer_sdp.to_string())?;
    pc.set_remote_description(offer).await?;

    // Store peer connection
    mesh.add_peer(remote_peer_id, pc.clone());

    // Create answer
    let answer = pc.create_answer(None).await?;
    let sdp = answer.sdp.clone();
    pc.set_local_description(answer).await?;

    // Send answer via signaling
    let msg = SignalingMessage {
        msg_type: "answer".into(),
        peer_id: local_peer_id.to_string(),
        target_id: Some(remote_peer_id.to_string()),
        payload: serde_json::json!({ "sdp": sdp }),
    };
    sig_tx.send(serde_json::to_string(&msg)?)?;

    info!("[WEBRTC] answer sent to {}", remote_peer_id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_turn_credentials_format() {
        let (username, password) = generate_turn_credentials("test-secret", 86400);
        // Username should be "{expiry}:mesh"
        assert!(username.ends_with(":mesh"), "username should end with ':mesh'");
        let expiry_str = username.split(':').next().unwrap();
        let expiry: u64 = expiry_str.parse().expect("expiry should be a number");
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        // Expiry should be roughly now + 86400
        assert!(expiry > now + 86300);
        assert!(expiry < now + 86500);
        // Password should be base64-encoded (non-empty, valid base64)
        assert!(!password.is_empty());
        general_purpose::STANDARD
            .decode(&password)
            .expect("password should be valid base64");
    }

    #[test]
    fn test_generate_turn_credentials_different_secrets_differ() {
        let (_, pass1) = generate_turn_credentials("secret-1", 86400);
        let (_, pass2) = generate_turn_credentials("secret-2", 86400);
        assert_ne!(pass1, pass2, "different secrets should produce different passwords");
    }

    #[test]
    fn test_generate_turn_credentials_deterministic_for_same_input() {
        // Same secret + TTL within same second should produce the same result
        let (u1, p1) = generate_turn_credentials("fixed-secret", 86400);
        let (u2, p2) = generate_turn_credentials("fixed-secret", 86400);
        assert_eq!(u1, u2);
        assert_eq!(p1, p2);
    }

    #[test]
    fn test_generate_turn_credentials_short_ttl() {
        let (username, _) = generate_turn_credentials("s", 60);
        let expiry_str = username.split(':').next().unwrap();
        let expiry: u64 = expiry_str.parse().unwrap();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert!(expiry >= now + 59);
        assert!(expiry <= now + 61);
    }

    #[test]
    fn test_ice_servers_from_env_default() {
        // Clear TURN env vars to test default behavior
        std::env::remove_var("TURN_URL");
        std::env::remove_var("TURN_SECRET");
        std::env::remove_var("TURN_USERNAME");
        std::env::remove_var("TURN_PASSWORD");
        let servers = ice_servers_from_env();
        assert_eq!(servers.len(), 1, "should have only STUN when no TURN configured");
        assert!(servers[0].urls[0].starts_with("stun:"));
    }

    #[test]
    fn test_ice_servers_from_env_with_turn_secret() {
        std::env::set_var("TURN_URL", "turn:my-turn.example.com:3478");
        std::env::set_var("TURN_SECRET", "my-shared-secret");
        std::env::remove_var("TURN_USERNAME");
        std::env::remove_var("TURN_PASSWORD");
        let servers = ice_servers_from_env();
        assert_eq!(servers.len(), 2, "should have STUN + TURN");
        assert_eq!(servers[1].urls[0], "turn:my-turn.example.com:3478");
        assert!(servers[1].username.ends_with(":mesh"));
        assert!(!servers[1].credential.is_empty());
        // Cleanup
        std::env::remove_var("TURN_URL");
        std::env::remove_var("TURN_SECRET");
    }

    #[test]
    fn test_ice_servers_from_env_with_static_credentials() {
        std::env::set_var("TURN_URL", "turn:turn.local:3478");
        std::env::remove_var("TURN_SECRET");
        std::env::set_var("TURN_USERNAME", "user1");
        std::env::set_var("TURN_PASSWORD", "pass1");
        let servers = ice_servers_from_env();
        assert_eq!(servers.len(), 2);
        assert_eq!(servers[1].username, "user1");
        assert_eq!(servers[1].credential, "pass1");
        // Cleanup
        std::env::remove_var("TURN_URL");
        std::env::remove_var("TURN_USERNAME");
        std::env::remove_var("TURN_PASSWORD");
    }
}
