//! Signaling client — connects to registry via WebSocket, handles SDP/ICE exchange.
//!
//! Automatically reconnects with exponential backoff when the WebSocket drops.

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{error, info, warn};
use webrtc::peer_connection::signaling_state::RTCSignalingState;

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

/// Backoff parameters for reconnection.
const INITIAL_BACKOFF: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(30);

/// Run the signaling connection with automatic reconnect.
///
/// On disconnect or error, clears stale WebRTC peers, re-authenticates
/// (JWT may have expired), and reconnects with exponential backoff.
pub async fn run(
    signaling_url: &str,
    token_url: &str,
    api_key: &str,
    peer_id: &str,
    mesh: Arc<MeshManager>,
    to_agent_tx: mpsc::UnboundedSender<IpcMessage>,
) -> Result<()> {
    let mut backoff = INITIAL_BACKOFF;
    let mut attempt: u32 = 0;

    loop {
        // Re-authenticate on every connect (JWT has 5-min expiry)
        let token = match exchange_token(token_url, api_key).await {
            Ok(t) => t,
            Err(e) => {
                warn!(
                    "[SIG] auth failed (attempt {}): {}, retrying in {:?}",
                    attempt, e, backoff
                );
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(MAX_BACKOFF);
                attempt += 1;
                continue;
            }
        };

        let ws_url = format!("{}?token={}", signaling_url, token.token);

        // Create fresh channels for this connection
        let (sig_tx, sig_rx) = mpsc::unbounded_channel::<String>();

        info!("[SIG] connecting (attempt {})...", attempt);
        match connect_once(&ws_url, peer_id, sig_tx, sig_rx, &mesh, &to_agent_tx).await {
            Ok(()) => {
                // The session was established (WS handshake succeeded) and later
                // dropped: reset backoff so the next reconnect is immediate.
                warn!("[SIG] connection closed, reconnecting...");
                attempt = 0;
                backoff = INITIAL_BACKOFF;
            }
            Err(e) => {
                warn!("[SIG] connection error: {}, reconnecting...", e);
            }
        }

        // Clean up stale WebRTC peers before reconnecting
        mesh.clear_all_peers().await;
        mesh.clear_signaling_tx();

        if attempt == 0 {
            // First failure after a working session — reconnect immediately
            info!("[SIG] reconnecting immediately (first attempt)");
        } else {
            info!("[SIG] reconnecting in {:?}", backoff);
            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(MAX_BACKOFF);
        }
        attempt += 1;
    }
}

/// Single signaling connection attempt.
///
/// Returns `Err` only if the WebSocket handshake fails. Once connected, it
/// returns `Ok(())` when the session ends (for any reason), so the caller can
/// distinguish "never connected" from "connected then dropped" for backoff.
async fn connect_once(
    ws_url: &str,
    peer_id: &str,
    sig_tx: mpsc::UnboundedSender<String>,
    mut sig_rx: mpsc::UnboundedReceiver<String>,
    mesh: &Arc<MeshManager>,
    to_agent_tx: &mpsc::UnboundedSender<IpcMessage>,
) -> Result<()> {
    let (ws_stream, _) = connect_async(ws_url).await?;
    let (mut ws_write, mut ws_read) = ws_stream.split();
    info!("[SIG] connected to signaling server");

    // Expose the outbound signaling sender so the IPC `connect` command can
    // initiate WebRTC offers for this session.
    mesh.set_signaling_tx(sig_tx.clone());

    // Send register message
    let register = SignalingMessage {
        msg_type: "register".into(),
        peer_id: peer_id.to_string(),
        target_id: None,
        payload: serde_json::Value::Null,
    };
    if let Err(e) = ws_write
        .send(Message::Text(serde_json::to_string(&register)?.into()))
        .await
    {
        warn!("[SIG] failed to send register: {}", e);
        return Ok(());
    }

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
                    handle_signaling_message(sm, &peer_id, &sig_tx, mesh, to_agent_tx).await;
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
            // Registry tells us about other connected peers — initiate offers.
            //
            // Awaited here, not spawned: the offer must be on record (in
            // `have-local-offer`) before this loop reads the next signaling
            // message, or a crossing offer from the same peer is answered
            // without the glare check and both sides end up answering.
            if let Some(peers) = msg.payload.get("peers").and_then(|p| p.as_array()) {
                for peer_val in peers {
                    if let Some(pid) = peer_val.as_str() {
                        if pid != local_peer_id && !mesh.is_connected(pid) {
                            info!("[SIG] initiating connection to peer: {}", pid);
                            if let Err(e) = webrtc_peer::initiate_connection(
                                local_peer_id,
                                pid,
                                sig_tx.clone(),
                                mesh.clone(),
                                to_agent_tx.clone(),
                            )
                            .await
                            {
                                error!("[SIG] failed to initiate connection to {}: {}", pid, e);
                            }
                        }
                    }
                }
            }
        }
        "offer" => {
            // Incoming offer — create answer
            let from_peer = msg.peer_id.clone();

            // Offer glare: both sides offered at once (e.g. two sidecars
            // registering in the same instant both see each other in their
            // peer_list). Deterministic tie-break so exactly one offer wins:
            // the lower peer_id keeps its offer, the higher one yields.
            let mut yielded = None;
            if let Some(existing) = mesh.get_pc(&from_peer) {
                if existing.signaling_state() == RTCSignalingState::HaveLocalOffer {
                    if glare_keep_local_offer(local_peer_id, &from_peer) {
                        info!(
                            "[SIG] offer glare with {}: keeping our offer (lower peer_id wins)",
                            from_peer
                        );
                        return;
                    }
                    info!(
                        "[SIG] offer glare with {}: yielding to their offer",
                        from_peer
                    );
                    yielded = Some(existing);
                }
            }

            // Awaited for the same reason as `peer_list`: the answering
            // connection is on record before the next message is read.
            if let Some(sdp) = msg.payload.get("sdp").and_then(|s| s.as_str()) {
                if let Err(e) = webrtc_peer::handle_offer(
                    local_peer_id,
                    &from_peer,
                    sdp,
                    sig_tx.clone(),
                    mesh.clone(),
                    to_agent_tx.clone(),
                )
                .await
                {
                    error!("[SIG] failed to handle offer from {}: {}", from_peer, e);
                }
            }

            // Close the abandoned offer only after its replacement is on
            // record, so its Closed callback sees a stale connection and
            // does not report the peer as disconnected.
            if let Some(existing) = yielded {
                tokio::spawn(async move {
                    let _ = existing.close().await;
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
                        mesh.flush_candidates(&from_peer).await;
                    }
                }
            }
        }
        "ice_candidate" => {
            // Incoming ICE candidate
            let from_peer = msg.peer_id.clone();
            {
                let candidate = msg
                    .payload
                    .get("candidate")
                    .and_then(|c| c.as_str())
                    .unwrap_or("")
                    .to_string();
                info!("[ICE] remote candidate from {}: {}", from_peer, candidate);
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
                    username_fragment: None,
                };
                // Trickle ICE races the offer/answer: a candidate can arrive
                // before its connection exists or has a remote description.
                // Hold it and apply it once the description is set.
                match mesh.get_pc(&from_peer) {
                    Some(pc) if pc.remote_description().await.is_some() => {
                        if let Err(e) = pc.add_ice_candidate(init).await {
                            warn!("[ICE] failed to add candidate from {}: {}", from_peer, e);
                        }
                    }
                    _ => {
                        mesh.queue_candidate(&from_peer, init);
                        // The description may have landed while we queued.
                        mesh.flush_candidates(&from_peer).await;
                    }
                }
            }
        }
        _ => {
            warn!("[SIG] unhandled message type: {}", msg.msg_type);
        }
    }
}

/// Offer-glare tie-break: when both peers have sent an offer, the peer with
/// the lexicographically lower id keeps its own offer and ignores the
/// incoming one; the other peer abandons its offer and answers.
fn glare_keep_local_offer(local_peer_id: &str, remote_peer_id: &str) -> bool {
    local_peer_id < remote_peer_id
}

#[cfg(test)]
mod tests {
    use super::{glare_keep_local_offer, handle_signaling_message};

    use std::sync::Arc;
    use std::time::Duration;

    use tokio::sync::mpsc;
    use webrtc::peer_connection::signaling_state::RTCSignalingState;

    use crate::mesh::MeshManager;
    use crate::protocol::{IpcMessage, SignalingMessage};

    struct Side {
        id: &'static str,
        mesh: Arc<MeshManager>,
        sig_tx: mpsc::UnboundedSender<String>,
        sig_rx: mpsc::UnboundedReceiver<String>,
        agent_tx: mpsc::UnboundedSender<IpcMessage>,
        _agent_rx: mpsc::UnboundedReceiver<IpcMessage>,
    }

    impl Side {
        fn new(id: &'static str) -> Self {
            let (sig_tx, sig_rx) = mpsc::unbounded_channel();
            let (agent_tx, _agent_rx) = mpsc::unbounded_channel();
            Self {
                id,
                mesh: Arc::new(MeshManager::new(id.into())),
                sig_tx,
                sig_rx,
                agent_tx,
                _agent_rx,
            }
        }

        async fn handle(&self, msg: SignalingMessage) {
            handle_signaling_message(msg, self.id, &self.sig_tx, &self.mesh, &self.agent_tx).await;
        }

        /// Next outbound signaling message of `msg_type` (skips trickled
        /// ice_candidate messages). Fails if none is already queued or
        /// arrives within a second.
        async fn next_sent(&mut self, msg_type: &str) -> SignalingMessage {
            let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
            loop {
                let raw = tokio::time::timeout_at(deadline, self.sig_rx.recv())
                    .await
                    .unwrap_or_else(|_| panic!("{} sent no {}", self.id, msg_type))
                    .expect("signaling channel closed");
                let msg: SignalingMessage = serde_json::from_str(&raw).unwrap();
                if msg.msg_type == msg_type {
                    return msg;
                }
            }
        }

        /// Types of every outbound message queued so far.
        fn drain_sent(&mut self) -> Vec<String> {
            let mut types = Vec::new();
            while let Ok(raw) = self.sig_rx.try_recv() {
                let msg: SignalingMessage = serde_json::from_str(&raw).unwrap();
                types.push(msg.msg_type);
            }
            types
        }

        fn state_toward(&self, peer: &str) -> RTCSignalingState {
            self.mesh
                .get_pc(peer)
                .expect("no connection on record")
                .signaling_state()
        }
    }

    fn peer_list(peer: &str) -> SignalingMessage {
        SignalingMessage {
            msg_type: "peer_list".into(),
            peer_id: "registry".into(),
            target_id: None,
            payload: serde_json::json!({ "peers": [peer] }),
        }
    }

    /// Two sidecars get each other in their peer_list at the same moment and
    /// both offer (ADR-021). The offer must already be on record when the
    /// peer_list handler returns; when it was spawned instead, a crossing
    /// offer could be answered without the glare check and both sides
    /// rejected the other's answer ("stable applying remote answer").
    #[tokio::test]
    async fn crossing_offers_resolve_to_exactly_one_negotiation() {
        let mut a = Side::new("glare-a");
        let mut b = Side::new("glare-b");

        a.handle(peer_list(b.id)).await;
        b.handle(peer_list(a.id)).await;
        assert_eq!(a.state_toward(b.id), RTCSignalingState::HaveLocalOffer);
        assert_eq!(b.state_toward(a.id), RTCSignalingState::HaveLocalOffer);
        let a_offer = a.next_sent("offer").await;
        let b_offer = b.next_sent("offer").await;
        let a_offerer = a.mesh.get_pc(b.id).unwrap();
        let b_offerer = b.mesh.get_pc(a.id).unwrap();

        // The offers cross. The lower id keeps its offer and ignores theirs.
        a.handle(b_offer).await;
        assert!(Arc::ptr_eq(&a.mesh.get_pc(b.id).unwrap(), &a_offerer));
        assert_eq!(a.state_toward(b.id), RTCSignalingState::HaveLocalOffer);
        // The higher id yields: its answering connection replaces its offer.
        b.handle(a_offer).await;
        assert!(!Arc::ptr_eq(&b.mesh.get_pc(a.id).unwrap(), &b_offerer));
        assert_eq!(b.state_toward(a.id), RTCSignalingState::Stable);

        // Exactly one answer comes back, and it applies to A's offer.
        let b_answer = b.next_sent("answer").await;
        a.handle(b_answer).await;
        assert_eq!(a.state_toward(b.id), RTCSignalingState::Stable);
        assert!(Arc::ptr_eq(&a.mesh.get_pc(b.id).unwrap(), &a_offerer));
        assert!(
            !a.drain_sent().contains(&"answer".to_string()),
            "A must not answer B's abandoned offer"
        );

        a.mesh.clear_all_peers().await;
        b.mesh.clear_all_peers().await;
        let _ = b_offerer.close().await;
    }

    #[test]
    fn glare_exactly_one_side_keeps_its_offer() {
        let (a, b) = ("world-the-alpha", "world-the-beta");
        assert!(glare_keep_local_offer(a, b));
        assert!(!glare_keep_local_offer(b, a));
    }

    #[test]
    fn glare_rule_is_antisymmetric_for_any_distinct_ids() {
        let ids = ["agent-001", "agent-002", "pi-kitchen", "Z", "a", "world-x"];
        for x in ids {
            for y in ids {
                if x != y {
                    assert_ne!(glare_keep_local_offer(x, y), glare_keep_local_offer(y, x));
                }
            }
        }
    }
}
