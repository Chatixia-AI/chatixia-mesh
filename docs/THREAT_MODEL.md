# Threat Model

## System Boundaries

```
Internet / LAN
    │
    ├── Registry (port 8080) — HTTP + WebSocket
    ├── TURN relay (port 3478) — UDP/TCP
    │
    └── Agent hosts
        ├── Sidecar (WebRTC, IPC socket)
        └── Python agent (LLM, skills, IPC socket)
```

## Assets

| Asset | Sensitivity | Location |
|-------|------------|----------|
| Agent-to-agent messages | High | DataChannels (DTLS encrypted) |
| API keys | High | `api_keys.json` (local file), environment variables |
| JWT signing secret | Critical | `SIGNALING_SECRET` env var (random per run when unset, since 2026-09-29) |
| Registry admin token | Critical | `REGISTRY_ADMIN_TOKEN` env var (random per run and logged once when unset); hub tab `sessionStorage` |
| Device tokens | High | Registry memory; returned by approval and by `/api/pairing/{id}/status` to the holder of the pairing secret |
| TURN shared secret | High | `TURN_SECRET` env var |
| Task payloads | Medium–High | In-memory on registry (unencrypted) |
| Agent capabilities/skills | Low | Broadcast via registry API |

## Threat Categories

### T1: Unauthorized Signaling Access

**Attack:** An attacker connects to `/ws` without valid credentials and injects SDP/ICE messages to redirect or intercept WebRTC connections.

**Mitigations:**
- JWT required for WebSocket upgrade (`ws?token=...`)
- JWT validated on upgrade; invalid tokens rejected with 401
- Sender verification: JWT `sub` must match message `peer_id`
- Since 2026-09-29 (G2, ADR-024): `offer`, `answer` and `ice_candidate` are relayed only when both sender and target are approved or legacy (API-key) peers; anything else is dropped and logged
- Since 2026-09-29: with `SIGNALING_SECRET` unset the registry signs JWTs with a random per-run secret instead of the public default `dev-secret-change-me`, so JWTs cannot be forged for known legacy peer ids

**Residual risk:** JWT is passed as a query parameter (visible in server logs, browser history). Consider moving to a WebSocket subprotocol or first-message auth.

### T2: API Key Compromise

**Attack:** Leaked API key allows an attacker to obtain a JWT, connect as a legitimate peer, and inject messages.

**Mitigations:**
- API keys map to specific `peer_id` + `role` — attacker can only impersonate one identity
- JWT TTL is 5 minutes — short window
- API keys loaded from file, not hardcoded

**Residual risk:** No key rotation mechanism. No rate limiting on `/api/token`. Development default key (`ak_dev_001`) must be changed in production.

### T3: Man-in-the-Middle on DataChannels

**Attack:** Intercept or modify agent-to-agent P2P traffic.

**Mitigations:**
- WebRTC DataChannels are DTLS-encrypted by default
- DTLS certificates are self-signed per-peer (fingerprints exchanged via signaling)

**Residual risk:** If the signaling path is compromised (T1), an attacker could perform a DTLS downgrade or fingerprint substitution. No certificate pinning or out-of-band verification.

### T4: Registry Denial of Service

**Attack:** Flood the registry with connections, registrations, or task submissions to prevent legitimate agents from operating.

**Mitigations:**
- Since 2026-09-29 (G3, ADR-024): memory no longer grows without bound over time. Finished tasks are evicted after `REGISTRY_TASK_RETENTION_SECS` (1 h), agents silent longer than `REGISTRY_AGENT_EVICTION_SECS` (1 h) are removed, rejected/revoked onboarding entries go after `REGISTRY_ONBOARDING_RETENTION_SECS` (24 h), and stale pairing rate-limit buckets are pruned
- Since 2026-09-29: registering agents, heartbeats and task submission need a credential (admin token, API key or device token), so an anonymous client can no longer fill the maps

**Residual risk:** No rate limiting and no connection limits. A credential holder can still flood the task queue inside the retention window.

**Recommended mitigations:**
- Add rate limiting per API key / IP (e.g., tower-governor)
- Limit max WebSocket connections
- Limit task queue size per source agent

### T5: Task Queue Poisoning

**Attack:** Submit malicious tasks that cause target agents to execute harmful skills or consume resources.

**Mitigations:**
- Tasks are assigned based on skill matching — agents only receive tasks for skills they advertise
- TTL limits task lifetime (default 300s)

**Residual risk:** No input validation on task payloads. No authorization check on who can submit tasks to whom. Any authenticated agent (or the hub UI) can submit tasks to any other agent.

**Recommended mitigations:**
- Add per-agent task submission ACLs
- Validate task payloads against skill parameter schemas
- Rate limit task submissions per source agent

### T6: IPC Socket Hijacking

**Attack:** A local process connects to the Unix socket (`/tmp/chatixia-sidecar.sock`) and sends commands to the sidecar, impersonating the Python agent.

**Mitigations:**
- Unix socket in `/tmp` — protected by filesystem permissions (owner-only)
- Sidecar accepts only one connection (first client wins)

**Residual risk:** `/tmp` is world-readable on some systems. Socket path is predictable.

**Recommended mitigations:**
- Use a socket in a non-world-accessible directory (e.g., `$XDG_RUNTIME_DIR`)
- Set strict file permissions (0600) on socket creation
- Authenticate the IPC connection (shared token)

### T7: Skill Injection via LLM

**Attack:** A malicious agent sends a crafted task payload that, when processed by the target agent's LLM, causes it to execute unintended skills (e.g., `shell` commands).

**Mitigations:**
- Skills have defined parameter schemas
- The `shell` skill (if enabled) should have allowlists

**Residual risk:** This is a prompt injection attack vector. The agent framework must sanitize task payloads before passing them to the LLM context.

### T8: Unauthorized Agent Deregistration

**Attack:** An attacker calls `DELETE /api/registry/agents/{agent_id}` to remove a legitimate agent from the registry, causing it to disappear from the dashboard and stop receiving tasks.

**Mitigations:**
- ~~None currently — DELETE endpoint is unauthenticated~~ (until 2026-09-29)
- Since 2026-09-29 (ADR-024): DELETE needs a caller credential: the admin token (hub) or a valid API key / device token (an agent deregistering itself on shutdown, which already sent `x-api-key`)

**Residual risk:** The registry cannot bind an `agent_id` to a credential (the runner's `agent_id` is independent of the API key's `peer_id`), so any mesh member can still deregister any agent by ID. The agent re-registers on its next heartbeat (~15s).

**Recommended mitigations:**
- Bind `agent_id` to the credential's `peer_id` at registration and allow self-deregister only

### T9: Information Disclosure via Registry API

**Attack:** Query registry endpoints to enumerate all agents, their skills, IPs, and topology.

**Mitigations:**
- Since 2026-09-29 (ADR-024): the pairing listings (`/api/pairing/pending`, `/api/pairing/all`, which include device tokens) need the admin token
- The CORS allowlist (`REGISTRY_ALLOWED_ORIGINS`) stops other websites from reading registry responses through a visitor's browser

**Residual risk:** The other GET endpoints (agents, route, tasks, topology, config) stay unauthenticated; anyone who can reach the registry directly can enumerate agents and read task payloads.

**Recommended mitigations:**
- Require JWT for all registry API endpoints (not just WebSocket)
- Add role-based access (e.g., only `hub` role can query topology)

### T8: Pairing Code Brute Force

**Attack:** An attacker brute-forces 6-digit invite codes to join the mesh without authorization.

**Mitigations:**
- Rate limiting: 5 pairing attempts per IP per 60 seconds
- Codes are single-use (consumed on first valid redemption)
- Codes expire after 300 seconds (5 minutes)
- Successful code redemption only creates a "pending_approval" entry — admin must still approve

**Residual risk:** 6-digit code space (1M possibilities) is small. The 5-per-minute rate limit makes brute force impractical within the 5-minute TTL (~25 attempts max), but a targeted attacker with many IPs could attempt more. Consider longer codes or CAPTCHA for higher-security deployments.

### T9: Unauthorized Approval of Pending Agents

**Attack:** An attacker calls `POST /api/pairing/{id}/approve` to approve their own pending agent without admin authorization.

**Mitigations:**
- ~~None currently — dashboard API endpoints are unauthenticated~~ (until 2026-09-29)
- Since 2026-09-29 (G1, ADR-024): approve, reject and revoke need the registry admin token (`x-admin-token`), compared in constant time. With `REGISTRY_ADMIN_TOKEN` unset the registry generates a random token and logs it once, so there is no open default
- Approve/reject/revoke are logged with the entry id and peer id; rejected admin calls are logged with method and path

**Residual risk:** One shared admin token, no per-user identity or rotation beyond a restart with a new value. The token sits in the hub tab's `sessionStorage`, so XSS in the hub would expose it.

**Recommended mitigations:**
- Per-user admin sessions with expiry
- Persist an audit log of approval actions

### T10: Device Token Theft

**Attack:** An attacker steals a device token (`dt_` + 32 hex) from a paired agent and uses it to impersonate that agent.

**Mitigations:**
- Device tokens are 128-bit random (infeasible to guess)
- Tokens are returned only to the admin at approval time and to the pairing device via `/api/pairing/{id}/status`, which needs the 256-bit pairing secret handed out by `/pair` (before 2026-09-29 anyone could read every token from `/api/pairing/all`)
- Revocation immediately invalidates the token

**Residual risk:** If the token is intercepted in transit (approval response) or leaked from the agent's storage, it can be used until revoked. TLS on the registry would mitigate in-transit theft.

### T11: WebRTC Protocol Stack Attack Surface

**Attack:** Exploit vulnerabilities in the ICE, STUN/TURN, DTLS, or SCTP layers of the WebRTC stack.

**Context:** The WebRTC data path uses four protocol layers (ICE → STUN/TURN → DTLS → SCTP), each with its own implementation and attack surface. By comparison, HTTP/gRPC uses only TLS. See [WEBRTC_VS_ALTERNATIVES.md §5.10](WEBRTC_VS_ALTERNATIVES.md) for full analysis.

**Known vulnerabilities:**

- DTLS ClientHello race condition (DoS) — affected Asterisk, RTPEngine, FreeSWITCH
- TURN server misconfiguration — open TURN servers abused as traffic relays
- ICE candidate injection via compromised signaling — could redirect DataChannel connections

**Mitigations:**

- Sidecars only accept connections from peers authenticated via registry signaling (JWT-verified)
- TURN uses ephemeral credentials (HMAC-SHA1, 24h TTL) — no long-lived TURN credentials
- TURN is optional and disabled by default — only deployed when needed
- Homogeneous webrtc-rs versions across all sidecars — no cross-implementation interop surface

**Residual risk:** The webrtc-rs library is less audited than hyper/tonic (HTTP/gRPC Rust ecosystem). A vulnerability in webrtc-rs DTLS or SCTP handling could affect all sidecars simultaneously. Mitigated by: sidecar isolation (separate process from Python agent), and the sidecar being a small surface (~1,500 lines of Rust).

**Recommended mitigations:**

- Pin webrtc-rs to audited versions and monitor for CVEs
- Run sidecars in sandboxed containers with minimal capabilities
- Consider fuzzing the sidecar's DTLS/SCTP handling in CI

## Known Gaps

Recorded open on 2026-04-10 (ADR-020 kept them open until chatixia-world needed them). Mitigated on 2026-09-29 by ADR-024, when the chatixia-world cross-NAT run put the registry on a public Cloudflare Tunnel.

| ID | Gap | Where | Status | Mitigation (2026-09-29, ADR-024) |
|----|-----|-------|--------|----------------------------------|
| G1 | The pairing admin endpoints (`GET /api/pairing/pending`, `GET /api/pairing/all`, `POST /api/pairing/{id}/approve`, `/reject`, `/revoke`) have no authentication, and the router uses `CorsLayer::permissive()`. Anyone who can reach port 8080 (including a browser page on another origin) can approve a pending agent and obtain a valid device token. Overlaps "Unauthorized Approval of Pending Agents" above. | `registry/src/main.rs:105-114` | Mitigated 2026-09-29 (open 2026-04-10 to 2026-09-29) | Admin routes need `x-admin-token` (`admin::RequireAdmin`, constant-time compare). `REGISTRY_ADMIN_TOKEN` sets it; unset means a random per-run token logged once at startup. Other writes (register, heartbeat, tasks, DELETE agent) need the admin token, an API key or a device token (`admin::RequireCaller`). CORS is an allowlist from `REGISTRY_ALLOWED_ORIGINS` (default: loopback 8080 and 5174). Device tokens reach the pairing device through `/api/pairing/{id}/status` with the pairing secret from `/pair`. |
| G2 | `offer`, `answer`, and `ice_candidate` signaling messages are relayed to the target peer without checking pairing approval. Only `register` → `peer_list` is gated on the approved/legacy peer sets, so any JWT holder can push SDP/ICE at any connected peer. | `registry/src/signaling.rs:94-113` | Mitigated 2026-09-29 (open 2026-04-10 to 2026-09-29) | `SignalingState::handle_message` relays only when both sender and target pass the same approved-or-legacy check as `register`; other messages are dropped with a warning. |
| G3 | Unbounded in-memory growth on the registry: `expire_tasks_loop` marks tasks failed but never removes them; `health_check_loop` marks agents offline but never evicts them; the pairing `cleanup_loop` prunes invite codes and rate-limit buckets but never removes rejected or revoked onboarding entries. Long-running registries grow without limit (a slow DoS, see T4). | `registry/src/hub.rs:71-87`, `registry/src/registry.rs:104-119`, `registry/src/pairing.rs:190-206` | Mitigated 2026-09-29 (open 2026-04-10 to 2026-09-29) | The existing loops now evict: finished tasks after `REGISTRY_TASK_RETENTION_SECS` (3600), agents silent for `REGISTRY_AGENT_EVICTION_SECS` (3600, never below the 270 s offline mark), rejected/revoked onboarding entries after `REGISTRY_ONBOARDING_RETENTION_SECS` (86400). Rate-limit buckets with only stale attempts are pruned too. Each sweep is one `retain` pass per map. |

## Security Checklist for Production

- [ ] Set `SIGNALING_SECRET` explicitly (a random per-run secret is used when unset)
- [ ] Set `REGISTRY_ADMIN_TOKEN` (e.g. `openssl rand -hex 32`) and keep it out of shared logs
- [ ] Set `REGISTRY_ALLOWED_ORIGINS` to exactly the origins that need browser access (often none)
- [ ] Replace `ak_dev_001` with unique API keys per agent
- [ ] Move `api_keys.json` to a secrets manager
- [ ] Enable TLS on registry (via nginx reverse proxy or native)
- [ ] Deploy coturn with TLS (port 5349)
- [ ] Move IPC socket to a secure directory
- [ ] Add rate limiting to all HTTP endpoints
- [ ] Add JWT requirement to GET registry/hub endpoints
- [ ] Implement task submission ACLs
- [ ] Sanitize task payloads before LLM processing
- [ ] Add certificate pinning or DTLS fingerprint verification
- [ ] Set up monitoring/alerting for abnormal signaling patterns
