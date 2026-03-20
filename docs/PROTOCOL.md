# Protocol Reference

## 1. Signaling Protocol (WebSocket)

Messages exchanged between sidecars and the registry over WebSocket.

### Message Format

```json
{
  "type": "register|offer|answer|ice_candidate|peer_list|heartbeat",
  "peer_id": "agent-001",
  "target_id": "agent-002",  // optional, for directed messages
  "payload": {}              // type-specific data
}
```

### Message Types

| Type | Direction | Payload |
|------|-----------|---------|
| `register` | sidecar → registry | `{}` |
| `peer_list` | registry → sidecar | `{ "peers": ["peer-a", "peer-b"] }` |
| `offer` | sidecar → sidecar (via registry) | `{ "sdp": "v=0\r\n..." }` |
| `answer` | sidecar → sidecar (via registry) | `{ "sdp": "v=0\r\n..." }` |
| `ice_candidate` | sidecar ↔ sidecar (via registry) | `{ "candidate": "...", "sdpMid": "...", "sdpMLineIndex": 0 }` |
| `heartbeat` | sidecar → registry | `{}` |

## 2. DataChannel Protocol (WebRTC)

Messages exchanged directly between agents over WebRTC DataChannels (DTLS encrypted).

### Message Format

```json
{
  "type": "task_request|task_response|task_stream_chunk|ping|pong|...",
  "request_id": "abc123",
  "source_agent": "agent-001",
  "target_agent": "agent-002",
  "payload": {}
}
```

### Message Types

| Type | Direction | Payload |
|------|-----------|---------|
| `ping` | A → B | `{}` |
| `pong` | B → A | `{}` (same request_id) |
| `task_request` | A → B | `{ "skill": "...", "args": {...}, "timeout_ms": 30000 }` |
| `task_response` | B → A | `{ "result": "..." }` or `{ "error": "..." }` |
| `task_stream_chunk` | B → A | `{ "text": "...", "done": false }` |
| `skill_query` | A → B | `{ "skill": "calculator" }` |
| `skill_response` | B → A | `{ "available": true, "description": "..." }` |
| `agent_status` | A → all | `{ "status": "thinking|acting|idle" }` |
| `agent_prompt` | user → agent | `{ "text": "...", "session_id": "..." }` |
| `agent_response` | agent → user | `{ "text": "..." }` |
| `agent_stream_chunk` | agent → user | `{ "text": "...", "done": false }` |

## 3. IPC Protocol (Unix Socket)

JSON-line protocol between Python agent and Rust sidecar. One JSON object per line.

### Agent → Sidecar (Commands)

```json
{"type": "send", "payload": {"target_peer": "peer-abc", "message": {...}}}
{"type": "broadcast", "payload": {"message": {...}}}
{"type": "list_peers", "payload": {}}
{"type": "connect", "payload": {"target_peer_id": "peer-abc"}}
```

### Sidecar → Agent (Events)

```json
{"type": "message", "payload": {"from_peer": "peer-abc", "message": {...}}}
{"type": "peer_connected", "payload": {"peer_id": "peer-abc"}}
{"type": "peer_disconnected", "payload": {"peer_id": "peer-abc"}}
{"type": "peer_list", "payload": {"peers": ["peer-abc", "peer-def"]}}
```

## 4. Registry REST API

### Authentication

```
POST /api/token
Header: X-API-Key: ak_dev_001
→ { "token": "jwt...", "peer_id": "agent-001", "role": "agent" }
```

### Agent Registry

```
POST /api/registry/agents          # Register/update agent
GET  /api/registry/agents          # List all agents
GET  /api/registry/agents/:id      # Get specific agent
GET  /api/registry/route?skill=X   # Find agent by skill
```

### Hub (Tasks)

```
POST /api/hub/tasks                # Submit task
GET  /api/hub/tasks/all            # List all tasks
GET  /api/hub/tasks/:id            # Get task status
POST /api/hub/tasks/:id            # Update task result
POST /api/hub/heartbeat            # Agent heartbeat (compat)
```

### Monitoring

```
GET  /api/hub/network/topology     # Mesh topology for visualization
GET  /api/config                   # ICE server config (STUN/TURN)
```
