# Glossary

| Term | Definition |
|------|------------|
| **Agent** | A Python AI process with skills, LLM integration, and mesh networking. Each agent has one sidecar. |
| **Sidecar** | A Rust process that handles WebRTC signaling and DataChannels on behalf of a Python agent. Communicates with the agent via IPC. |
| **Registry** | Central Rust server (port 8080) that provides signaling relay, agent discovery, task queue, and hub API. |
| **Hub** | The monitoring/control plane: task queue + dashboard. The hub API lives in the registry; the hub UI is a React app. |
| **Mesh** | The network of WebRTC DataChannel connections between sidecars. Full mesh = every sidecar connected to every other sidecar. |
| **Peer** | A sidecar identified by its `peer_id`. Peers communicate via WebRTC DataChannels. |
| **Peer ID** | Unique identifier assigned to a sidecar, derived from its API key entry (e.g., `agent-001`). |
| **Signaling** | The process of exchanging SDP offers/answers and ICE candidates between peers via the registry WebSocket to establish WebRTC connections. |
| **SDP** | Session Description Protocol — describes media/data capabilities. Exchanged as offers and answers during WebRTC negotiation. |
| **ICE** | Interactive Connectivity Establishment — discovers the best network path between peers (direct, STUN, or TURN). |
| **ICE Candidate** | A potential network address (host, server-reflexive, or relay) that a peer can be reached at. |
| **STUN** | Session Traversal Utilities for NAT — server that helps peers discover their public IP. Used for NAT traversal. |
| **TURN** | Traversal Using Relays around NAT — relay server used when direct and STUN connections fail (symmetric NATs). |
| **DataChannel** | A WebRTC primitive for arbitrary data transfer. DTLS-encrypted, P2P. Carries `MeshMessage` JSON in this system. |
| **DTLS** | Datagram Transport Layer Security — encryption layer for WebRTC DataChannels. Automatic, no configuration needed. |
| **IPC** | Inter-Process Communication — the JSON-line protocol over Unix domain socket between a sidecar and its Python agent. |
| **MeshMessage** | Application-level JSON message exchanged over DataChannels: `type`, `request_id`, `source_agent`, `target_agent`, `payload`. |
| **IpcMessage** | JSON-line message between sidecar and Python agent: `type`, `payload`. |
| **Skill** | A named capability (Python function) that an agent can execute. Skills are registered with the registry and used for task routing. |
| **Task** | A unit of work submitted to the hub task queue. Has a lifecycle: pending → assigned → completed/failed. |
| **TTL** | Time To Live — maximum seconds a task can remain pending/assigned before expiring. Default: 300s. |
| **Heartbeat** | Periodic HTTP POST from agent to registry (`/api/hub/heartbeat`). Updates agent metadata and picks up pending tasks. |
| **Health** | Agent status derived from heartbeat recency: `active` (<90s), `stale` (90–270s), `offline` (>270s). |
| **Topology** | The mesh network graph — which agents are online and which DataChannel connections exist between them. |
| **MCP** | Model Context Protocol — standard for connecting LLMs to external tools and data sources. |
| **API Key** | Pre-shared credential (e.g., `ak_dev_001`) exchanged for a JWT to authenticate with the registry. |
| **JWT** | JSON Web Token — short-lived (5 min) bearer token used for WebSocket authentication and sender verification. |
