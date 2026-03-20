//! WebRTC peer connection management — create offers, handle offers, wire DataChannels.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use base64::{engine::general_purpose, Engine};
use hmac::{Hmac, Mac};
use sha1::Sha1;
use tokio::sync::mpsc;
use tracing::{error, info, warn};
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::MediaEngine;
use webrtc::api::APIBuilder;
use webrtc::data_channel::data_channel_message::DataChannelMessage;
use webrtc::data_channel::RTCDataChannel;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::interceptor::registry::Registry;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::peer_connection::RTCPeerConnection;

use crate::mesh::MeshManager;
use crate::protocol::{IpcMessage, MeshMessage, SignalingMessage, ipc_types};

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
            ..Default::default()
        });
    }

    servers
}

fn generate_turn_credentials(secret: &str, ttl_secs: u64) -> (String, String) {
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

    let config = RTCConfiguration {
        ice_servers: ice_servers_from_env(),
        ..Default::default()
    };

    Ok(Arc::new(api.new_peer_connection(config).await?))
}

/// Wire up ICE candidate forwarding for a peer connection.
fn setup_ice_forwarding(
    pc: &Arc<RTCPeerConnection>,
    local_peer_id: &str,
    remote_peer_id: &str,
    sig_tx: mpsc::UnboundedSender<String>,
) {
    let pid = local_peer_id.to_string();
    let tid = remote_peer_id.to_string();
    pc.on_ice_candidate(Box::new(move |candidate| {
        let sig_tx = sig_tx.clone();
        let pid = pid.clone();
        let tid = tid.clone();
        Box::pin(async move {
            if let Some(c) = candidate {
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
        Box::pin(async {})
    }));
}

/// Wire up DataChannel message handling — forwards messages to Python agent via IPC.
fn setup_datachannel_handler(
    dc: Arc<RTCDataChannel>,
    remote_peer_id: &str,
    mesh: Arc<MeshManager>,
    to_agent_tx: mpsc::UnboundedSender<IpcMessage>,
) {
    let rpid = remote_peer_id.to_string();
    let mesh_for_msg = mesh.clone();

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

    setup_ice_forwarding(&pc, local_peer_id, remote_peer_id, sig_tx.clone());

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

    setup_ice_forwarding(&pc, local_peer_id, remote_peer_id, sig_tx.clone());

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
