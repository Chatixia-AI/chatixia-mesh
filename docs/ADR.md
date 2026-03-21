# Architecture Decision Records

## ADR-001: Rust Sidecar Pattern for WebRTC

**Date:** 2026-03-21
**Status:** Accepted

**Context:** Python agents need to communicate over WebRTC DataChannels, but the Python WebRTC ecosystem (aiortc) is fragile, hard to debug, and lacks production-grade DTLS support.

**Decision:** Each Python agent spawns a Rust sidecar process that handles all WebRTC/signaling complexity. The agent communicates with its sidecar via a Unix domain socket using a simple JSON-line protocol.

**Consequences:**

- (+) Robust WebRTC via webrtc-rs (well-maintained, DTLS built-in)
- (+) Python agents stay simple — no WebRTC dependencies
- (+) Sidecar can be reused for agents in other languages
- (-) Extra process per agent; slightly more complex deployment
- (-) IPC adds a small latency hop (~1ms)

---

## ADR-002: Full Mesh Topology

**Date:** 2026-03-21
**Status:** Accepted

**Context:** Agents need to communicate with each other. Options: star (through registry), partial mesh (topic-based), or full mesh.

**Decision:** Full mesh — every sidecar connects to every other sidecar via WebRTC DataChannels. Registry only handles signaling.

**Consequences:**

- (+) Direct P2P: lowest latency, no single point of failure for data
- (+) Simple routing — any agent can reach any other agent directly
- (-) O(N²) connections — practical limit of ~50 agents
- (-) Every new agent triggers N-1 WebRTC negotiations

**Migration path:** If N grows beyond 50, switch to selective mesh with topic-based routing.

---

## ADR-003: JWT for WebSocket Authentication

**Date:** 2026-03-21
**Status:** Accepted

**Context:** Sidecars connect to the registry via WebSocket for signaling. Need to authenticate the connection and bind it to a peer identity.

**Decision:** API key → JWT exchange. Sidecar sends API key via `POST /api/token`, receives a short-lived JWT (5-min TTL). JWT is passed as a query parameter on WebSocket upgrade. Registry validates JWT and extracts `peer_id` from claims.

**Consequences:**

- (+) Stateless verification — no session table needed
- (+) Short TTL limits exposure if token leaks
- (+) Sender verification: JWT `sub` must match message `peer_id`
- (-) API keys stored in a JSON file — not suitable for production (should use a secrets manager)

---

## ADR-004: In-Memory State (No Database)

**Date:** 2026-03-21
**Status:** Accepted

**Context:** Registry needs to track agents, tasks, and signaling peers. Options: database (PostgreSQL, Redis) or in-memory (DashMap).

**Decision:** All state is in-memory using `DashMap` (concurrent hash maps). No database dependency.

**Consequences:**

- (+) Zero deployment complexity — single binary, no external services
- (+) Very fast reads/writes
- (-) No durability — restart loses all state
- (-) Single-instance only (no horizontal scaling)

**Migration path:** Add PostgreSQL for task queue and agent registry when persistence or multi-instance is needed.

---

## ADR-005: Hub Task Queue for Sync Skill Handlers

**Date:** 2026-03-21
**Status:** Accepted

**Context:** Python skill handlers are synchronous (called by the LLM tool-use loop). The async `MeshClient` IPC bridge can't be used directly in sync handlers. Need a way for sync code to delegate tasks.

**Decision:** Sync skill handlers (e.g., `handle_delegate`, `handle_mesh_send`) submit tasks via the registry's hub REST API instead of going through the sidecar IPC. The target agent picks up tasks on its next heartbeat.

**Consequences:**

- (+) Works from synchronous Python code
- (+) Centralized task queue with status tracking
- (-) Higher latency than direct DataChannel (poll-based, ~3s intervals)
- (-) Bypasses P2P for these operations (goes through registry)

**Migration path:** Once the agent framework supports async skill handlers, route through the sidecar DataChannel directly.

---

## ADR-006: Ephemeral TURN Credentials

**Date:** 2026-03-21
**Status:** Accepted

**Context:** Agents behind symmetric NATs need a TURN relay for WebRTC. Static TURN credentials are a security risk.

**Decision:** Use coturn's `use-auth-secret` mode. The registry generates ephemeral credentials via HMAC-SHA1 with a 24-hour TTL, served via `GET /api/config`.

**Consequences:**

- (+) No long-lived TURN credentials
- (+) Standard coturn mechanism — well-documented
- (-) Requires shared secret between registry and coturn
- (-) Agents must refresh ICE config periodically (currently not implemented)

---

## ADR-007: Atmospheric Luminescence UI Design System

**Date:** 2026-03-21
**Status:** Accepted

**Context:** The hub dashboard used a dark, monospace (JetBrains Mono) hacker-terminal theme with explicit 1px borders and flat fills. The project needed a premium, editorial-quality UI that communicates the sophistication of the mesh network rather than a raw dev console.

**Decision:** Adopt the "Atmospheric Luminescence" design system (`docs/DESIGN.md`) — a visionOS-inspired, light-mode glassmorphic design. Key pillars: (1) tonal surface layering instead of borders ("No-Line Rule"), (2) Space Grotesk + Manrope typography pairing, (3) frosted glass surfaces via `backdrop-filter: blur()`, (4) Electric Cyan gradient primary accent, (5) ambient luminance shadows instead of drop shadows. All tokens centralized in `hub/src/theme.ts`.

**Consequences:**

- (+) Premium, architectural feel — differentiated from standard dashboard templates
- (+) Centralized design tokens make future theming straightforward
- (+) Generous spacing and glass effects improve visual hierarchy and scannability
- (-) `backdrop-filter` has a performance cost on low-end devices / older browsers
- (-) Light-mode only — no dark mode variant yet
- (-) Google Fonts dependency for typography (could be self-hosted later)

---

## ADR-008: Clean Agent Deregistration on Shutdown

**Date:** 2026-03-21
**Status:** Accepted

**Context:** When an agent process is killed (Ctrl+C), it remained "active" on the hub dashboard until the registry's health check loop marked it stale (90s) then offline (270s). This was confusing — dead agents appeared alive for minutes.

**Decision:** Added `DELETE /api/registry/agents/{agent_id}` to the registry. The agent runner (`run_agent.py`) installs signal handlers for SIGINT/SIGTERM that call this endpoint before exiting. The stale/offline health states remain as a fallback for hard crashes (SIGKILL, OOM, network loss) where the shutdown handler cannot run.

**Consequences:**

- (+) Clean shutdown removes agent from dashboard instantly
- (+) Stale/offline health check still catches hard crashes
- (-) Agents that crash without clean shutdown still linger for up to 270s

---

## ADR-009: Hybrid Pairing + Approval for Agent Onboarding

**Date:** 2026-03-20
**Status:** Accepted

**Context:** The original rust-p2p had a device pairing system (6-digit codes, device tokens, revocation) that was dropped when merging into chatixia-mesh. The mesh currently uses static API keys with no dynamic onboarding, no approval flow, no scoped visibility, and no revocation. New agents require manual `api_keys.json` edits and a restart.

**Decision:** Implement a hybrid system combining pairing codes (from rust-p2p) with admin approval (via hub dashboard). Flow: existing agent generates invite code → new agent redeems code → status is "pending_approval" → admin approves in hub → agent receives device token → agent can exchange token for JWT and join mesh. Signaling filters peer_list to only show approved + legacy API-key peers.

**Consequences:**

- (+) Dynamic agent onboarding without editing config files or restarting
- (+) Admin oversight — new agents require explicit approval before mesh access
- (+) Scoped peer visibility — pending agents can't see or communicate with mesh peers
- (+) Revocation — approved agents can be revoked, immediately losing mesh access
- (+) Backward compatible — legacy API-key agents are auto-approved
- (-) In-memory state — pairing data lost on registry restart (matches existing pattern)
- (-) No push notification on approval — agent must poll `/api/token`
- (-) Dashboard admin endpoints are unauthenticated (matches existing hub API pattern)

---

## ADR-010: `chatixia` CLI for Agent Scaffolding and Lifecycle

**Date:** 2026-03-21
**Status:** Accepted

**Context:** Creating a new agent required manually copying `agent.yaml.example`, editing `.env`, and running `python run_agent.py`. There was no standard onboarding path for external users — they had to understand the monorepo internals, env vars, and sidecar setup before they could run anything.

**Decision:** Add a `chatixia` CLI (PyPI package) to the `agent/` directory with four subcommands: `init` (scaffold agent.yaml + .env.example + .gitignore), `run` (register + connect to mesh + heartbeat), `validate` (check manifest), and `pair` (redeem invite code via pairing API). The package ships `chatixia`, `core`, and `skills` as top-level Python packages. Entry point: `chatixia.cli:main`.

**Consequences:**

- (+) Users scaffold a new agent with one command: `chatixia init my-agent`
- (+) `chatixia pair <code>` integrates with ADR-009 pairing flow — no manual API calls
- (+) `chatixia run` replaces `run_agent.py` with config-driven startup from `agent.yaml`
- (+) `chatixia validate` catches config errors before runtime
- (-) Package name `chatixia` on PyPI supersedes the old `chatixia-agent` SDK repo (now deprecated)
- (-) `core` and `skills` are top-level packages — could collide in large virtualenvs (acceptable for now)
