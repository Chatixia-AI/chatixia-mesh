# chatixia-mesh

Agent-to-agent mesh network with WebRTC DataChannels.

## Architecture

- **registry/** — Rust (axum): signaling server + agent registry + hub API (port 8080)
- **sidecar/** — Rust (webrtc-rs): WebRTC mesh peer + IPC bridge to Python agent
- **agent/** — Python (`chatixia` PyPI package): AI agent framework + CLI (skills, mesh client, scaffolding)
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

# 2. Scaffold and run a new agent
chatixia init my-agent
cd my-agent
cp .env.example .env          # fill in credentials
chatixia pair <invite-code>   # get code from admin
chatixia run

# 3. Hub dashboard (dev)
cd hub && npm run dev
```

## Key files

| Component         | Entry point                 |
| ----------------- | --------------------------- |
| Registry          | `registry/src/main.rs`      |
| Sidecar           | `sidecar/src/main.rs`       |
| CLI               | `agent/chatixia/cli.py`     |
| Agent mesh client | `agent/core/mesh_client.py` |
| Mesh skills       | `agent/core/mesh_skills.py` |
| Hub dashboard     | `hub/src/App.tsx`           |

## Protocol

- **Signaling**: JSON over WebSocket (SDP offers/answers, ICE candidates)
- **DataChannel**: JSON `MeshMessage` (task_request, task_response, agent_prompt, etc.)
- **IPC**: JSON lines over Unix socket (send, broadcast, message events)
- **Registry API**: REST (agent registration, skill routing, task queue)

## Documentations

**Read `docs/COMPONENTS.md` first** when starting a new session. It contains a detailed reference of every component, module, file, struct, route, and env var in the system. This is your map of the codebase.

All documentation lives in the `docs/` folder. Always log decisions and keep docs in sync with code changes.

| Document                | When to read                                                       | When to update                                                                                                                       |
| ----------------------- | ------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ |
| `docs/COMPONENTS.md`    | Start of every session — your codebase map                         | When adding/removing files, modules, routes, env vars, skills, or config                                                             |
| `docs/SYSTEM_DESIGN.md` | When you need to understand architecture, protocols, or auth flows | When changing architecture, protocols, authentication, infrastructure, or component responsibilities                                 |
| `docs/ADR.md`           | When you need context on past decisions                            | When making a new architectural decision — add a new ADR entry                                                                       |
| `docs/GLOSSARY.md`      | When encountering unfamiliar domain terms                          | When introducing new domain-specific terms — append as a markdown table row                                                          |
| `docs/THREAT_MODEL.md`  | When working on auth, security, or network-facing code             | When adding new attack surfaces, mitigations, or security-relevant changes                                                           |
| `docs/meetings/`        | For context on prior session work                                  | After every session — create `yyyy_mm_dd_S<session_number>.md` with summary, decisions, next actions, and items needing human review |
