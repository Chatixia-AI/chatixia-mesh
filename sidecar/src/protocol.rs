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
#[allow(dead_code)]
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
#[allow(dead_code)]
pub mod ipc_types {
    // Agent → Sidecar commands
    pub const SEND: &str = "send"; // Send message to specific peer
    pub const BROADCAST: &str = "broadcast"; // Broadcast to all peers
    pub const CONNECT: &str = "connect"; // Connect to a peer
    pub const LIST_PEERS: &str = "list_peers"; // List connected peers

    // Sidecar → Agent events
    pub const MESSAGE: &str = "message"; // Received message from peer
    pub const PEER_CONNECTED: &str = "peer_connected";
    pub const PEER_DISCONNECTED: &str = "peer_disconnected";
    pub const PEER_LIST: &str = "peer_list"; // Response to list_peers
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── SignalingMessage ────────────────────────────────────────────────

    #[test]
    fn test_signaling_message_serialize_with_target() {
        let msg = SignalingMessage {
            msg_type: "offer".into(),
            peer_id: "peer-1".into(),
            target_id: Some("peer-2".into()),
            payload: serde_json::json!({"sdp": "..."}),
        };
        let json: serde_json::Value = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["type"], "offer");
        assert_eq!(json["peer_id"], "peer-1");
        assert_eq!(json["target_id"], "peer-2");
        assert_eq!(json["payload"]["sdp"], "...");
        // "msg_type" key must NOT appear (renamed to "type")
        assert!(json.get("msg_type").is_none());
    }

    #[test]
    fn test_signaling_message_serialize_without_target() {
        let msg = SignalingMessage {
            msg_type: "join".into(),
            peer_id: "peer-1".into(),
            target_id: None,
            payload: serde_json::Value::Null,
        };
        let json: serde_json::Value = serde_json::to_value(&msg).unwrap();
        // target_id should be skipped entirely when None
        assert!(json.get("target_id").is_none());
    }

    #[test]
    fn test_signaling_message_deserialize() {
        let raw = r#"{"type":"answer","peer_id":"p1","target_id":"p2","payload":{}}"#;
        let msg: SignalingMessage = serde_json::from_str(raw).unwrap();
        assert_eq!(msg.msg_type, "answer");
        assert_eq!(msg.peer_id, "p1");
        assert_eq!(msg.target_id, Some("p2".into()));
    }

    #[test]
    fn test_signaling_message_roundtrip() {
        let original = SignalingMessage {
            msg_type: "ice".into(),
            peer_id: "a".into(),
            target_id: Some("b".into()),
            payload: serde_json::json!({"candidate": "c1"}),
        };
        let json_str = serde_json::to_string(&original).unwrap();
        let decoded: SignalingMessage = serde_json::from_str(&json_str).unwrap();
        assert_eq!(decoded.msg_type, original.msg_type);
        assert_eq!(decoded.peer_id, original.peer_id);
        assert_eq!(decoded.target_id, original.target_id);
        assert_eq!(decoded.payload, original.payload);
    }

    // ─── MeshMessage ────────────────────────────────────────────────────

    #[test]
    fn test_mesh_message_serialize() {
        let msg = MeshMessage {
            msg_type: mesh_types::TASK_REQUEST.into(),
            request_id: "req-1".into(),
            source_agent: "agent-a".into(),
            target_agent: "agent-b".into(),
            payload: serde_json::json!({"task": "summarize"}),
        };
        let json: serde_json::Value = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["type"], "task_request");
        assert_eq!(json["request_id"], "req-1");
        assert_eq!(json["source_agent"], "agent-a");
        assert_eq!(json["target_agent"], "agent-b");
        assert_eq!(json["payload"]["task"], "summarize");
        assert!(json.get("msg_type").is_none());
    }

    #[test]
    fn test_mesh_message_deserialize_defaults() {
        // Only "type" is required; everything else should default.
        let raw = r#"{"type":"ping"}"#;
        let msg: MeshMessage = serde_json::from_str(raw).unwrap();
        assert_eq!(msg.msg_type, "ping");
        assert_eq!(msg.request_id, "");
        assert_eq!(msg.source_agent, "");
        assert_eq!(msg.target_agent, "");
        assert_eq!(msg.payload, serde_json::Value::Null);
    }

    #[test]
    fn test_mesh_message_roundtrip() {
        let original = MeshMessage {
            msg_type: mesh_types::SKILL_QUERY.into(),
            request_id: "r42".into(),
            source_agent: "src".into(),
            target_agent: "tgt".into(),
            payload: serde_json::json!(["skill_a", "skill_b"]),
        };
        let json_str = serde_json::to_string(&original).unwrap();
        let decoded: MeshMessage = serde_json::from_str(&json_str).unwrap();
        assert_eq!(decoded.msg_type, original.msg_type);
        assert_eq!(decoded.request_id, original.request_id);
        assert_eq!(decoded.source_agent, original.source_agent);
        assert_eq!(decoded.target_agent, original.target_agent);
        assert_eq!(decoded.payload, original.payload);
    }

    // ─── IpcMessage ─────────────────────────────────────────────────────

    #[test]
    fn test_ipc_message_serialize() {
        let msg = IpcMessage {
            msg_type: ipc_types::SEND.into(),
            payload: serde_json::json!({"target": "peer-3", "data": 42}),
        };
        let json: serde_json::Value = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["type"], "send");
        assert_eq!(json["payload"]["target"], "peer-3");
        assert!(json.get("msg_type").is_none());
    }

    #[test]
    fn test_ipc_message_deserialize() {
        let raw = r#"{"type":"message","payload":{"text":"hello"}}"#;
        let msg: IpcMessage = serde_json::from_str(raw).unwrap();
        assert_eq!(msg.msg_type, "message");
        assert_eq!(msg.payload["text"], "hello");
    }

    #[test]
    fn test_ipc_message_roundtrip() {
        let original = IpcMessage {
            msg_type: ipc_types::BROADCAST.into(),
            payload: serde_json::json!({"content": "hi all"}),
        };
        let json_str = serde_json::to_string(&original).unwrap();
        let decoded: IpcMessage = serde_json::from_str(&json_str).unwrap();
        assert_eq!(decoded.msg_type, original.msg_type);
        assert_eq!(decoded.payload, original.payload);
    }

    // ─── Constants ──────────────────────────────────────────────────────

    #[test]
    fn test_mesh_type_constants() {
        assert_eq!(mesh_types::PING, "ping");
        assert_eq!(mesh_types::PONG, "pong");
        assert_eq!(mesh_types::TASK_REQUEST, "task_request");
        assert_eq!(mesh_types::TASK_RESPONSE, "task_response");
        assert_eq!(mesh_types::TASK_STREAM_CHUNK, "task_stream_chunk");
        assert_eq!(mesh_types::SKILL_QUERY, "skill_query");
        assert_eq!(mesh_types::SKILL_RESPONSE, "skill_response");
        assert_eq!(mesh_types::AGENT_STATUS, "agent_status");
        assert_eq!(mesh_types::AGENT_PROMPT, "agent_prompt");
        assert_eq!(mesh_types::AGENT_RESPONSE, "agent_response");
        assert_eq!(mesh_types::AGENT_STREAM_CHUNK, "agent_stream_chunk");
    }

    #[test]
    fn test_ipc_type_constants() {
        assert_eq!(ipc_types::SEND, "send");
        assert_eq!(ipc_types::BROADCAST, "broadcast");
        assert_eq!(ipc_types::CONNECT, "connect");
        assert_eq!(ipc_types::LIST_PEERS, "list_peers");
        assert_eq!(ipc_types::MESSAGE, "message");
        assert_eq!(ipc_types::PEER_CONNECTED, "peer_connected");
        assert_eq!(ipc_types::PEER_DISCONNECTED, "peer_disconnected");
        assert_eq!(ipc_types::PEER_LIST, "peer_list");
    }
}
