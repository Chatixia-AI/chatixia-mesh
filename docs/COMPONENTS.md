# Components Reference

> Comprehensive codebase map — read this first when starting a new session.

## Directory Structure

```
chatixia-mesh/
├── registry/           # Rust (axum): signaling + registry + hub API
├── sidecar/            # Rust (webrtc-rs): WebRTC mesh peer + IPC bridge
├── agent/              # Python: AI agent framework
├── hub/                # React (Vite): monitoring dashboard
├── infra/              # Nginx + coturn configs
├── site/               # GitHub Pages documentation site
├── docs/               # Documentation
├── .github/workflows/  # CI: GitHub Pages deployment
├── Cargo.toml          # Workspace manifest (registry + sidecar)
├── .env.example        # All environment variables
└── agent.yaml.example  # Agent configuration template
```

---

## Registry (`registry/`)

Rust crate — signaling server, agent registry, and hub API. Port 8080.

### Modules

| File | Purpose |
|------|---------|
| `src/main.rs` | Entry point, route setup, WebSocket upgrade handler, `AppState` |
| `src/auth.rs` | API key → JWT exchange, ICE config endpoint, TURN credential generation |
| `src/signaling.rs` | WebSocket relay for SDP/ICE messages, peer tracking |
| `src/registry.rs` | Agent registration, discovery, health checks, skill routing |
| `src/hub.rs` | Task queue (submit, poll, update, expire), task lifecycle |
| `src/pairing.rs` | Agent pairing + approval: invite codes, onboarding pipeline, revocation |
| `src/topology.rs` | Mesh topology endpoint for dashboard visualization |

### Key Structs

| Struct | Module | Description |
|--------|--------|-------------|
| `AppState` | `main` | Shared state: `Arc<AuthState>`, `Arc<SignalingState>`, `Arc<RegistryState>`, `Arc<HubState>`, `Arc<PairingState>` |
| `Claims` | `auth` | JWT claims: `sub` (peer_id), `role`, `exp`, `iat` |
| `ApiKeyEntry` | `auth` | API key mapping: `peer_id`, `role` |
| `AuthState` | `auth` | JWT secret + API key store (`RwLock<HashMap>`) |
| `SignalingMessage` | `signaling` | WebSocket message: `type`, `peer_id`, `target_id`, `payload` |
| `SignalingState` | `signaling` | Peer sender map (`DashMap<String, UnboundedSender>`) |
| `AgentInfo` | `registry` | Registration payload: `agent_id`, `hostname`, `ip`, `port`, `sidecar_peer_id`, `capabilities` |
| `AgentCapabilities` | `registry` | Skills list, MCP servers, goals count, mode |
| `AgentRecord` | `registry` | `AgentInfo` + `health`, `registered_at`, `last_heartbeat` |
| `Heartbeat` | `registry` | Heartbeat payload: agent metadata + `skill_names`, `uptime_seconds` |
| `RegistryState` | `registry` | Agent store (`DashMap<String, AgentRecord>`) |
| `TaskSubmission` | `hub` | Task creation: `skill`, `target_agent_id`, `source_agent_id`, `payload`, `ttl` |
| `TaskUpdate` | `hub` | Task update: `state`, `result`, `error` |
| `Task` | `hub` | Full task record with lifecycle timestamps |
| `HubState` | `hub` | Task store (`DashMap<String, Task>`) |
| `InviteCode` | `pairing` | Ephemeral 6-digit invite code with TTL |
| `OnboardingEntry` | `pairing` | Agent lifecycle: id, agent_name, peer_id, device_token, status (pending_approval→approved→revoked) |
| `PairingState` | `pairing` | Invite codes (`DashMap`), onboarding entries (`DashMap`), rate limiter (`DashMap`) |
| `TopologyNode` | `topology` | Agent node for visualization: position, peer ID, skills count, mesh peers |
| `TopologyResponse` | `topology` | `nodes` + `mesh_edges` |
| `MeshEdge` | `topology` | Edge between two sidecar peer IDs |

### Routes

```
POST /api/token                      # Exchange API key for JWT (auth.rs)
GET  /ws?token=...                   # WebSocket upgrade (main.rs → signaling)
GET  /api/registry/agents            # List all agents (registry.rs)
POST /api/registry/agents            # Register/update agent (registry.rs)
GET    /api/registry/agents/{agent_id} # Get specific agent (registry.rs)
DELETE /api/registry/agents/{agent_id} # Unregister agent (registry.rs)
GET    /api/registry/route?skill=...   # Find agent by skill (registry.rs)
POST /api/hub/tasks                  # Submit task (hub.rs)
GET  /api/hub/tasks/all              # List all tasks (hub.rs)
GET  /api/hub/tasks/{task_id}        # Get task status (hub.rs)
POST /api/hub/tasks/{task_id}        # Update task result (hub.rs)
POST /api/hub/heartbeat              # Agent heartbeat — upserts agent record (registry.rs)
GET  /api/hub/network/topology       # Mesh topology for visualization (topology.rs)
POST /api/pairing/generate-code       # Generate 6-digit invite code (pairing.rs)
POST /api/pairing/pair                # Redeem code, create pending entry (pairing.rs)
GET  /api/pairing/pending             # List pending approvals (pairing.rs)
GET  /api/pairing/all                 # List all onboarding entries (pairing.rs)
POST /api/pairing/{id}/approve        # Approve pending agent (pairing.rs)
POST /api/pairing/{id}/reject         # Reject pending agent (pairing.rs)
POST /api/pairing/{id}/revoke         # Revoke approved agent (pairing.rs)
GET  /api/config                      # ICE server config — STUN + optional TURN (auth.rs)
```

### Background Tasks

| Task | Interval | Logic |
|------|----------|-------|
| `health_check_loop` | 15s | Mark agents: active (<90s), stale (90–270s), offline (>270s) |
| `expire_tasks_loop` | 30s | Fail pending/assigned tasks whose TTL has elapsed |
| `cleanup_loop` (pairing) | 60s | Remove expired invite codes (>300s), prune rate-limit buckets |

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `SIGNALING_SECRET` | `dev-secret-change-me` | JWT signing secret |
| `API_KEYS_FILE` | `api_keys.json` | Path to API key definitions |
| `TURN_URL` | _(none)_ | Optional TURN server URL |
| `TURN_SECRET` | _(none)_ | Coturn shared secret for ephemeral credentials |
| `RUST_LOG` | `info` | Tracing filter |

---

## Sidecar (`sidecar/`)

Rust crate — one per Python agent. WebRTC mesh peer with IPC bridge.

### Modules

| File | Purpose |
|------|---------|
| `src/main.rs` | Entry point, token exchange, component wiring |
| `src/protocol.rs` | All message types: `SignalingMessage`, `MeshMessage`, `IpcMessage` |
| `src/signaling.rs` | WebSocket client, SDP/ICE relay, peer connection orchestration |
| `src/webrtc_peer.rs` | `RTCPeerConnection` creation, ICE forwarding, DataChannel setup |
| `src/mesh.rs` | `MeshManager` — tracks all peer connections and DataChannels |
| `src/ipc.rs` | Unix socket server, JSON-line protocol with Python agent |

### Key Structs

| Struct | Module | Description |
|--------|--------|-------------|
| `SignalingMessage` | `protocol` | WebSocket message to/from registry |
| `MeshMessage` | `protocol` | DataChannel application message: `type`, `request_id`, `source_agent`, `target_agent`, `payload` |
| `IpcMessage` | `protocol` | IPC message: `type`, `payload` |
| `MeshManager` | `mesh` | Central manager for peer connections and DataChannels |
| `MeshPeer` | `mesh` | Individual peer: `peer_id`, `RTCPeerConnection`, `RTCDataChannel` |

### Message Type Constants

**DataChannel** (`protocol::mesh_types`):
`ping`, `pong`, `task_request`, `task_response`, `task_stream_chunk`, `skill_query`, `skill_response`, `agent_status`, `agent_prompt`, `agent_response`, `agent_stream_chunk`

**IPC** (`protocol::ipc_types`):
- Agent → Sidecar: `send`, `broadcast`, `connect`, `list_peers`
- Sidecar → Agent: `message`, `peer_connected`, `peer_disconnected`, `peer_list`

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `SIGNALING_URL` | `ws://localhost:8080/ws` | Registry WebSocket URL |
| `API_KEY` | `ak_dev_001` | API key for JWT exchange |
| `TOKEN_URL` | `http://localhost:8080/api/token` | Registry token endpoint |
| `IPC_SOCKET` | `/tmp/chatixia-sidecar.sock` | Unix socket path for agent IPC |
| `RUST_LOG` | `info` | Tracing filter |

---

## Python Agent (`agent/`)

AI agent framework with mesh networking. Published as the `chatixia` PyPI package.

### CLI (`chatixia`)

Installed via `pip install chatixia`. Entry point: `chatixia.cli:main`.

| Command | Description |
|---------|-------------|
| `chatixia init [name]` | Scaffold a new agent (`agent.yaml`, `.env.example`, `.gitignore`) |
| `chatixia run [manifest]` | Run agent — register, connect to mesh, heartbeat |
| `chatixia validate [manifest]` | Validate manifest and print summary |
| `chatixia pair <code> [manifest]` | Redeem 6-digit invite code to join a mesh network |
| `chatixia -V` | Show version |

### CLI Modules (`chatixia/`)

| File | Purpose |
|------|---------|
| `chatixia/__init__.py` | Package version |
| `chatixia/cli.py` | CLI entry point, argument parsing, subcommand dispatch |
| `chatixia/scaffold.py` | `chatixia init` — writes `agent.yaml`, `.env.example`, `.gitignore` templates |
| `chatixia/config.py` | `AgentConfig` dataclass, YAML manifest parser (`load_config`) |
| `chatixia/runner.py` | `chatixia run` — registers with registry, spawns sidecar, connects mesh, heartbeats |

### Core Modules

| File | Purpose |
|------|---------|
| `chatixia/core/__init__.py` | Core subpackage init |
| `chatixia/core/mesh_client.py` | `MeshClient` — async IPC bridge to sidecar, message dispatch, request/response correlation |
| `chatixia/core/mesh_skills.py` | Synchronous skill handlers: `delegate`, `list_agents`, `mesh_send`, `mesh_broadcast`, `find_agent` |
| `run_agent.py` | Legacy standalone agent runner (use `chatixia run` instead) |
| `.env` | Local env var defaults for agent runner (gitignored) |

### Key Classes

| Class | Module | Description |
|-------|--------|-------------|
| `AgentConfig` | `chatixia.config` | Dataclass: agent name, registry URL, sidecar config, LLM provider, skills, runtime settings |
| `SidecarConfig` | `chatixia.config` | Dataclass: `binary`, `api_key`, `socket` |
| `MeshMessage` | `chatixia.core.mesh_client` | Dataclass: `msg_type`, `request_id`, `source_agent`, `target_agent`, `payload` |
| `MeshClient` | `chatixia.core.mesh_client` | Async IPC client — spawns sidecar, connects to socket, dispatches messages, correlates request/response |

### Skills

| Skill | Handler | Description |
|-------|---------|-------------|
| `list_agents` | `handle_list_agents()` | List all agents via registry REST API |
| `delegate` | `handle_delegate()` | Submit task to hub queue, optionally route by skill, poll for result |
| `mesh_send` | `handle_mesh_send()` | Send direct message to agent via hub task queue |
| `mesh_broadcast` | `handle_mesh_broadcast()` | Broadcast to all active agents via hub task queue |
| `find_agent` | `handle_find_agent()` | Find best agent for a skill via registry route endpoint |

### Skill Definition Format (`chatixia/skills/*/skill.json`)

```json
{
  "name": "skill_name",
  "description": "...",
  "version": "1.0.0",
  "category": "Mesh",
  "parameters": {
    "param_name": {
      "type": "string",
      "description": "...",
      "required": true
    }
  }
}
```

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `REGISTRY_URL` | `http://localhost:8080` | Registry server URL (used by `run_agent.py`) |
| `AGENT_ID` | `agent-{hostname}` | Agent identifier (used by `run_agent.py`) |
| `API_KEY` | `ak_dev_001` | API key for registry auth |
| `SIGNALING_URL` | `ws://localhost:8080/ws` | Sidecar WebSocket URL (passed through to sidecar) |
| `TOKEN_URL` | `http://localhost:8080/api/token` | Sidecar token endpoint (passed through to sidecar) |
| `SIDECAR_BINARY` | `chatixia-sidecar` | Path to sidecar binary |
| `CHATIXIA_REGISTRY_URL` | `http://localhost:8080` | Registry URL (used by `mesh_skills.py`) |
| `CHATIXIA_AGENT_ID` | `my-agent` | Agent identifier (used by `mesh_skills.py`) |
| `LLM_PROVIDER` | `azure` | LLM backend: `azure`, `ollama`, `openai` |
| `AZURE_OPENAI_ENDPOINT` | — | Azure OpenAI endpoint |
| `AZURE_OPENAI_API_KEY` | — | Azure OpenAI API key |
| `AZURE_OPENAI_DEPLOYMENT` | — | Azure OpenAI deployment name |
| `AZURE_OPENAI_API_VERSION` | — | Azure OpenAI API version |
| `OLLAMA_URL` | `http://localhost:11434/v1` | Ollama endpoint |
| `AGENT_MODEL` | — | Model name for Ollama |
| `LOG_LEVEL` | `WARNING` | Python log level |

---

## Hub Dashboard (`hub/`)

React + Vite + TypeScript — real-time monitoring dashboard.

### Design System: Atmospheric Luminescence

Light-mode glassmorphic UI inspired by visionOS. See `docs/DESIGN.md` for full specification.

- **Typography:** Space Grotesk (headlines/labels), Manrope (body/utility) — loaded via Google Fonts in `index.html`
- **Surfaces:** Tonal layering with frosted glass (`backdrop-filter: blur(24–32px)`) instead of borders or shadows
- **Color:** Light canvas (`#f5f7f9`), Electric Cyan primary gradient (`#00647b` → `#00cffc` at 135°)
- **Boundaries:** "No-Line Rule" — background color shifts and ghost borders (`outline-variant` at 15% opacity), no `1px solid` borders
- **Elevation:** Ambient shadows only (40–64px blur at 4–8% opacity)
- **Spacing:** Generous gutters (`4rem` desktop), minimum `1.4rem` internal padding
- **Radii:** `rounded-lg` (2rem) for containers, `rounded-md` (1.5rem) for buttons/pills

### Source Files

| File | Purpose |
|------|---------|
| `src/theme.ts` | Centralized design tokens: colors, gradients, typography, spacing, radii, shadows, glass presets |
| `src/App.tsx` | Main layout — sticky glass header, stat cards grid, polling orchestration |
| `src/api.ts` | TypeScript API client — interfaces + fetch wrappers |
| `src/main.tsx` | React entry point |
| `src/components/AgentCards.tsx` | Glassmorphic agent cards grid with health indicators, tonal detail rows, pill badges |
| `src/components/TaskQueue.tsx` | Task list with spacing-based row separation (no divider lines), hover background shift, pill state badges |
| `src/components/NetworkTopology.tsx` | Canvas mesh visualization (gradient hub node, white circles with health dots, glassmorphic legend overlay) |
| `src/components/AgentChat.tsx` | Intervention interface — glassmorphic container, gradient primary CTA, focus-state input indicator |

### Design Tokens (`theme.ts`)

| Export | Contents |
|--------|----------|
| `color` | Surface hierarchy, primary/accent, semantic health colors, text colors |
| `gradient` | `primary` (signature 135° gradient), `primarySubtle` (low-opacity variant) |
| `font` | `display` (Space Grotesk), `body` (Manrope) |
| `radius` | `sm` (0.75rem), `md` (1.5rem), `lg` (2rem), `xl` (3rem) |
| `spacing` | Scale from `1` (0.25rem) to `12` (4rem) |
| `shadow` | `ambient`, `float`, `primaryGlow` |
| `glass` | `header` (60% opacity, 32px blur), `card` (80%, 24px), `overlay` (50%, 24px) |

### TypeScript Interfaces (`api.ts`)

| Interface | Fields |
|-----------|--------|
| `Agent` | `agent_id`, `hostname`, `ip`, `port`, `sidecar_peer_id`, `health`, `mode`, `status`, `capabilities` |
| `Task` | `id`, `skill`, `source_agent_id`, `target_agent_id`, `assigned_agent_id`, `state`, `result`, `error`, `created_at`, `updated_at`, `ttl` |
| `TopologyNode` | `agent_id`, `ip`, `port`, `hostname`, `sidecar_peer_id`, `mode`, `skills_count`, `health`, `mesh_peers` |
| `Topology` | `nodes`, `mesh_edges` |

### API Endpoints Consumed

```
GET  /api/registry/agents
GET  /api/hub/tasks/all
GET  /api/hub/network/topology
POST /api/hub/tasks
```

### Styling

Atmospheric Luminescence design system — light-mode glassmorphic. Inline CSS with centralized tokens from `theme.ts`. No external CSS framework. Fonts loaded from Google Fonts.

---

## Infrastructure (`infra/`)

| File | Purpose |
|------|---------|
| `coturn/turnserver.conf` | TURN relay config (port 3478, ephemeral credentials via `use-auth-secret`) |
| `nginx/signaling.conf` | Reverse proxy for registry — TLS termination, WebSocket upgrade |

---

## Configuration Files

| File | Purpose |
|------|---------|
| `.env.example` | All environment variables with defaults |
| `agent/.env` | Agent runner env vars — loaded by `python-dotenv` (gitignored) |
| `agent.yaml.example` | Agent configuration: name, registry URL, LLM provider, sidecar config, skills, goals |
| `api_keys.json` | API key → peer_id/role mappings (not committed — in `.gitignore`) |
| `Cargo.toml` | Rust workspace: members `registry`, `sidecar` |
| `hub/package.json` | Hub dependencies: React 19, Vite 6, TypeScript 5.7 |
| `agent/pyproject.toml` | Agent dependencies: openai, mcp, fastapi, psycopg, structlog, pyyaml |

---

## Documentation Site (`site/`)

Static GitHub Pages documentation site using the Atmospheric Luminescence design system.

| File | Purpose |
|------|---------|
| `site/index.html` | Single-page docs — architecture, quickstart, API, protocol, security, glossary, ADRs |

### Deployment

GitHub Actions workflow (`.github/workflows/pages.yml`) deploys the `site/` directory to GitHub Pages on push to `main` (when `site/**` files change) or manual dispatch.
