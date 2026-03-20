# chatixia-mesh

Agent-to-agent mesh network with WebRTC DataChannels. Combines the [chatixia-agent](https://github.com/Chatixia-AI/chatixia-agent) Python AI framework with the [rust-p2p](https://github.com/Chatixia-AI/rust-p2p) WebRTC stack into a unified system where agents discover each other, communicate directly over encrypted P2P channels, and are monitored from a central hub.

## What is this?

- **Agents** are Python processes with skills (tools), MCP integration, autonomous goals, and knowledge bases
- **Sidecars** are Rust binaries that handle WebRTC DataChannels — one per agent
- **Registry** is the central signaling + discovery + monitoring server
- **Hub** is a React dashboard where users monitor and intervene

```
┌──────────────┐     ┌──────────────┐     ┌──────────────┐
│  Agent (Py)  │     │  Agent (Py)  │     │  Agent (Py)  │
│  29+ skills  │     │  MCP tools   │     │  Auto goals  │
└──────┬───────┘     └──────┬───────┘     └──────┬───────┘
       │ IPC                │ IPC                │ IPC
┌──────▼───────┐     ┌──────▼───────┐     ┌──────▼───────┐
│ Sidecar (Rs) │◄───►│ Sidecar (Rs) │◄───►│ Sidecar (Rs) │
│   WebRTC DC  │ P2P │   WebRTC DC  │ P2P │   WebRTC DC  │
└──────┬───────┘     └──────┬───────┘     └──────┬───────┘
       │ WS                 │ WS                 │ WS
       └────────────────────┼────────────────────┘
                    ┌───────▼────────┐
                    │   Registry     │
                    │  (Rust/axum)   │
                    │ signaling+hub  │
                    └───────┬────────┘
                            │ HTTP
                    ┌───────▼────────┐
                    │  Hub Dashboard │
                    │    (React)     │
                    └────────────────┘
```

## Quick Start

### Prerequisites

- Rust 1.75+
- Python 3.12+
- Node.js 20+

### 1. Build

```bash
# Rust components
cargo build --release

# Hub dashboard
cd hub && npm install && npm run build && cd ..
```

### 2. Configure

```bash
cp .env.example .env
# Edit .env with your settings (LLM provider, API keys, etc.)
```

### 3. Run the Registry

```bash
cargo run --release -p chatixia-registry
# Listening on 0.0.0.0:8080
```

### 4. Run an Agent

```bash
# Copy and customize the agent config
cp agent.yaml.example my-agent.yaml

# Start the agent (spawns sidecar automatically)
cd agent
API_KEY=ak_dev_001 CHATIXIA_REGISTRY_URL=http://localhost:8080 python -c "
from core.mesh_client import MeshClient
import asyncio
async def main():
    client = MeshClient()
    await client.start()
    print('Agent connected to mesh')
    await asyncio.Event().wait()
asyncio.run(main())
"
```

### 5. Open the Hub

Visit `http://localhost:8080` — the registry serves the hub dashboard.

## Project Structure

```
chatixia-mesh/
├── registry/        # Signaling + registry + hub API (Rust/axum)
├── sidecar/         # WebRTC mesh peer + IPC bridge (Rust/webrtc-rs)
├── agent/           # Python AI agent framework
│   ├── core/        # Agent core (skills, MCP, mesh client, autonomous)
│   └── skills/      # Skill definitions and handlers
├── hub/             # Monitoring dashboard (React/Vite)
├── web-client/      # Phone/browser client
├── infra/           # nginx, coturn configs
└── docs/            # Architecture, protocol, ADRs
```

## Documentation

- [Architecture](docs/ARCHITECTURE.md) — system design and data flow
- [Protocol](docs/PROTOCOL.md) — signaling, DataChannel, IPC, REST API reference
- [CLAUDE.md](CLAUDE.md) — build and run instructions for Claude Code

## Origin

This project combines:
- **[chatixia-agent](https://github.com/Chatixia-AI/chatixia-agent)** — Python AI agent framework with skills, MCP, autonomous goals, and multi-agent hub
- **[rust-p2p](https://github.com/Chatixia-AI/rust-p2p)** — Rust WebRTC P2P system with signaling server, DTLS encryption, and TURN support
