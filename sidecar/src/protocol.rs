//! Protocol types for signaling, DataChannel, and IPC.

use serde::{Deserialize, Serialize};

// ─── Signaling (WebSocket to registry) ───────────────────────────────────

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

// ─── DataChannel (WebRTC, agent-to-agent) ────────────────────────────────

/// Application-level message exchanged over DataChannel.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MeshMessage {
    #[serde(rename = "type")]
    pub msg_type: String,
    #[serde(default)]
    pub request_id: String,
    #[serde(default)]
    pub source_agent: String,
    #[serde(default)]
    pub target_agent: String,
    #[serde(default)]
    pub payload: serde_json::Value,
}

/// DataChannel message types.
pub mod mesh_types {
    /// Ping/pong for connectivity testing.
    pub const PING: &str = "ping";
    pub const PONG: &str = "pong";

    /// Task delegation between agents.
    pub const TASK_REQUEST: &str = "task_request";
    pub const TASK_RESPONSE: &str = "task_response";
    pub const TASK_STREAM_CHUNK: &str = "task_stream_chunk";

    /// Skill discovery.
    pub const SKILL_QUERY: &str = "skill_query";
    pub const SKILL_RESPONSE: &str = "skill_response";

    /// Agent status broadcast.
    pub const AGENT_STATUS: &str = "agent_status";

    /// Generic message relay (for agent_prompt compatibility).
    pub const AGENT_PROMPT: &str = "agent_prompt";
    pub const AGENT_RESPONSE: &str = "agent_response";
    pub const AGENT_STREAM_CHUNK: &str = "agent_stream_chunk";
}

// ─── IPC (Unix socket, sidecar ↔ Python agent) ──────────────────────────

/// IPC message between sidecar and Python agent.
/// JSON-line protocol over Unix domain socket.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct IpcMessage {
    #[serde(rename = "type")]
    pub msg_type: String,
    #[serde(default)]
    pub payload: serde_json::Value,
}

/// IPC message types.
pub mod ipc_types {
    // Agent → Sidecar commands
    pub const SEND: &str = "send";           // Send message to specific peer
    pub const BROADCAST: &str = "broadcast"; // Broadcast to all peers
    pub const CONNECT: &str = "connect";     // Connect to a peer
    pub const LIST_PEERS: &str = "list_peers"; // List connected peers

    // Sidecar → Agent events
    pub const MESSAGE: &str = "message";               // Received message from peer
    pub const PEER_CONNECTED: &str = "peer_connected";
    pub const PEER_DISCONNECTED: &str = "peer_disconnected";
    pub const PEER_LIST: &str = "peer_list";           // Response to list_peers
}
