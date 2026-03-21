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
| JWT signing secret | Critical | `SIGNALING_SECRET` env var |
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
- None currently — no rate limiting, no connection limits

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
- None currently — DELETE endpoint is unauthenticated (same as other registry GET endpoints)

**Residual risk:** Any network-adjacent client can deregister any agent by ID. The agent will re-register on its next heartbeat cycle (~15s), but there is a brief window where it is invisible.

**Recommended mitigations:**
- Require JWT for DELETE endpoint
- Validate that the requesting agent's JWT `sub` matches the `agent_id` being deleted (self-deregister only)

### T9: Information Disclosure via Registry API

**Attack:** Query registry endpoints to enumerate all agents, their skills, IPs, and topology.

**Mitigations:**
- None — registry API is unauthenticated for GET endpoints

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
- None currently — dashboard API endpoints are unauthenticated (consistent with all existing hub endpoints)

**Recommended mitigations:**
- Add authentication to all `/api/pairing/{id}/approve|reject|revoke` endpoints
- Require a dashboard admin JWT or session token
- Add audit logging for all approval/rejection actions

### T10: Device Token Theft

**Attack:** An attacker steals a device token (`dt_` + 32 hex) from a paired agent and uses it to impersonate that agent.

**Mitigations:**
- Device tokens are 128-bit random (infeasible to guess)
- Tokens are only returned once at approval time
- Revocation immediately invalidates the token

**Residual risk:** If the token is intercepted in transit (approval response) or leaked from the agent's storage, it can be used until revoked. TLS on the registry would mitigate in-transit theft.

## Security Checklist for Production

- [ ] Change `SIGNALING_SECRET` from default
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
