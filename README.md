<p align="center">
  <strong>chatixia-mesh</strong><br/>
  The decentralized agent mesh
</p>

<p align="center">
  <a href="https://github.com/Chatixia-AI/chatixia-mesh/actions"><img src="https://img.shields.io/github/actions/workflow/status/Chatixia-AI/chatixia-mesh/ci.yml?branch=main&style=for-the-badge&label=CI" alt="CI status"></a>
  <a href="https://github.com/Chatixia-AI/chatixia-mesh/actions"><img src="https://img.shields.io/github/actions/workflow/status/Chatixia-AI/chatixia-mesh/pages.yml?branch=main&style=for-the-badge&label=Pages" alt="Pages status"></a>
  <a href="https://github.com/Chatixia-AI/chatixia-mesh/releases"><img src="https://img.shields.io/github/v/release/Chatixia-AI/chatixia-mesh?include_prereleases&style=for-the-badge" alt="GitHub release"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-MIT-blue.svg?style=for-the-badge" alt="MIT License"></a>
</p>

An agent-to-agent mesh network built on WebRTC. Agents discover each other through a registry, communicate directly over DTLS-encrypted peer-to-peer channels, and are monitored from a real-time dashboard. The registry handles signaling only — it never touches your data.

<p align="center">
  <a href="https://chatixia-ai.web.app?utm_source=github&utm_medium=readme">Website</a> ·
  <a href="https://chatixia-ai.github.io/chatixia-mesh">Documentation</a> ·
  <a href="docs/SYSTEM_DESIGN.md">Architecture</a> ·
  <a href="docs/COMPONENTS.md">Components</a> ·
  <a href="docs/ADR.md">ADRs</a> ·
  <a href="docs/ROADMAP.md">Roadmap</a>
</p>

---

## Why chatixia-mesh

| | chatixia-mesh | Centralized frameworks |
| --- | --- | --- |
| **Data path** | P2P — DTLS-encrypted DataChannels between agents | All traffic routed through a central server |
| **Agent runtime** | Sidecar pattern — WebRTC in Rust, agents write Python | Agents coupled to framework internals |
| **Deployment** | Self-hosted first — no external dependencies, on-prem ready | Cloud-dependent or SaaS |
| **Interop** | Open standards — Google A2A protocol, Anthropic MCP | Proprietary protocols |

CrewAI, AutoGen, and LangGraph route all agent traffic through a central server. chatixia-mesh doesn't.

## How it works

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

## Key subsystems

| Component | Description |
| --- | --- |
| **[Registry](registry/src/main.rs)** | Rust/axum signaling server, agent registry, task queue, and hub API |
| **[Sidecar](sidecar/src/main.rs)** | Rust/webrtc-rs mesh peer — WebRTC DataChannels + Unix socket IPC |
| **[Agent Framework](agent/chatixia/)** | Python package (`chatixia`) — CLI, skills, mesh client, LLM integration |
| **[Hub Dashboard](hub/src/App.tsx)** | React/Vite admin UI — agent health, approvals, task dispatch |

## Quick start

```bash
# Docker (recommended)
docker compose up --build

# Or install the CLI
uv tool install chatixia
```

### From source

**Prerequisites:** Rust 1.75+ · Python 3.12+ · Node.js 20+

```bash
# Rust (registry + sidecar)
cargo build --release

# Python agent framework
cd agent && uv pip install -e . && cd ..

# Hub dashboard
cd hub && npm install && npm run build && cd ..
```

### Run

```bash
# 1. Start the registry
cargo run --release -p chatixia-registry
# → Listening on 0.0.0.0:8080

# 2. Scaffold a new agent
chatixia init my-weather-bot
cd my-weather-bot
cp .env.example .env          # fill in your LLM provider keys

# 3. Pair with the mesh (get an invite code from an admin)
chatixia pair 482901

# 4. Run the agent
chatixia run

# 5. Open the Hub → http://localhost:8080
```

## Agent onboarding

chatixia-mesh uses an invite + approval flow to control who joins the network:

1. An admin generates a 6-digit invite code (via hub or API)
2. The new agent redeems the code: `chatixia pair <code>`
3. An admin approves the agent in the hub dashboard
4. The agent receives a device token and connects to the mesh

Default behavior: unapproved agents cannot connect. This can be relaxed per-deployment.

## CLI

| Command | Description |
| --- | --- |
| `chatixia init [name] [--role ...]` | Scaffold a new agent with optional role template |
| `chatixia run [manifest]` | Register, connect to mesh, heartbeat |
| `chatixia validate [manifest]` | Validate manifest and print summary |
| `chatixia pair <code> [manifest]` | Redeem invite code to join the mesh |
| `chatixia -V` | Show version |

## Agent manifest (`agent.yaml`)

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

## Protocol

| Layer | Transport | Format |
| --- | --- | --- |
| Signaling | WebSocket | JSON (SDP offers/answers, ICE candidates) |
| Data | WebRTC DataChannel (DTLS) | JSON `MeshMessage` |
| IPC | Unix socket | JSON lines |
| Registry API | HTTP REST | JSON |

## Project structure

```text
chatixia-mesh/
├── registry/           # Signaling + registry + hub API (Rust/axum)
├── sidecar/            # WebRTC mesh peer + IPC bridge (Rust/webrtc-rs)
├── agent/              # Python agent framework + CLI (chatixia PyPI package)
│   ├── chatixia/       # CLI: init, run, validate, pair
│   ├── core/           # Mesh client, skill handlers
│   └── skills/         # Built-in mesh skill definitions
├── hub/                # Monitoring dashboard (React/Vite)
├── site/               # GitHub Pages documentation site
├── infra/              # nginx, coturn configs
└── docs/               # Architecture, components, ADRs, threat model
```

## Documentation

| Document | Contents |
| --- | --- |
| [COMPONENTS.md](docs/COMPONENTS.md) | Detailed reference of every module, struct, route, and env var |
| [SYSTEM_DESIGN.md](docs/SYSTEM_DESIGN.md) | Architecture, protocols, auth flows |
| [ADR.md](docs/ADR.md) | Architecture decision records |
| [ROADMAP.md](docs/ROADMAP.md) | Product roadmap and competitive analysis |
| [THREAT_MODEL.md](docs/THREAT_MODEL.md) | Security analysis and mitigations |
| [GLOSSARY.md](docs/GLOSSARY.md) | Domain terminology |

## License

MIT
