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

**Decision:** Add a `chatixia` CLI (PyPI package) to the `agent/` directory with four subcommands: `init` (scaffold agent.yaml + .env.example + .gitignore), `run` (register + connect to mesh + heartbeat), `validate` (check manifest), and `pair` (redeem invite code via pairing API). Entry point: `chatixia.cli:main`.

**Consequences:**

- (+) Users scaffold a new agent with one command: `chatixia init my-agent`
- (+) `chatixia pair <code>` integrates with ADR-009 pairing flow — no manual API calls
- (+) `chatixia run` replaces `run_agent.py` with config-driven startup from `agent.yaml`
- (+) `chatixia validate` catches config errors before runtime
- (-) Package name `chatixia` on PyPI supersedes the old `chatixia-agent` SDK repo (now deprecated)

**Update (ADR-012):** `core` and `skills` moved under the `chatixia` namespace — no longer top-level packages.

---

## ADR-011: GitHub Pages Documentation Site

**Date:** 2026-03-21
**Status:** Accepted

**Context:** All project documentation lived in markdown files inside the `docs/` directory — useful for contributors reading the repo, but not discoverable or presentable for external users evaluating the system. A public-facing documentation site was needed without introducing a static site generator (Jekyll, Hugo, Docusaurus) or a separate build pipeline.

**Decision:** Create a single-page static HTML documentation site in `site/index.html` using the existing Atmospheric Luminescence design system (ADR-007). The site is deployed to GitHub Pages via a GitHub Actions workflow (`.github/workflows/pages.yml`) that uploads the `site/` directory directly — no build step required. Content covers architecture, quickstart, protocol layers, API reference, CLI, skills, threat model, glossary, and ADRs.

**Consequences:**

- (+) Zero build dependencies — plain HTML/CSS, no framework, no bundler
- (+) Design continuity with the hub dashboard (same tokens, fonts, glass effects)
- (+) GitHub Actions deployment is simple and free
- (+) Single file is easy to maintain and review in PR diffs
- (-) Content must be manually kept in sync with the markdown docs in `docs/`
- (-) No search, no multi-page navigation, no versioning (acceptable for current scale)

**Migration path:** If the site grows beyond a single page, consider adopting a static site generator (e.g., Astro or VitePress) that can consume the existing `docs/*.md` files as content sources.

---

## ADR-012: Consolidate Package Under `chatixia` Namespace

**Date:** 2026-03-21
**Status:** Accepted

**Context:** The `chatixia` PyPI package (ADR-010) shipped `core` and `skills` as top-level Python packages. These generic names would collide with any other installed package that also provides a `core` or `skills` module — a significant risk for users installing into shared virtualenvs.

**Decision:** Move `core/` → `chatixia/core/` and `skills/` → `chatixia/skills/`, making everything a subpackage of `chatixia`. The wheel now contains a single top-level package. All internal imports updated from `from core.…` to `from chatixia.core.…`. Published as `chatixia 0.2.0` to PyPI.

**Consequences:**

- (+) No namespace collisions — `chatixia` is the only top-level package
- (+) Standard Python packaging practice — single namespace for the project
- (+) `pip install chatixia` is safe in any environment
- (-) Breaking change for anyone importing `from core.mesh_client import MeshClient` directly (unlikely — only `0.1.0` was published, and it used a different internal layout from the old `chatixia-agent` repo)

---

## ADR-013: Heartbeat-Driven Task Execution

**Date:** 2026-03-21
**Status:** Accepted

**Context:** The registry assigns pending tasks to agents via the heartbeat response (`pending_tasks` array in the JSON body of `POST /api/hub/heartbeat`). However, the Python runner's heartbeat loop (`runner.py`) discards the response — it fires `requests.post(…)` and ignores the result. Tasks transition from `pending` → `assigned` server-side but are never executed. During E2E testing (Session 4), task completion had to be simulated via direct API calls.

**Decision:** Modify the runner's heartbeat loop to:

1. Parse `pending_tasks` from the heartbeat response
2. For each task, look up the matching built-in skill handler
3. Execute the handler (passing `task.payload` as parameters)
4. POST the result (or error) back to `POST /api/hub/tasks/{task_id}` with state `completed` or `failed`

Task execution runs inline in the heartbeat loop for simplicity. Long-running tasks can be moved to `asyncio.create_task` in a future iteration if needed.

**Consequences:**

- (+) Agents actually execute delegated tasks — closes the last gap in the task lifecycle
- (+) No new infrastructure — reuses existing heartbeat polling and hub task API
- (+) Simple implementation — skill handlers are already synchronous functions
- (-) Heartbeat interval (~15s) bounds task pickup latency
- (-) Inline execution blocks the heartbeat loop during skill execution — acceptable for fast skills, needs async dispatch for slow ones

---

## ADR-014: Git History Rewrite for Public Release

**Date:** 2026-03-22
**Status:** Accepted

**Context:** The repository was originally a private fork of an internal procurement assistant (ProcX/chatixia-agent). The initial commits contained sensitive files that must not be exposed when the repo goes public: real SAP Ariba purchase requisition data (`ariba_prs.md`), a production Postgres schema dump (`docs/postgres_schema_snapshot.json`), local filesystem paths in `.mcp.json`, BLE device MAC addresses (`deploy/pi-agent/devices.yaml`), a third-party PDF, and an internal context document (`CONTEXT.md`).

**Decision:** Use `git filter-repo --invert-paths` to permanently remove the 6 sensitive files from all commits across all branches, then force-push the rewritten history. This approach was chosen over a fresh squash to preserve the meaningful commit history of the monorepo era (commits `7b6afc1` onward).

**Consequences:**

- (+) All sensitive business data, local paths, and device identifiers are permanently removed from git history
- (+) Commit history from the monorepo era is fully preserved
- (+) Repository is safe for public visibility
- (-) Force-push rewrites remote history — any existing clones or forks are invalidated
- (-) Old commit hashes from the pre-monorepo lineage are changed (GitHub PR/issue references to those hashes will break)

---

## ADR-015: Docker Compose Deployment

**Date:** 2026-03-22
**Status:** Accepted

**Context:** Running chatixia-mesh requires starting multiple components (registry, sidecar, agent, hub) with correct environment variables, network connectivity, and startup ordering. The README tells users to run 3+ separate commands in different terminals. There is no single-command way to start the full stack, which makes onboarding and testing painful.

**Decision:** Add Docker Compose as the primary deployment method (Roadmap item 0.5). Structure:

- **Registry Dockerfile** — multi-stage: Node.js builds hub static assets, Rust builds the registry binary, final image is `debian:bookworm-slim` with both.
- **Sidecar Dockerfile** — multi-stage Rust build, same workspace caching pattern.
- **Agent Dockerfile** — `python:3.13-slim-bookworm` with `pip install .` of the chatixia package.
- **docker-compose.yml** — wires registry, sidecar, agent together. Sidecar ↔ agent share an IPC socket via a named volume. Registry health check gates sidecar/agent startup. coturn available via `--profile turn`.
- **`HUB_DIST_DIR` env var** — registry's static file serving path is now configurable (was hardcoded to `hub/dist`). Docker sets it to `/srv/hub`; local dev is unchanged.

**Consequences:**

- (+) `docker compose up --build` starts the entire stack — zero manual setup
- (+) Service dependencies enforced via health checks — no startup race conditions
- (+) IPC socket shared via volume — sidecar and agent don't need to be in the same container
- (+) coturn is opt-in via `--profile turn` — not required for local development
- (+) Hub assets built inside the registry image — no separate build step needed
- (-) First build is slow (~5 min) due to Rust compilation; subsequent builds use Docker layer cache
- (-) Agent container does not include the sidecar binary — agent cannot spawn sidecar itself in Docker mode (sidecar runs as a separate service)

---

## ADR-016: P2P Task Execution via DataChannels

**Date:** 2026-03-22
**Status:** Accepted

**Context:** Despite the system's P2P architecture (WebRTC DataChannels between sidecars), all agent-to-agent task execution routed through the registry's REST API task queue (ADR-005, ADR-013). The `delegate`, `mesh_send`, and `mesh_broadcast` skill handlers used synchronous HTTP calls to `POST /api/hub/tasks`, and the target agent picked up tasks on its next heartbeat poll (~15s). This contradicted the core positioning: "registry is control plane only, agents talk directly."

**Decision:** Route task delegation and messaging through WebRTC DataChannels with automatic fallback to the registry task queue:

1. **Sidecar emits peer lifecycle events** — `peer_connected`, `peer_disconnected`, `peer_list` IPC messages to the Python agent (protocol types already defined but never sent).
2. **MeshClient tracks connected peers** — maintains a local peer set from sidecar events.
3. **Skill handlers are async with P2P-first path** — `handle_delegate` sends `task_request` via `MeshClient.request()` (send + await `task_response` matched by `request_id`); `handle_mesh_send` and `handle_mesh_broadcast` send `agent_prompt` messages via `MeshClient.send()`/`broadcast()`.
4. **Runner registers P2P task handler** — incoming `task_request` messages via DataChannel are dispatched to skill handlers, with `task_response` sent back via DataChannel.
5. **Non-blocking task execution** — `asyncio.create_task()` in the heartbeat loop replaces synchronous inline execution.
6. **Registry fallback preserved** — if the target peer is not directly connected (no DataChannel), the handler falls back to the existing HTTP task queue path.

Discovery (`list_agents`, `find_agent`) remains HTTP to the registry — that is legitimate control plane.

**Consequences:**

- (+) Agent-to-agent data flows directly over DTLS-encrypted DataChannels — registry is truly out of the data path
- (+) Sub-second task delegation latency (vs. 3–15s with heartbeat polling)
- (+) Connected agents keep working if the registry goes down (P2P resilience)
- (+) Async handlers no longer block the heartbeat loop
- (+) Backward compatible — HTTP fallback preserves behavior when P2P path is unavailable
- (-) Discovery still requires the registry — agents can't find new peers without it
- (-) HTTP fallback path still uses synchronous urllib (acceptable since it's the backup path)
