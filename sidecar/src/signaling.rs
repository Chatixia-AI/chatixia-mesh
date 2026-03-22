//! Signaling client — connects to registry via WebSocket, handles SDP/ICE exchange.

use std::sync::Arc;

use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{error, info, warn};

use crate::mesh::MeshManager;
use crate::protocol::{IpcMessage, SignalingMessage};
use crate::webrtc_peer;

/// Token response from /api/token.
#[derive(Debug, serde::Deserialize)]
#[allow(dead_code)]
pub struct TokenResponse {
    pub token: String,
    pub peer_id: String,
    pub role: String,
}

/// Exchange API key for JWT + peer_id.
pub async fn exchange_token(token_url: &str, api_key: &str) -> Result<TokenResponse> {
    let client = reqwest::Client::new();
    let resp = client
        .post(token_url)
        .header("x-api-key", api_key)
        .send()
        .await?
        .json::<TokenResponse>()
        .await?;
    Ok(resp)
}

/// Run the signaling connection loop.
pub async fn run(
    ws_url: &str,
    peer_id: &str,
    sig_tx: mpsc::UnboundedSender<String>,
    mut sig_rx: mpsc::UnboundedReceiver<String>,
    mesh: Arc<MeshManager>,
    to_agent_tx: mpsc::UnboundedSender<IpcMessage>,
) -> Result<()> {
    let (ws_stream, _) = connect_async(ws_url).await?;
    let (mut ws_write, mut ws_read) = ws_stream.split();
    info!("[SIG] connected to signaling server");

    // Send register message
    let register = SignalingMessage {
        msg_type: "register".into(),
        peer_id: peer_id.to_string(),
        target_id: None,
        payload: serde_json::Value::Null,
    };
    ws_write
        .send(Message::Text(serde_json::to_string(&register)?.into()))
        .await?;

    // Forward outbound signaling messages
    let ws_write_task = tokio::spawn(async move {
        while let Some(msg) = sig_rx.recv().await {
            if ws_write.send(Message::Text(msg.into())).await.is_err() {
                break;
            }
        }
    });

    // Process incoming signaling messages
    let peer_id = peer_id.to_string();
    while let Some(Ok(msg)) = ws_read.next().await {
        if let Message::Text(text) = msg {
            let text_str: &str = text.as_ref();
            match serde_json::from_str::<SignalingMessage>(text_str) {
                Ok(sm) => {
                    handle_signaling_message(sm, &peer_id, &sig_tx, &mesh, &to_agent_tx).await;
                }
                Err(e) => {
                    warn!("[SIG] failed to parse message: {}", e);
                }
            }
        }
    }

    ws_write_task.abort();
    Ok(())
}

/// Handle an incoming signaling message.
async fn handle_signaling_message(
    msg: SignalingMessage,
    local_peer_id: &str,
    sig_tx: &mpsc::UnboundedSender<String>,
    mesh: &Arc<MeshManager>,
    to_agent_tx: &mpsc::UnboundedSender<IpcMessage>,
) {
    match msg.msg_type.as_str() {
        "peer_list" => {
            // Registry tells us about other connected peers — initiate offers
            if let Some(peers) = msg.payload.get("peers").and_then(|p| p.as_array()) {
                for peer_val in peers {
                    if let Some(pid) = peer_val.as_str() {
                        if pid != local_peer_id && !mesh.is_connected(pid) {
                            info!("[SIG] initiating connection to peer: {}", pid);
                            let mesh = mesh.clone();
                            let sig_tx = sig_tx.clone();
                            let local_id = local_peer_id.to_string();
                            let target_id = pid.to_string();
                            let to_agent = to_agent_tx.clone();
                            tokio::spawn(async move {
                                if let Err(e) = webrtc_peer::initiate_connection(
                                    &local_id, &target_id, sig_tx, mesh, to_agent,
                                )
                                .await
                                {
                                    error!(
                                        "[SIG] failed to initiate connection to {}: {}",
                                        target_id, e
                                    );
                                }
                            });
                        }
                    }
                }
            }
        }
        "offer" => {
            // Incoming offer — create answer
            let from_peer = msg.peer_id.clone();
            if let Some(sdp) = msg.payload.get("sdp").and_then(|s| s.as_str()) {
                let mesh = mesh.clone();
                let sig_tx = sig_tx.clone();
                let local_id = local_peer_id.to_string();
                let sdp = sdp.to_string();
                let to_agent = to_agent_tx.clone();
                tokio::spawn(async move {
                    if let Err(e) = webrtc_peer::handle_offer(
                        &local_id, &from_peer, &sdp, sig_tx, mesh, to_agent,
                    )
                    .await
                    {
                        error!("[SIG] failed to handle offer from {}: {}", from_peer, e);
                    }
                });
            }
        }
        "answer" => {
            // Incoming answer — set remote description
            let from_peer = msg.peer_id.clone();
            if let Some(sdp) = msg.payload.get("sdp").and_then(|s| s.as_str()) {
                if let Some(pc) = mesh.get_pc(&from_peer) {
                    let answer = webrtc::peer_connection::sdp::session_description::RTCSessionDescription::answer(
                        sdp.to_string(),
                    )
                    .unwrap();
                    if let Err(e) = pc.set_remote_description(answer).await {
                        error!("[SIG] failed to set answer from {}: {}", from_peer, e);
                    } else {
                        info!("[SIG] answer set from peer: {}", from_peer);
                    }
                }
            }
        }
        "ice_candidate" => {
            // Incoming ICE candidate
            let from_peer = msg.peer_id.clone();
            if let Some(pc) = mesh.get_pc(&from_peer) {
                let candidate = msg
                    .payload
                    .get("candidate")
                    .and_then(|c| c.as_str())
                    .unwrap_or("")
                    .to_string();
                let sdp_mid = msg
                    .payload
                    .get("sdpMid")
                    .and_then(|s| s.as_str())
                    .map(|s| s.to_string());
                let sdp_mline_index = msg
                    .payload
                    .get("sdpMLineIndex")
                    .and_then(|n| n.as_u64())
                    .map(|n| n as u16);

                let init = webrtc::ice_transport::ice_candidate::RTCIceCandidateInit {
                    candidate,
                    sdp_mid,
                    sdp_mline_index,
                    username_fragment: Some(String::new()),
                };
                if let Err(e) = pc.add_ice_candidate(init).await {
                    warn!("[ICE] failed to add candidate from {}: {}", from_peer, e);
                }
            }
        }
        _ => {
            warn!("[SIG] unhandled message type: {}", msg.msg_type);
        }
    }
}
