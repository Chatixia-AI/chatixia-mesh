# Deployment Guide

How to deploy chatixia-mesh agents across different networks (home, office, VPN, cloud).

## Cross-Network Architecture

The registry handles signaling only — agent-to-agent data flows directly over WebRTC. The registry must be reachable from all machines; the agents do not need to be directly addressable.

```text
Work PC (Enterprise VPN)            Raspberry Pi (Home NAT)
┌─────────────┐                     ┌─────────────┐
│  Sidecar A  │◄── WebRTC P2P ────►│  Sidecar B  │
│  Agent A    │    (DTLS encrypted) │  Agent B    │
└──────┬──────┘                     └──────┬──────┘
       │ WebSocket                         │ WebSocket
       └───────────┐           ┌───────────┘
                   ▼           ▼
            ┌──────────────────────┐
            │   Registry Server    │  ← must be reachable from both
            │   (signaling only)   │
            └──────────────────────┘
```

## Step 1: Choose Where to Run the Registry

The registry must be reachable from every machine running an agent. Options:

| Option | Setup | Trade-offs |
|--------|-------|------------|
| **Cloudflare Tunnel** on a home machine | Free, no port forwarding, HTTPS out of the box | Requires Cloudflare account for persistent URL |
| **Cheap VPS** (Oracle free tier, Hetzner, Fly.io) | Always reachable, cleanest for multi-user | Extra infra to manage |
| **Port-forward** on home router | No external dependencies | Exposes a port to the internet, dynamic IP issues |

**Recommended for personal use**: Cloudflare Tunnel on a Raspberry Pi.

## Step 2: Set Up the Registry

On the machine that will host the registry:

```bash
git clone <repo>
cd chatixia-mesh
cargo build --release -p chatixia-registry
cargo run --release -p chatixia-registry
```

Or with Docker:

```bash
docker compose up registry
```

The registry listens on port 8080 by default.

### Lock down the registry (do this before exposing it)

Set these in the registry's environment (`.env` in the registry's working directory, the shell, or `docker compose`). On the registry host, once:

```bash
cat >> .env <<EOF
SIGNALING_SECRET=$(openssl rand -hex 32)
REGISTRY_ADMIN_TOKEN=$(openssl rand -hex 32)
REGISTRY_ALLOWED_ORIGINS=
EOF
chmod 600 .env
```

An empty `REGISTRY_ALLOWED_ORIGINS` means no other website may call the registry from a browser.

- **`REGISTRY_ADMIN_TOKEN`** protects pairing approval (`/api/pairing/pending|all|{id}/approve|reject|revoke`) and every hub write. If you leave it unset the registry generates a new token on every start and logs it once, like this:

  ```text
  WARN [AUTH] REGISTRY_ADMIN_TOKEN is not set. Generated an admin token for this run:
      adm_3f9c…
      Hub: http://localhost:8080/#admin_token=adm_3f9c…
  ```

  That is fine for a quick test; for a registry that stays up, set it so the token survives restarts.
- **Open the hub** with `https://<registry-url>/#admin_token=<token>` (the hub moves the token into the tab's session storage and removes it from the address bar), or paste it into the **admin token** field in the hub header. Without it the hub still shows agents, tasks and topology but hides the approval queue.
- **`REGISTRY_ALLOWED_ORIGINS`** is a comma-separated list of browser origins allowed to call the registry cross-origin. The default only covers loopback (`http://localhost:8080`, `http://127.0.0.1:8080`, `http://localhost:5174`, `http://127.0.0.1:5174`). The bundled hub is served by the registry itself, so behind a tunnel it is same-origin and needs no entry. Add an origin only if a web page on another host must call the registry.
- Agents and sidecars keep using their API keys (`x-api-key`); registration, heartbeats, task updates and deregistration are refused without one.
- Old entries are evicted automatically (finished tasks after 1 h, silent agents after 1 h, rejected/revoked pairings after 24 h). Tune with `REGISTRY_TASK_RETENTION_SECS`, `REGISTRY_AGENT_EVICTION_SECS`, `REGISTRY_ONBOARDING_RETENTION_SECS`.

## Step 3: Expose the Registry with Cloudflare Tunnel

### Install cloudflared

```bash
# Debian/Ubuntu (arm64 — Raspberry Pi 4/5)
curl -L https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-linux-arm64.deb -o cloudflared.deb
sudo dpkg -i cloudflared.deb

# Debian/Ubuntu (amd64)
curl -L https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-linux-amd64.deb -o cloudflared.deb
sudo dpkg -i cloudflared.deb

# macOS
brew install cloudflared
```

### Quick tunnel (no account, temporary URL)

Good for testing. URL changes on every restart.

```bash
cloudflared tunnel --url http://localhost:8080
```

Prints a URL like `https://random-words-here.trycloudflare.com`. Use this as your registry URL.

### Persistent tunnel (free Cloudflare account, stable URL)

Requires a domain managed by Cloudflare (free plan works).

```bash
# 1. Authenticate
cloudflared tunnel login

# 2. Create a named tunnel
cloudflared tunnel create chatixia-mesh
# Note the tunnel ID printed (e.g., a1b2c3d4-...)

# 3. Configure the tunnel
cat > ~/.cloudflared/config.yml << 'EOF'
tunnel: chatixia-mesh
credentials-file: /home/pi/.cloudflared/<TUNNEL_ID>.json

ingress:
  - hostname: mesh.yourdomain.com
    service: http://localhost:8080
  - service: http_status:404
EOF

# 4. Create DNS record
cloudflared tunnel route dns chatixia-mesh mesh.yourdomain.com

# 5. Start the tunnel
cloudflared tunnel run chatixia-mesh
```

### Run as a system service (auto-start on boot)

```bash
sudo cloudflared service install
sudo systemctl enable cloudflared
sudo systemctl start cloudflared
```

Verify: `curl https://mesh.yourdomain.com/api/registry/agents` should return `[]`, and `curl -i https://mesh.yourdomain.com/api/pairing/pending` should return `401` (with `-H "x-admin-token: $REGISTRY_ADMIN_TOKEN"` it returns `[]`). A `200` without the token means you are running a registry from before ADR-024.

> **The tunnel URL is public.** Anyone who finds it can reach every registry endpoint. That is why the admin token matters: before ADR-024 the pairing approval endpoints were open, so a stranger could approve their own device. Keep the token out of screenshots and shared logs.

## Step 4: Set Up TURN Relay (Recommended)

Enterprise VPNs and strict NATs often block direct UDP between peers. STUN alone won't work in these environments. A TURN relay ensures WebRTC connectivity.

### Option A: Self-host coturn

Run alongside the registry using the included Docker Compose profile:

```bash
# Set a strong secret
export TURN_SECRET=$(openssl rand -hex 32)

# Start coturn
docker compose --profile turn up coturn -d
```

Configure the registry to advertise TURN:

```bash
# .env (on registry host)
TURN_URL=turn:your-host:3478
TURN_SECRET=<the-secret-from-above>
```

> **Note**: Cloudflare Tunnel only proxies HTTP/WebSocket — it cannot relay UDP for TURN. If the registry is behind a Cloudflare Tunnel, coturn must be on a host with a public IP or port-forwarded UDP 3478.

### Option B: Managed TURN service

Use a hosted TURN provider (Metered.ca free tier, Xirsys, Twilio) and set `TURN_URL` / `TURN_SECRET` accordingly.

### Option C: Skip TURN, rely on Tier 3 fallback

If UDP is fully blocked, the system falls back to the HTTP task queue through the registry. This always works but is slower (3–15s per task instead of <100ms). Acceptable for non-real-time workloads.

## Step 5: Create API Keys

Edit `api_keys.json` in the registry's working directory (or point `API_KEYS_FILE` at it). The entries live under a `keys` object:

```json
{
  "keys": {
    "ak_work_pc": { "peer_id": "work-pc", "role": "agent" },
    "ak_rpi_home": { "peer_id": "rpi-home", "role": "agent" }
  }
}
```

Generate real keys with `openssl rand -hex 12`; the registry reads the file on startup, so restart it after changes.

## Step 6: Run Agents

### On the Raspberry Pi (same host as registry)

```bash
chatixia init rpi-agent
cd rpi-agent
```

Edit `.env`:

```bash
CHATIXIA_REGISTRY_URL=http://localhost:8080
SIGNALING_URL=ws://localhost:8080/ws
TOKEN_URL=http://localhost:8080/api/token
API_KEY=ak_rpi_home
CHATIXIA_AGENT_ID=rpi-agent
```

```bash
chatixia run
```

### On the Work PC (remote, via tunnel)

```bash
chatixia init work-agent
cd work-agent
```

Edit `.env`:

```bash
# Use wss:// (not ws://) since Cloudflare Tunnel provides TLS
CHATIXIA_REGISTRY_URL=https://mesh.yourdomain.com
SIGNALING_URL=wss://mesh.yourdomain.com/ws
TOKEN_URL=https://mesh.yourdomain.com/api/token
API_KEY=ak_work_pc
CHATIXIA_AGENT_ID=work-agent
```

```bash
chatixia run
```

### chatixia-world across two NATs

For the world (Phase 2, ADR-020) the same shape is scripted end to end: registry plus quick tunnels on the Pi, The-Alpha's world on a laptop elsewhere. See `chatixia-world/docs/CROSS_NAT_RUN.md` and `scripts/cross-nat/` there. The world only uses API keys, so it runs unchanged against a registry with ADR-024. `pi-home.sh` does not set `REGISTRY_ADMIN_TOKEN` or `SIGNALING_SECRET`, so each run gets fresh random values; the admin token is in `state/registry.log` if you want the hub's approval queue. The evidence the sidecar prints for it is the `[ICE] <peer> selected pair: local=<typ> … remote=<typ> …` line (`srflx`, `prflx` or `relay` means a real NAT crossing; `host` means same LAN), also carried on the `peer_connected` IPC message.

## What Happens Automatically

1. Both sidecars authenticate (API key → JWT) and connect to the registry via WebSocket
2. Registry sends each sidecar the current peer list
3. Sidecars exchange SDP offers/answers through the registry (signaling)
4. ICE negotiation: tries direct P2P → TURN relay → HTTP fallback
5. DTLS-encrypted DataChannel established — registry exits the data path
6. Agents discover each other's skills and can send tasks directly

## Connectivity Tiers

The transport layer degrades gracefully:

| Tier | Path | Latency | When used |
|------|------|---------|-----------|
| **1** | Direct P2P DataChannel | <100ms | Both peers have open UDP path |
| **2** | TURN relay | ~50–200ms | NAT/firewall blocks direct UDP, TURN available |
| **3** | HTTP task queue (via registry) | 3–15s | All UDP blocked, no TURN configured |

Enterprise VPNs typically land on Tier 2 (with TURN) or Tier 3 (without). The system never fails — it only slows down.

## Troubleshooting

| Symptom | Likely cause | Fix |
|---------|-------------|-----|
| Agent can't reach registry | Tunnel not running or URL wrong | Check `cloudflared tunnel run` is active; verify URL with `curl` |
| WebSocket connects but no peers | API key not in `api_keys.json` | Check key exists and peer_id is unique |
| Peers listed but DataChannel fails | UDP blocked, no TURN configured | Set up TURN relay (Step 4) or accept Tier 3 fallback |
| Tasks work but are slow (3–15s) | Using Tier 3 HTTP fallback | Set up TURN relay for Tier 2 speeds |
| `cloudflared` URL changes on restart | Using quick tunnel mode | Set up persistent tunnel with a named tunnel + DNS |
| Agent gets `401` on register/heartbeat | Missing or unknown `x-api-key` | Check `sidecar.api_key` / `API_KEY` matches `api_keys.json` on the registry |
| Hub shows no approval queue, header asks for a token | Hub tab has no admin token (or the registry restarted with a new generated one) | Paste `REGISTRY_ADMIN_TOKEN` (or the token from the registry log) into the header field |
| Browser console: CORS error calling the registry | Page origin not in `REGISTRY_ALLOWED_ORIGINS` | Add the exact origin (scheme + host + port), or serve the page from the registry |
