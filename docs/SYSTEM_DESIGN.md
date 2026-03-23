# System Design

## Overview

Chatixia Mesh is a peer-to-peer agent-to-agent network where AI agents communicate directly over WebRTC DataChannels. A central registry handles signaling and discovery but is not in the data path.

## Architecture

```text
┌────────────────────────────────────────────────────────────────────┐
│                        Registry Server (Rust)                      │
│   Signaling (WebSocket)  │  Agent Registry  │  Hub API (Tasks)    │
│                          │  REST + Health    │  REST + Topology    │
└──────────┬───────────────┴──────────────────┴────────────┬────────┘
           │ WebSocket (SDP/ICE)                           │ HTTP
    ┌──────┴──────┐                                 ┌──────┴──────┐
    │  Sidecar A  │◄── WebRTC DataChannel (P2P) ──►│  Sidecar B  │
    │   (Rust)    │     DTLS encrypted              │   (Rust)    │
    └──────┬──────┘                                 └──────┬──────┘
           │ IPC (Unix socket, JSON lines)                 │ IPC
    ┌──────┴──────┐                                 ┌──────┴──────┐
    │  Agent A    │                                 │  Agent B    │
    │  (Python)   │                                 │  (Python)   │
    └─────────────┘                                 └─────────────┘
```

### Design Principles

1. **Registry is control plane only** — signaling, discovery, task queue. Agent-to-agent data flows directly over WebRTC DataChannels.
2. **Sidecar pattern** — all WebRTC/signaling complexity lives in Rust. Python agents interact via a simple IPC protocol.
3. **Full mesh** — every sidecar connects to every other sidecar. N agents = N×(N-1)/2 DataChannels.
4. **Fail-open discovery** — agents register on heartbeat, so the registry is eventually consistent. No startup ordering required.

## Communication Layers

### Layer 1: Signaling (WebSocket + JWT)

- Sidecar → Registry: SDP offers/answers, ICE candidates
- Registry relays to target sidecar
- JWT authentication (5-min expiry, API key exchange)
- Sender verification: JWT `sub` must match message `peer_id`

### Layer 2: DataChannel (WebRTC, P2P)

- Direct sidecar-to-sidecar, DTLS encrypted
- Application protocol: `MeshMessage` JSON (type, request_id, source_agent, target_agent, payload)
- Message types: task delegation, skill discovery, status broadcast, ping/pong

### Layer 3: IPC (Unix Socket, JSON Lines)

- Sidecar ↔ Python agent
- Commands: `send`, `broadcast`, `list_peers`, `connect`
- Events: `message`, `peer_connected`, `peer_disconnected`, `peer_list`

### Layer 4: Registry REST API (HTTP)

- Agent registration and discovery
- Task queue (submit, poll on heartbeat, update)
- Topology for dashboard visualization
- ICE server configuration (STUN + optional TURN)

## Authentication Flow

```text
Agent starts
  → Sidecar reads API_KEY from environment
  → POST /api/token with X-API-Key header
  → Registry validates key against api_keys.json
  → Returns JWT (5-min TTL) + peer_id + role
  → Sidecar connects to /ws?token=<jwt>
  → Registry validates JWT on WebSocket upgrade
  → Sidecar sends { type: "register" }
  → Registry replies with peer_list
```

## Task Lifecycle

```text
1. Task submitted     → POST /api/hub/tasks (from hub UI or agent delegate skill)
2. Task pending       → Stored in HubState, state="pending"
3. Task assigned      → On next heartbeat, registry matches task to agent by skill/ID
4. Task in-progress   → Agent executes skill
5. Task completed     → POST /api/hub/tasks/{id} with state="completed" + result
   OR Task failed     → state="failed" + error
   OR Task expired    → expire_tasks_loop sets state="failed", error="TTL expired"
```

Task states: `pending` → `assigned` → `completed` | `failed`

Default TTL: 300s (5 minutes). Expiry check runs every 30s.

## Agent Lifecycle

### Registration

- Agent runner (`run_agent.py`) calls `POST /api/registry/agents` on startup
- Sidecar authenticates via JWT and connects to signaling WebSocket
- Agent begins sending heartbeats every ~15s

### Deregistration

- On clean shutdown (SIGINT/SIGTERM): agent calls `DELETE /api/registry/agents/{agent_id}` — instant removal from dashboard
- On hard crash (SIGKILL, OOM, network loss): no deregister call — registry relies on health check (see below)

### Health Tracking

- Agents send heartbeats every ~15s via `POST /api/hub/heartbeat`
- Heartbeat upserts agent record (skills, mode, status, sidecar peer ID)
- Background `health_check_loop` runs every 15s:
  - **active**: last heartbeat <90s ago
  - **stale**: 90–270s ago (likely dead, shutdown handler didn't run)
  - **offline**: >270s ago (confirmed dead)
- Hub dashboard color-codes agents by health state

## NAT Traversal

- Default: Google STUN server (`stun:stun.l.google.com:19302`)
- Optional: self-hosted coturn TURN relay
- TURN credentials: ephemeral, generated via HMAC-SHA1 (coturn `use-auth-secret` mode, 24h TTL)
- ICE config served via `GET /api/config`

## Transport Rationale

WebRTC DataChannels were chosen over HTTP and gRPC for the agent-to-agent data plane. The key factors:

1. **NAT traversal** — agents may run on developer laptops, edge devices, or behind corporate firewalls. ICE/STUN/TURN handles connectivity without VPNs or port forwarding. gRPC and HTTP require all endpoints to be directly addressable.
2. **End-to-end encryption** — DTLS between peers means the registry (signaling server) never sees message content. HTTP routing through the registry would expose all payloads to the server.
3. **No single point of failure for data** — once DataChannels are established, agents communicate directly. Registry downtime does not interrupt existing P2P connections.

### Graceful Degradation

The transport layer degrades across three tiers:

```text
Tier 1: P2P DataChannel     (fastest, <100ms, DTLS encrypted, direct)
  ↓ if no direct path
Tier 2: TURN relay           (slower, still DTLS encrypted, relayed via coturn)
  ↓ if UDP blocked / no TURN
Tier 3: HTTP task queue      (slowest, 3–15s, via registry REST API)
```

Skill handlers (`delegate`, `mesh_send`, `mesh_broadcast`) attempt the P2P path first via `MeshClient`. If the target peer is not directly connected, they fall back to the registry HTTP task queue (ADR-005, ADR-016). The system never fails — it only slows down.

### Trade-offs

This choice carries significant costs: 5–10s connection setup per peer, O(N²) connections in full mesh, SCTP reliable mode has TCP-like head-of-line blocking, the sidecar adds deployment complexity, and the WebRTC ecosystem is less mature than HTTP/gRPC.

See [WEBRTC_VS_ALTERNATIVES.md](WEBRTC_VS_ALTERNATIVES.md) for the full comparison (advantages, devil's advocate critique, rebuttals, and experiment plan).

## Scalability Considerations

- Full mesh: O(N²) connections. Practical for ~10-50 agents.
- Registry is stateless (in-memory DashMap) — no persistence. Restart = agents re-register on next heartbeat.
- Task queue is in-memory. No durability guarantee.
- For larger deployments: consider switching to selective mesh (topic-based routing) and persistent task queue (Redis/PostgreSQL).
