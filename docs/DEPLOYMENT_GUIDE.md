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

Verify: `curl https://mesh.yourdomain.com/api/registry/agents` should return `[]`.

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

Edit `api_keys.json` in the registry's working directory:

```json
{
  "ak_work_pc": { "peer_id": "work-pc", "role": "agent" },
  "ak_rpi_home": { "peer_id": "rpi-home", "role": "agent" }
}
```

Restart the registry to pick up changes (or it reads on startup).

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
