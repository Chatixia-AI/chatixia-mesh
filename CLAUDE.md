# chatixia-mesh

Agent-to-agent mesh network with WebRTC DataChannels.

## Architecture

- **registry/** — Rust (axum): signaling server + agent registry + hub API (port 8080)
- **sidecar/** — Rust (webrtc-rs): WebRTC mesh peer + IPC bridge to Python agent
- **agent/** — Python: AI agent framework (skills, MCP, autonomous goals, knowledge)
- **hub/** — React (Vite): monitoring dashboard

## Build

```bash
# Rust (registry + sidecar)
cargo build --release

# Hub dashboard
cd hub && npm install && npm run build

# Python agent
cd agent && pip install -e .
```

## Run

```bash
# 1. Start registry
cargo run --release -p chatixia-registry

# 2. Start an agent with sidecar
# The agent spawns the sidecar automatically
cd agent && python -m core.runner --config ../agent.yaml.example

# 3. Hub dashboard (dev)
cd hub && npm run dev
```

## Key files

| Component | Entry point |
|-----------|-------------|
| Registry | `registry/src/main.rs` |
| Sidecar | `sidecar/src/main.rs` |
| Agent mesh client | `agent/core/mesh_client.py` |
| Mesh skills | `agent/core/mesh_skills.py` |
| Hub dashboard | `hub/src/App.tsx` |

## Protocol

- **Signaling**: JSON over WebSocket (SDP offers/answers, ICE candidates)
- **DataChannel**: JSON `MeshMessage` (task_request, task_response, agent_prompt, etc.)
- **IPC**: JSON lines over Unix socket (send, broadcast, message events)
- **Registry API**: REST (agent registration, skill routing, task queue)
