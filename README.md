<p align="center">
  <strong>chatixia-mesh</strong><br/>
  A working agent-to-agent network, and a course on how it's built
</p>

<p align="center">
  <a href="https://github.com/Chatixia-AI/chatixia-mesh/actions"><img src="https://img.shields.io/github/actions/workflow/status/Chatixia-AI/chatixia-mesh/ci.yml?branch=main&style=for-the-badge&label=CI" alt="CI status"></a>
  <a href="https://github.com/Chatixia-AI/chatixia-mesh/actions"><img src="https://img.shields.io/github/actions/workflow/status/Chatixia-AI/chatixia-mesh/pages.yml?branch=main&style=for-the-badge&label=Pages" alt="Pages status"></a>
  <a href="https://github.com/Chatixia-AI/chatixia-mesh/releases"><img src="https://img.shields.io/github/v/release/Chatixia-AI/chatixia-mesh?include_prereleases&style=for-the-badge" alt="GitHub release"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-MIT-blue.svg?style=for-the-badge" alt="MIT License"></a>
</p>

chatixia-mesh is a real, running agent-to-agent network built on WebRTC, taken apart in the open so you can learn how it works. Agents find each other through a registry, then talk directly over DTLS-encrypted peer-to-peer channels; the registry handles signaling only and never touches their messages.

Every part of it is written down: an 18-lesson course that walks through the system from first principles, 23 architecture decision records, and a threat model that lists what is still open.

<p align="center">
  <a href="https://blog.chatixia.net?utm_source=github&utm_medium=readme"><b>Start the course</b></a> ·
  <a href="https://chatixia-ai.github.io/chatixia-mesh">Documentation</a> ·
  <a href="docs/SYSTEM_DESIGN.md">Architecture</a> ·
  <a href="docs/ADR.md">Decisions</a> ·
  <a href="docs/THREAT_MODEL.md">Threat model</a> ·
  <a href="https://chatixia.net?utm_source=github&utm_medium=readme">chatixia.net</a>
</p>

## What this is, and what it isn't

- **A reference system to learn from.** About 6,000 lines of Rust and Python, small enough to read end to end, and the [course](https://blog.chatixia.net) (sources in [`learnings/`](learnings/)) teaches distributed systems through it: peer-to-peer networking, WebRTC, signaling, IPC, the sidecar pattern, threat modeling, deployment and testing.
- **The transport layer for chatixia-world**, Chatixia's creature world: the Rust sidecar and registry carry creature-to-creature traffic between machines. Since 2026-09-22 two world instances hold live conversations over sidecar DataChannels (ADR-021). What the world needs next is what gets built here (ADR-020).
- **Not a product, and not hardened for production.** There is no roadmap. The [threat model](docs/THREAT_MODEL.md#known-gaps) lists what is and is not covered: the registry now requires an admin token for pairing approval and a credential for every write (ADR-024), but read endpoints are open, there is no rate limiting and no native TLS. Run it on networks you trust, or treat it as a starting point.

---

## Design at a glance

| | chatixia-mesh | A centralized design |
| --- | --- | --- |
| **Data path** | P2P — DTLS-encrypted DataChannels between agents | All traffic routed through a central server |
| **Agent runtime** | Sidecar pattern — WebRTC in Rust, agents write Python | Agents coupled to framework internals |
| **Deployment** | Self-hosted — no external services required | Usually a hosted control plane |
| **Interop** | Open standards — WebRTC (ICE/DTLS/SCTP), JSON messages over DataChannels | Proprietary protocols |

Why these choices, and what they cost, is in the [ADRs](docs/ADR.md) and lesson 11, [Transport Comparison](learnings/11-transport-comparison.md).

## How it works

```text
┌──────────────┐     ┌──────────────┐     ┌──────────────┐
│  Agent (Py)  │     │  Agent (Py)  │     │  Agent (Py)  │
│ mesh skills  │     │ mesh skills  │     │ mesh skills  │
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
| **[Agent Framework](agent/chatixia/)** | Python package (`chatixia`) — CLI, six built-in mesh skills, mesh client (no LLM loop yet) |
| **[Hub Dashboard](hub/src/App.tsx)** | React/Vite admin UI — agent health, approvals, task dispatch |

## Quick start

### Docker (recommended)

```bash
docker compose up --build
# Hub dashboard → http://localhost:8080
```

### Install without Docker

**Prerequisites:** [Rust](https://rustup.rs/) · Python 3.12+ · [uv](https://docs.astral.sh/uv/)

```bash
# 1. Install the sidecar (Rust WebRTC peer — goes into ~/.cargo/bin/)
cargo install --git https://github.com/Chatixia-AI/chatixia-mesh chatixia-sidecar

# 2. Install the registry (Rust signaling server)
cargo install --git https://github.com/Chatixia-AI/chatixia-mesh chatixia-registry

# 3. Install the Python agent CLI
uv tool install chatixia
```

### Run

```bash
# 1. Start the registry (use PORT to change the default 8080)
chatixia-registry
# → Listening on 0.0.0.0:8080

# 2. Scaffold a new agent (creates a directory)
chatixia init my-weather-bot
cd my-weather-bot
cp .env.example .env          # fill in your LLM provider keys

# 3. Pair with the mesh (get an invite code from an admin)
chatixia pair 482901

# 4. Run the agent
chatixia run

# 5. Open the Hub → http://localhost:8080
```

### Multi-device setup

Run agents across multiple machines (e.g., laptop + Raspberry Pi). The registry runs on one machine; agents on each device point to it.

```bash
# ── Machine A (registry host) ─────────────────────────────
chatixia-registry                    # listens on 0.0.0.0:8080

chatixia init agent-a
cd agent-a
# edit agent.yaml → registry: "http://localhost:8080"
chatixia run

# ── Machine B (e.g., Raspberry Pi) ────────────────────────
# Install Rust + sidecar + chatixia CLI (same as above)

chatixia init agent-b
cd agent-b
# edit agent.yaml → registry: "http://<machine-a-ip>:8080"
# edit .env        → SIGNALING_URL=ws://<machine-a-ip>:8080/ws
#                    TOKEN_URL=http://<machine-a-ip>:8080/api/token
chatixia run
```

Both agents appear in the Hub dashboard and form a direct WebRTC DataChannel automatically.

### From source

For contributors or development:

**Prerequisites:** Rust 1.75+ · Python 3.12+ · Node.js 20+

```bash
# Rust (registry + sidecar)
cargo build --release

# Python agent framework
cd agent && uv pip install -e . && cd ..

# Hub dashboard
cd hub && npm install && npm run build && cd ..

# Run the registry
cargo run --release -p chatixia-registry
```

**Integration test** (Linux, needs [uv](https://docs.astral.sh/uv/)): builds the registry and sidecar, then runs a real registry and two real sidecars on localhost. They connect, exchange messages, re-dial after a SIGSTOP past the ICE consent timeout, and resolve offer glare. It takes about 40 seconds.

```bash
uvx pytest tests/integration -v
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
| `chatixia init [name] [-d dir]` | Scaffold a new agent (`agent.yaml`, `AGENT.md`, `.env.example`, `.gitignore`) |
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
  binary: chatixia-sidecar    # found in PATH after cargo install
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
│   └── chatixia/       # CLI (init, run, validate, pair), runner with SKILL_HANDLERS
│       └── core/       # Mesh client, skill handlers
├── hub/                # Monitoring dashboard (React/Vite)
├── site/               # GitHub Pages documentation site
├── tests/integration/  # Real registry + two real sidecars (pytest)
├── infra/              # nginx, coturn configs
└── docs/               # Architecture, components, ADRs, threat model
```

## Documentation

| Document | Contents |
| --- | --- |
| [COMPONENTS.md](docs/COMPONENTS.md) | Detailed reference of every module, struct, route, and env var |
| [SYSTEM_DESIGN.md](docs/SYSTEM_DESIGN.md) | Architecture, protocols, auth flows |
| [ADR.md](docs/ADR.md) | Architecture decision records |
| Roadmap | Retired 2026-04-10 (ADR-020). chatixia-mesh is the transport layer for chatixia-world; the old roadmap is archived in that repository. |
| [CURRICULUM.md](CURRICULUM.md) · [`learnings/`](learnings/) | The 18-lesson course, glossary and reading list, published at [blog.chatixia.net](https://blog.chatixia.net) |
| [THREAT_MODEL.md](docs/THREAT_MODEL.md) | Security analysis and mitigations |
| [GLOSSARY.md](docs/GLOSSARY.md) | Domain terminology |

## License

MIT
