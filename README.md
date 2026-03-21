# chatixia-mesh

Agent-to-agent mesh network with WebRTC DataChannels. AI agents discover each other, communicate directly over encrypted P2P channels, and are monitored from a central hub.

## What is this?

- **Agents** are Python processes with skills (tools), MCP integration, autonomous goals, and knowledge bases
- **Sidecars** are Rust binaries that handle WebRTC DataChannels — one per agent
- **Registry** is the central signaling + discovery + monitoring server
- **Hub** is a React dashboard where admins monitor agents and approve new ones

```text
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

### 1. Build the infrastructure

```bash
# Rust components (registry + sidecar)
cargo build --release

# Hub dashboard
cd hub && npm install && npm run build && cd ..
```

### 2. Start the registry

```bash
cargo run --release -p chatixia-registry
# Listening on 0.0.0.0:8080
```

### 3. Create and run an agent

```bash
# Install the CLI
cd agent && pip install -e .

# Scaffold a new agent
chatixia init my-weather-bot
cd my-weather-bot

# Configure credentials
cp .env.example .env
# Edit .env with your LLM provider keys

# Validate the manifest
chatixia validate

# Pair with the mesh (get an invite code from an admin)
chatixia pair 482901

# Run the agent
chatixia run
```

### 4. Open the Hub

Visit `http://localhost:8080` — the registry serves the hub dashboard. From here you can monitor agents, approve new ones, and submit tasks.

## Agent Onboarding

New agents join the mesh through a pairing + approval flow:

1. An existing mesh member generates a 6-digit invite code (via hub or API)
2. The new agent redeems the code: `chatixia pair <code>`
3. An admin approves the agent in the hub dashboard
4. The agent receives a device token and can connect to the mesh

## CLI Reference

| Command                            | Description                                                       |
| ---------------------------------- | ----------------------------------------------------------------- |
| `chatixia init [name]`             | Scaffold a new agent (`agent.yaml`, `.env.example`, `.gitignore`) |
| `chatixia run [manifest]`          | Run an agent — register with registry, connect to mesh, heartbeat |
| `chatixia validate [manifest]`     | Validate an agent manifest and print summary                      |
| `chatixia pair <code> [manifest]`  | Redeem a 6-digit invite code to join a mesh network               |
| `chatixia -V`                      | Show version                                                      |

## Agent Manifest (`agent.yaml`)

```yaml
name: my-weather-bot
description: "Fetches weather data and shares with the mesh"

registry: "http://localhost:8080"

provider: azure          # azure | openai | ollama
model: gpt-4o

prompt: |
  You are a weather specialist agent.
  Use delegate to ask other agents for help.

sidecar:
  binary: ./target/release/chatixia-sidecar
  api_key: ak_dev_001
  socket: /tmp/chatixia-my-weather-bot.sock

skills:
  builtin:
    - delegate
    - list_agents
    - mesh_send
    - mesh_broadcast
  # dirs:
  #   - ./custom-skills

data_dir: .chatixia
```

## Project Structure

```text
chatixia-mesh/
├── registry/           # Signaling + registry + hub API (Rust/axum)
├── sidecar/            # WebRTC mesh peer + IPC bridge (Rust/webrtc-rs)
├── agent/              # Python agent framework + CLI (`chatixia` PyPI package)
│   ├── chatixia/       # CLI: init, run, validate, pair
│   ├── core/           # Mesh client, skill handlers
│   └── skills/         # Built-in mesh skill definitions
├── hub/                # Monitoring dashboard (React/Vite)
├── infra/              # nginx, coturn configs
└── docs/               # Architecture, components, ADRs
```

## Documentation

| Document                                   | Contents                                                       |
| ------------------------------------------ | -------------------------------------------------------------- |
| [COMPONENTS.md](docs/COMPONENTS.md)        | Detailed reference of every module, struct, route, and env var |
| [SYSTEM_DESIGN.md](docs/SYSTEM_DESIGN.md)  | Architecture, protocols, auth flows                            |
| [ADR.md](docs/ADR.md)                      | Architecture decision records                                  |
| [THREAT_MODEL.md](docs/THREAT_MODEL.md)    | Security analysis and mitigations                              |
| [GLOSSARY.md](docs/GLOSSARY.md)            | Domain terminology                                             |

## License

MIT
