# Architecture

## Overview

Chatixia Mesh is an agent-to-agent network where AI agents communicate directly
over WebRTC DataChannels. Each agent is a Python process with a Rust sidecar
that handles all WebRTC/signaling complexity.

## Components

### Registry Server (Rust — `registry/`)

Single server that combines three roles:

1. **Signaling relay** — WebSocket server that forwards SDP offers/answers and
   ICE candidates between peers. Supports N:N mesh (not just 1:1).

2. **Agent registry** — Tracks which agents are online, their skills, health,
   and sidecar peer IDs. Agents register via HTTP POST or heartbeat.

3. **Hub API** — Task queue (submit, poll, update) and network topology for
   the monitoring dashboard.

Port: 8080 (HTTP + WebSocket)

### Sidecar (Rust — `sidecar/`)

One sidecar per Python agent. Responsibilities:

- Connect to registry via WebSocket for signaling
- Establish WebRTC DataChannels with other sidecars (full mesh)
- Bridge messages between DataChannel mesh and Python agent via IPC
- Handle STUN/TURN for NAT traversal

The sidecar is spawned by the Python agent as a subprocess and communicates
via a Unix domain socket using JSON-line protocol.

### Python Agent (`agent/`)

The AI agent framework (ported from chatixia-agent):

- **Skills** — 29+ skill handlers (Python functions callable by LLM)
- **MCP** — Model Context Protocol client for external tool servers
- **Sessions** — Chat history persistence (SQLite)
- **Knowledge** — Semantic knowledge base with entity extraction
- **Memory** — Long-term memory across sessions
- **Autonomous** — Sense-Think-Act loop for goal-driven behavior
- **Mesh client** — IPC bridge to Rust sidecar

### Hub Dashboard (React — `hub/`)

Browser-based monitoring dashboard:

- Agent status cards with health indicators
- Task queue with state tracking
- WebRTC mesh topology visualization (canvas)
- User intervention — send prompts/tasks to any agent

## Data Flow

### Agent-to-Agent Communication

```
Agent A (Python)
    ↓ IPC (Unix socket, JSON lines)
Sidecar A (Rust)
    ↓ WebRTC DataChannel (DTLS encrypted, P2P)
Sidecar B (Rust)
    ↓ IPC (Unix socket, JSON lines)
Agent B (Python)
```

### User Intervention (via Hub)

```
User (Browser)
    ↓ HTTP POST /api/hub/tasks
Registry Server
    ↓ Task queued, assigned on next heartbeat
Agent (Python)
    ↓ Executes skill, returns result
Registry Server
    ↓ HTTP GET /api/hub/tasks/{id}
User (Browser)
```

### Agent Registration

```
Agent starts
    → Spawns sidecar subprocess
    → Sidecar connects to registry WebSocket, registers
    → Registry returns peer_list of other sidecars
    → Sidecar initiates WebRTC connections to all peers
    → Agent registers capabilities via HTTP POST /api/registry/agents
    → Agent starts heartbeat (every 10s)
```

## Security

1. **API keys** — Each agent has an API key mapped to a peer_id and role
2. **JWT** — API key exchanged for short-lived JWT (5 min) for WebSocket auth
3. **DTLS** — WebRTC DataChannels are encrypted by default
4. **TURN credentials** — Ephemeral (coturn use-auth-secret mode, 24h TTL)
5. **Sender verification** — Registry checks JWT peer_id matches message peer_id
