# Lesson 15: Deploying Distributed Systems -- Docker Compose, Tunnels, and Cross-Network Connectivity

**Prerequisites:** Lessons 10 (The Sidecar Pattern), 12 (State Management Without a Database)

**Time estimate:** 60-90 minutes

**Key files:**
- `docker-compose.yml` -- full stack service definitions
- `registry/Dockerfile` -- multi-stage build (Node.js + Rust -> debian-slim)
- `sidecar/Dockerfile` -- multi-stage Rust build
- `agent/Dockerfile` -- Python slim build with uv
- `docs/DEPLOYMENT_GUIDE.md` -- cross-network setup guide
- `docs/ADR.md` -- ADR-015 (Docker Compose decision)

---

## Introduction

Building a distributed system and deploying it are fundamentally different problems. The code that runs on your laptop in three terminal windows needs a different strategy when it runs across a Raspberry Pi at home, a laptop at work, and a VM in the cloud. Each step on the deployment spectrum adds capability -- and complexity.

This lesson traces that spectrum for chatixia-mesh. You will learn how Docker Compose orchestrates multiple services on a single machine, how multi-stage Docker builds produce small and secure images, how Cloudflare Tunnel exposes local services to the internet without port forwarding, and how WebRTC connectivity degrades gracefully across hostile network environments.

---

## 1. The Deployment Spectrum

Deploying a multi-component system is not a binary choice between "local" and "production." There is a spectrum, and each step solves problems the previous step could not handle.

### Single machine (development)

The simplest deployment: run every component in separate terminal windows on one machine.

```bash
# Terminal 1: registry
cargo run --release -p chatixia-registry

# Terminal 2: sidecar
cargo run --release -p chatixia-sidecar

# Terminal 3: agent
chatixia run

# Terminal 4: hub
cd hub && npm run dev
```

This works when you are the only user and everything lives on `localhost`. There is no networking to debug, no startup ordering to manage, and no containers to build.

**Limitations:** Manual startup, no reproducibility across machines, no isolation between components. A Rust panic in the sidecar can leave orphaned sockets. You cannot share this setup with a teammate without writing a README longer than the code.

### Docker Compose (local multi-service)

Compose defines all services, their dependencies, environment variables, and shared volumes in a single declarative file. One command starts everything:

```bash
docker compose up --build
```

This solves reproducibility (anyone with Docker gets the same environment), startup ordering (health checks gate dependent services), and isolation (each service gets its own container and filesystem).

**Limitations:** Everything still runs on one machine. Agents on other networks cannot reach the registry. You cannot split the sidecar to a Raspberry Pi and the agent to a laptop.

### Cross-network (VPN, tunnels)

When agents run on different machines -- a Raspberry Pi at home, a laptop behind a corporate VPN, a cloud VM -- the registry must be reachable from all of them. This requires exposing the registry to the internet, either through port forwarding, a VPN, or a reverse tunnel like Cloudflare Tunnel.

The agents themselves do not need to be reachable. WebRTC handles NAT traversal. But the registry (signaling server) must be addressable from every agent.

**Limitations:** Operational complexity increases. You need to manage DNS, TLS certificates (or let the tunnel handle them), TURN relays for UDP-hostile networks, and authentication to prevent unauthorized agents from joining.

### Cloud-native (Kubernetes)

At scale, individual Docker Compose files give way to orchestration platforms. Kubernetes adds auto-scaling, rolling deployments, self-healing, and declarative infrastructure-as-code.

chatixia-mesh does not currently ship Kubernetes manifests. The exercises at the end of this lesson ask you to design one. The key challenge: the sidecar and agent must stay co-located (same pod) because they share an IPC socket, but they are separate containers with independent lifecycles.

**Why not jump straight to Kubernetes?** The same reason you do not write a load balancer before you have two users. Each step on the spectrum is justified by a concrete problem the previous step could not solve. Docker Compose was added to chatixia-mesh (ADR-015) because onboarding required running 3+ commands in different terminals -- not because the system needed horizontal scaling.

---

## 2. Docker Compose for Multi-Service Systems

Docker Compose defines the entire chatixia-mesh stack in `docker-compose.yml`. Four services, one optional profile, and one shared volume:

```yaml
services:
  # ── Registry: signaling + agent registry + hub API + dashboard ──
  registry:
    build:
      context: .
      dockerfile: registry/Dockerfile
    ports:
      - "8080:8080"
    environment:
      SIGNALING_SECRET: ${SIGNALING_SECRET:-dev-secret-change-me}
      API_KEYS_FILE: /etc/chatixia/api_keys.json
      TURN_URL: ${TURN_URL:-}
      TURN_SECRET: ${TURN_SECRET:-}
      REGISTRY_PUBLIC_URL: ${REGISTRY_PUBLIC_URL:-http://localhost:8080}
      HUB_DIST_DIR: /srv/hub
      RUST_LOG: ${RUST_LOG:-info}
    volumes:
      - ./api_keys.json:/etc/chatixia/api_keys.json:ro
    healthcheck:
      test: ["CMD", "curl", "-sf", "http://localhost:8080/api/registry/agents"]
      interval: 10s
      timeout: 3s
      retries: 5
      start_period: 5s

  # ── Sidecar: WebRTC mesh peer + IPC bridge ──
  sidecar:
    build:
      context: .
      dockerfile: sidecar/Dockerfile
    environment:
      SIGNALING_URL: ws://registry:8080/ws
      TOKEN_URL: http://registry:8080/api/token
      API_KEY: ${API_KEY:-ak_dev_001}
      IPC_SOCKET: /run/chatixia/sidecar.sock
      RUST_LOG: ${RUST_LOG:-info}
    volumes:
      - ipc-socket:/run/chatixia
    depends_on:
      registry:
        condition: service_healthy

  # ── Agent: Python AI agent ──
  agent:
    build:
      context: .
      dockerfile: agent/Dockerfile
    environment:
      CHATIXIA_REGISTRY_URL: http://registry:8080
      REGISTRY_URL: http://registry:8080
      SIGNALING_URL: ws://registry:8080/ws
      TOKEN_URL: http://registry:8080/api/token
      API_KEY: ${API_KEY:-ak_dev_001}
      IPC_SOCKET: /run/chatixia/sidecar.sock
      SIDECAR_BINARY: chatixia-sidecar
      LLM_PROVIDER: ${LLM_PROVIDER:-ollama}
      OLLAMA_URL: ${OLLAMA_URL:-http://host.docker.internal:11434/v1}
      LOG_LEVEL: ${LOG_LEVEL:-INFO}
    volumes:
      - ipc-socket:/run/chatixia
      - ./agent.yaml:/app/agent.yaml:ro
    depends_on:
      registry:
        condition: service_healthy
      sidecar:
        condition: service_started

  # ── coturn: TURN relay for NAT traversal ──
  # Enabled with: docker compose --profile turn up
  coturn:
    image: coturn/coturn:4
    ports:
      - "3478:3478/udp"
      - "3478:3478/tcp"
    command:
      - --listening-port=3478
      - --fingerprint
      - --use-auth-secret
      - --static-auth-secret=${TURN_SECRET:-dev-turn-secret}
      - --realm=mesh.chatixia.local
      - --total-quota=100
      - --stale-nonce=600
      - --no-multicast-peers
    profiles:
      - turn

volumes:
  ipc-socket:
```

### Service definitions

Each `services:` entry maps to one container. The `build:` block tells Compose which Dockerfile to use. The `context: .` means the build context is the repository root, so Dockerfiles can `COPY` from any directory in the monorepo.

Services reference each other by name. Inside the Compose network, `ws://registry:8080/ws` resolves to the registry container -- Docker's built-in DNS handles this. No hardcoded IP addresses.

### Health checks

The registry defines a health check:

```yaml
healthcheck:
  test: ["CMD", "curl", "-sf", "http://localhost:8080/api/registry/agents"]
  interval: 10s
  timeout: 3s
  retries: 5
  start_period: 5s
```

This hits the registry's agent list endpoint every 10 seconds. The `-sf` flags make `curl` silent and return a non-zero exit code on HTTP errors. `start_period: 5s` gives the Rust binary time to compile and start listening before the first check.

Health checks serve two purposes:

1. **Dependency ordering.** The sidecar declares `depends_on: registry: condition: service_healthy`. Compose will not start the sidecar container until the registry's health check passes. Without this, the sidecar would try to connect to the registry before it is listening, fail, and exit.

2. **Runtime monitoring.** `docker compose ps` shows the health status of each container. An unhealthy registry is immediately visible.

### Dependency ordering

Compose supports three dependency conditions:

| Condition | When the dependent starts |
|-----------|--------------------------|
| `service_started` | Immediately after the dependency container starts (process may not be ready) |
| `service_healthy` | After the dependency's health check passes |
| `service_completed_successfully` | After the dependency exits with code 0 (for init containers) |

chatixia-mesh uses a chain:

```
registry (healthy) --> sidecar (started) --> agent
```

The sidecar waits for a healthy registry because it needs to authenticate and establish a WebSocket connection on startup. The agent waits for both the registry (healthy) and the sidecar (started). The agent uses `service_started` for the sidecar rather than `service_healthy` because the sidecar has no HTTP endpoint for a health check -- it communicates over WebSocket and Unix sockets only.

### Named volumes for IPC

The sidecar and agent communicate through a Unix domain socket at `/run/chatixia/sidecar.sock`. In Docker, each container has its own filesystem. A named volume bridges them:

```yaml
volumes:
  ipc-socket:

services:
  sidecar:
    volumes:
      - ipc-socket:/run/chatixia
  agent:
    volumes:
      - ipc-socket:/run/chatixia
```

Both containers mount the `ipc-socket` volume at `/run/chatixia`. When the sidecar creates `sidecar.sock` in that directory, the agent can see and connect to it.

Why not use a shared network namespace (putting sidecar and agent in the same network stack)? Because that couples their lifecycles. With separate containers and a shared volume, you can restart the sidecar without killing the agent, scale them independently, and replace either container's image without affecting the other. This is the same reasoning behind the sidecar pattern itself (Lesson 10) -- separation of concerns, applied at the deployment level.

### Profiles for optional services

coturn is declared with `profiles: [turn]`. It does not start with a regular `docker compose up`. You opt in:

```bash
docker compose --profile turn up
```

This keeps the default stack simple. Most developers working on `localhost` do not need a TURN relay -- both peers are on the same machine. Profiles let you add production infrastructure without cluttering the development experience.

### Environment variables and defaults

Compose supports `${VAR:-default}` syntax for environment variables. Values come from:

1. A `.env` file in the same directory as `docker-compose.yml`
2. The shell environment
3. The default after `:-`

This layering means `docker compose up` works out of the box with dev defaults, but production deployments can override every secret via `.env` or shell variables without modifying the Compose file.

---

## 3. Multi-Stage Docker Builds

A naive Dockerfile installs the entire build toolchain (compilers, package managers, dev headers) into the runtime image. The result is a 2+ GB image that takes minutes to pull, expands the attack surface, and wastes disk on every machine it runs on.

Multi-stage builds solve this. You use one image for building and a different, minimal image for running. Only the compiled artifacts are copied forward.

### The registry Dockerfile: three stages

The registry Dockerfile is the most complex because it combines two build ecosystems (Node.js for the hub dashboard, Rust for the server) into one minimal runtime image.

**Stage 1: Build hub static assets (Node.js)**

```dockerfile
FROM node:22-slim AS hub-builder
RUN corepack enable
WORKDIR /app
COPY hub/package.json hub/pnpm-lock.yaml ./
RUN pnpm install --frozen-lockfile
COPY hub/ .
RUN pnpm build
```

This installs pnpm, installs Node.js dependencies, and runs `pnpm build` to produce static HTML/CSS/JS files in `/app/dist`. The `--frozen-lockfile` flag ensures the build fails if `pnpm-lock.yaml` is out of sync with `package.json` -- reproducibility matters.

**Stage 2: Build registry binary (Rust)**

```dockerfile
FROM rust:1.88-bookworm AS rust-builder
WORKDIR /src

# Copy workspace manifests first for layer caching
COPY Cargo.toml Cargo.toml
COPY registry/Cargo.toml registry/Cargo.toml
COPY sidecar/Cargo.toml sidecar/Cargo.toml

# Create stub sources so cargo can resolve the workspace
RUN mkdir -p registry/src sidecar/src \
    && echo "fn main() {}" > registry/src/main.rs \
    && echo "fn main() {}" > sidecar/src/main.rs

RUN cargo build --release -p chatixia-registry && rm -rf registry/src sidecar/src

# Copy real source and rebuild
COPY registry/src registry/src
COPY sidecar/src sidecar/src
RUN touch registry/src/main.rs && cargo build --release -p chatixia-registry
```

This stage uses a dependency caching trick worth understanding. Rust's `cargo build` downloads and compiles all dependencies listed in `Cargo.toml` before touching your source code. By copying only the manifest files first and building with stub `main.rs` files, Docker caches the dependency compilation layer. On subsequent builds, if only your source code changed (but dependencies did not), Docker skips the expensive dependency compilation entirely. The `touch` command after copying real source forces `cargo` to recognize the source file has changed.

This technique reduces rebuild times from 5+ minutes (full dependency recompilation) to 30-60 seconds (only your code).

**Stage 3: Runtime image**

```dockerfile
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

COPY --from=rust-builder /src/target/release/chatixia-registry /usr/local/bin/chatixia-registry
COPY --from=hub-builder /app/dist /srv/hub

ENV RUST_LOG=info
ENV HUB_DIST_DIR=/srv/hub
EXPOSE 8080
ENTRYPOINT ["chatixia-registry"]
```

The final image is `debian:bookworm-slim` -- no Rust compiler, no Node.js, no build tools. It contains:

- The compiled registry binary (~15 MB)
- The hub static assets (~2 MB)
- `ca-certificates` (for HTTPS to external services)
- `curl` (for the health check)

The result is roughly 50-80 MB. Compare this to the build stages: `rust:1.88-bookworm` is about 1.5 GB, and `node:22-slim` adds another 200 MB. Without multi-stage builds, the runtime image would carry all of that.

### Why this matters beyond image size

Small images have security implications. Every package installed in the runtime image is an attack surface. The `gcc` compiler, `make`, header files, and development libraries that live in build images are exactly the kind of tools an attacker exploits if they gain container access. The runtime image should contain the minimum needed to run the application -- nothing more.

### The sidecar Dockerfile: two stages

The sidecar follows the same pattern but simpler -- no Node.js stage:

```dockerfile
FROM rust:1.88-bookworm AS builder
# ... same workspace caching trick ...
RUN cargo build --release -p chatixia-sidecar

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /src/target/release/chatixia-sidecar /usr/local/bin/chatixia-sidecar
ENTRYPOINT ["chatixia-sidecar"]
```

No `curl` here -- the sidecar has no HTTP endpoint to health-check.

### The agent Dockerfile: single-stage with uv

The agent uses a different approach. Python does not compile to a static binary, so there is no "build stage" in the same sense:

```dockerfile
FROM python:3.12-slim-bookworm

RUN apt-get update && apt-get install -y --no-install-recommends libpq5 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Install uv
COPY --from=ghcr.io/astral-sh/uv:latest /uv /usr/local/bin/uv

# Install chatixia package
COPY agent/ /app/
RUN uv sync --frozen --no-dev

ENTRYPOINT ["uv", "run", "chatixia"]
CMD ["run"]
```

The `COPY --from=ghcr.io/astral-sh/uv:latest` line is worth noting. It pulls the `uv` binary directly from a pre-built image without adding the full `uv` image as a build stage. This is a common pattern for grabbing standalone binaries.

`uv sync --frozen --no-dev` installs Python dependencies from the lockfile, skipping development dependencies. The `--frozen` flag is the Python equivalent of pnpm's `--frozen-lockfile`: it fails if the lockfile is missing or inconsistent.

---

## 4. Exposing Services with Cloudflare Tunnel

When agents run on different networks, the registry must be reachable from all of them. Traditional options -- port forwarding on a router, renting a VPS with a public IP -- either expose your home network or cost money. Cloudflare Tunnel offers a third path.

### How it works

Cloudflare Tunnel creates an outbound connection from your machine to Cloudflare's edge network. Traffic flows:

```
Remote agent --> Cloudflare edge (HTTPS) --> Tunnel --> localhost:8080 (registry)
```

Your machine never opens an inbound port. The tunnel process (`cloudflared`) maintains a persistent outbound connection to Cloudflare, and Cloudflare routes incoming requests through it. This is zero-trust networking: no firewall rules, no port forwarding, no dynamic DNS.

### Quick tunnel (temporary, no account)

For testing:

```bash
cloudflared tunnel --url http://localhost:8080
```

This prints a temporary URL like `https://random-words-here.trycloudflare.com`. The URL changes every time you restart the tunnel. No Cloudflare account needed.

### Persistent tunnel (stable URL, requires Cloudflare DNS)

For production use with a domain managed by Cloudflare (free plan works):

```bash
# 1. Authenticate
cloudflared tunnel login

# 2. Create a named tunnel
cloudflared tunnel create chatixia-mesh

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

# 5. Start the tunnel (can also run as a systemd service)
cloudflared tunnel run chatixia-mesh
```

The `ingress` block maps hostnames to local services. The final `- service: http_status:404` is a catch-all for unmatched requests.

To run as a system service that starts on boot:

```bash
sudo cloudflared service install
sudo systemctl enable cloudflared
sudo systemctl start cloudflared
```

### Limitation: Cloudflare Tunnel cannot proxy UDP

Cloudflare Tunnel proxies HTTP and WebSocket traffic. It does not proxy raw UDP. This matters because:

- **WebSocket signaling works** -- the registry's `/ws` endpoint is HTTP-upgraded to WebSocket, which the tunnel handles.
- **TURN relay does not work through the tunnel** -- coturn needs UDP on port 3478. If the registry is behind a Cloudflare Tunnel, coturn must be on a host with a public IP or port-forwarded UDP.

This is a fundamental constraint of reverse proxy tunnels: they sit at layer 7 (HTTP), not layer 4 (TCP/UDP).

---

## 5. Cross-Network Connectivity

When chatixia-mesh agents run on different networks, the transport layer encounters NAT, firewalls, and corporate network policies. The system handles this with three connectivity tiers that degrade gracefully.

### The three tiers

```
Tier 1: Direct P2P          Tier 2: TURN Relay          Tier 3: HTTP Fallback
(<100ms)                     (~50-200ms)                 (3-15s)

  Agent A <----> Agent B     Agent A <---> TURN <---> B  Agent A --> Registry --> B
  (UDP hole-punch)           (UDP relay)                 (HTTP poll)
```

| Tier | Path | Latency | When used |
|------|------|---------|-----------|
| **1** | Direct P2P DataChannel | <100ms | Both peers have open UDP path (same LAN, permissive NAT) |
| **2** | TURN relay | ~50-200ms | NAT/firewall blocks direct UDP, TURN server available |
| **3** | HTTP task queue (via registry) | 3-15s | All UDP blocked, no TURN configured |

The system never fails -- it only slows down. ICE negotiation (Lesson 2) automatically tries Tier 1, falls back to Tier 2 if a TURN server is configured, and the application layer falls back to Tier 3 if no DataChannel can be established at all.

### A real-world scenario

Consider a common deployment: a Raspberry Pi at home running the registry and one agent, and a work laptop behind a corporate VPN running another agent.

```
Work PC (Enterprise VPN)              Raspberry Pi (Home NAT)
+---------------------------+         +---------------------------+
|  +--------+  +--------+  |         |  +--------+  +--------+  |
|  |Sidecar | |  Agent  |  |         |  |Registry|  |Sidecar |  |
|  |   A    | |    A    |  |         |  |        |  |   B    |  |
|  +---+----+ +----+----+  |         |  +---+----+  +---+----+  |
|      |  IPC      |       |         |      |  WS       |  IPC  |
|      +-----+-----+       |         |      +-----+-----+       |
|            |              |         |            |  +--------+ |
+------------|-------+------+         +------------|--|Agent B |-+
             |       |                             |  +--------+
             |       |                             |
   +---------+-------+-----------------------------+--------+
   |   Cloudflare Tunnel   (HTTPS / WSS only)               |
   +---------------------------------------------------------+
             |
   +---------+---------+
   |    coturn TURN     |    <-- Needs public IP or
   |    (UDP relay)     |       port-forwarded UDP
   +-------------------+
```

What happens:

1. Both sidecars connect to the registry via WebSocket. The work laptop uses `wss://mesh.yourdomain.com/ws` (through the Cloudflare Tunnel). The Raspberry Pi sidecar uses `ws://localhost:8080/ws` (local).

2. The registry sends each sidecar the current peer list. Both sidecars begin ICE negotiation, exchanging SDP offers and ICE candidates through the registry's signaling channel.

3. **Tier 1 attempt:** The work PC's sidecar tries to connect directly to the Raspberry Pi via UDP. The corporate VPN likely blocks outbound UDP to arbitrary ports. Symmetric NAT on both sides makes hole-punching impossible. Tier 1 fails.

4. **Tier 2 attempt:** If a TURN server is configured, the sidecars request relay allocations from coturn. Both sides can reach the TURN server (it has a public IP with UDP 3478 open). The TURN server relays UDP packets between them. DataChannel established at ~50-200ms latency.

5. **Tier 3 fallback:** If no TURN server is configured (or UDP is completely blocked, even to the TURN server), no DataChannel is established. The agent framework falls back to the HTTP task queue through the registry. Tasks are submitted via REST API and picked up on the next heartbeat poll. This always works because the HTTP path goes through the Cloudflare Tunnel, but latency jumps to 3-15 seconds.

Enterprise VPNs typically land on Tier 2 (with TURN) or Tier 3 (without). Home-to-home connections often achieve Tier 1 if both routers support endpoint-independent mapping (most consumer routers do).

---

## 6. IPC in Containers

Lesson 6 covered the Unix socket IPC protocol between the sidecar and agent. In Docker, the challenge is that each container has an isolated filesystem. The sidecar creates `/run/chatixia/sidecar.sock`, but the agent's container cannot see it unless the filesystem is shared.

### The named volume pattern

```yaml
volumes:
  ipc-socket:

services:
  sidecar:
    volumes:
      - ipc-socket:/run/chatixia
  agent:
    volumes:
      - ipc-socket:/run/chatixia
```

Docker creates a volume called `ipc-socket` (a directory on the host managed by Docker). Both containers mount it at `/run/chatixia`. The sidecar writes `sidecar.sock` into the volume; the agent connects to it from the same volume.

### Why not share a network namespace?

An alternative approach is `network_mode: "service:sidecar"` on the agent container. This puts both containers in the same network namespace, so `localhost` traffic between them works -- including Unix sockets created on `localhost`'s filesystem.

The chatixia-mesh Compose file deliberately avoids this. The reason is the same principle behind the sidecar pattern (Lesson 10): independent lifecycles. If the sidecar and agent share a network namespace:

- Restarting the sidecar tears down the agent's network stack
- You cannot independently scale or replace either container
- Port conflicts between the containers must be managed manually
- Health monitoring conflates two different components

Named volumes give you the IPC path without coupling the container lifecycles. The sidecar can crash and restart, and as long as it recreates the socket at the same path, the agent can reconnect.

### Socket cleanup

One subtlety: if the sidecar crashes without cleanly removing its socket file, the stale socket remains in the volume. The next sidecar instance must either unlink the old socket before binding or use `SO_REUSEADDR`. The sidecar handles this by unlinking the socket path before creating a new listener.

---

## 7. TURN Relay Setup

When direct peer-to-peer UDP fails (Tier 1), a TURN relay provides an alternative path. TURN (Traversal Using Relays around NAT) is a protocol where a server with a public IP relays UDP packets between peers that cannot reach each other directly.

### Self-hosted coturn via Docker Compose

The chatixia-mesh Compose file includes a coturn service behind a profile:

```bash
# Set a strong shared secret
export TURN_SECRET=$(openssl rand -hex 32)

# Start coturn alongside the stack
docker compose --profile turn up
```

The coturn configuration in `docker-compose.yml`:

```yaml
coturn:
  image: coturn/coturn:4
  ports:
    - "3478:3478/udp"
    - "3478:3478/tcp"
  command:
    - --listening-port=3478
    - --fingerprint
    - --use-auth-secret
    - --static-auth-secret=${TURN_SECRET:-dev-turn-secret}
    - --realm=mesh.chatixia.local
    - --total-quota=100
    - --stale-nonce=600
    - --no-multicast-peers
  profiles:
    - turn
```

Key flags:

- `--use-auth-secret` enables ephemeral credentials (ADR-006). The registry generates time-limited TURN credentials using HMAC-SHA1 over the shared secret. Clients never see the secret itself.
- `--static-auth-secret` is the shared secret between coturn and the registry.
- `--total-quota=100` limits the number of concurrent relay allocations.
- `--no-multicast-peers` prevents the relay from forwarding to multicast addresses (a security measure).

### Connecting coturn to the registry

The registry needs two environment variables to advertise TURN to connecting sidecars:

```bash
# In .env or docker-compose environment
TURN_URL=turn:your-host:3478
TURN_SECRET=<same-secret-as-coturn>
```

When a sidecar requests ICE configuration via `GET /api/config`, the registry generates ephemeral credentials and returns the TURN server URL. The sidecar includes this in its ICE configuration, and WebRTC automatically uses the TURN relay if direct connectivity fails.

### The public IP requirement

coturn must be reachable by both peers on UDP port 3478. This means it needs either:

- A machine with a public IP address
- Port forwarding on a router (UDP 3478 -> coturn host)

Critically, coturn cannot sit behind a Cloudflare Tunnel. The tunnel only proxies HTTP/WebSocket, not raw UDP. If your registry is behind a Cloudflare Tunnel, coturn must be deployed separately on a host with direct UDP connectivity.

This is often the most confusing part of the deployment: the registry can hide behind a tunnel, but the TURN relay cannot.

### Managed alternatives

If you do not want to operate your own TURN server:

| Provider | Free tier | Notes |
|----------|-----------|-------|
| **Metered.ca** | 500 GB/month | Simple setup, TURN-as-a-service |
| **Xirsys** | Limited free tier | Global network, enterprise-oriented |
| **Twilio** | Pay-as-you-go | Network Traversal Service, well-documented |

Set `TURN_URL` and `TURN_SECRET` to the values provided by your managed TURN service. The registry's ephemeral credential generation works with any TURN server that supports `use-auth-secret` mode (RFC 5766 long-term credentials with a shared secret).

### When you can skip TURN

If all your agents are on the same LAN or you accept Tier 3 HTTP fallback latency (3-15 seconds), you do not need a TURN relay. TURN solves a specific problem: agents that can exchange signaling messages (via the registry) but cannot establish direct UDP connectivity (due to NAT or firewalls).

---

## Summary

Deploying chatixia-mesh follows a spectrum from simple to sophisticated:

| Level | Method | What it solves | What it does not solve |
|-------|--------|----------------|----------------------|
| **Dev** | Manual terminals | Quick iteration | Reproducibility, multi-user |
| **Local** | Docker Compose | Reproducibility, startup ordering, isolation | Cross-network connectivity |
| **Cross-network** | Compose + Cloudflare Tunnel + TURN | Internet-reachable registry, NAT traversal | Auto-scaling, self-healing |
| **Production** | Kubernetes | All of the above + orchestration | Complexity budget |

Key takeaways:

1. **Multi-stage Docker builds** separate build-time dependencies from runtime, reducing image size from gigabytes to tens of megabytes and shrinking the attack surface.

2. **Health checks and dependency ordering** in Compose prevent startup race conditions. Use `service_healthy` when you need the dependency to be ready, not just started.

3. **Named volumes** share IPC sockets between containers without coupling their network namespaces or lifecycles.

4. **Cloudflare Tunnel** provides zero-trust access to the registry without port forwarding, but cannot proxy UDP -- TURN servers need direct connectivity.

5. **Connectivity degrades gracefully** through three tiers. The system never fails; it only gets slower.

6. **Each deployment step is justified by a concrete problem** the previous step could not solve. Do not adopt Kubernetes until Docker Compose is insufficient.

---

## Exercises

### Exercise 1: Network diagram

Draw a network diagram for the following deployment scenario:

- A Raspberry Pi at home runs the registry and one agent (Agent B)
- A laptop at work runs another agent (Agent A)
- A Cloudflare Tunnel exposes the registry to the internet
- A coturn TURN relay runs on the Raspberry Pi (with port-forwarded UDP 3478)

Your diagram should show:

- Which connections go through the Cloudflare Tunnel (HTTP/WSS)
- Which connections go directly to coturn (UDP)
- Where the TURN relay sits in the data path for Tier 2 connectivity
- How the agents would communicate under Tier 1 (direct), Tier 2 (TURN), and Tier 3 (HTTP fallback)

### Exercise 2: Sidecar crash detection

The sidecar container crashes unexpectedly. Consider the following questions:

1. How does the agent container detect that the sidecar is gone? (Hint: what happens to the Unix socket when the sidecar process exits?)
2. What happens to existing WebRTC DataChannels when the sidecar crashes?
3. Does the registry know the sidecar crashed? How long until it marks the peer as offline?
4. What connectivity tier does the agent fall back to while the sidecar is down?
5. If the sidecar container restarts automatically (Docker's `restart: always`), what needs to happen for the agent to reconnect?

### Exercise 3: Sidecar health check

The agent container has no health check, and neither does the sidecar. Design a health check for the sidecar container.

Constraints:

- The sidecar has no HTTP endpoints -- it communicates over WebSocket (to the registry) and Unix socket (to the agent).
- `curl` is not installed in the sidecar's runtime image (check the Dockerfile to verify this).
- The health check must run inside the sidecar container.

Questions to answer:

1. What command could the health check use? Consider checking whether the sidecar process is running, whether the IPC socket exists, or whether a connection to the registry WebSocket is active.
2. What are the trade-offs of each approach? Which one most accurately reflects "the sidecar is working correctly"?
3. Write the `healthcheck:` block you would add to the sidecar service in `docker-compose.yml`.

### Exercise 4: Kubernetes deployment design

Propose a Kubernetes deployment for chatixia-mesh. For each component, decide whether it should be a Deployment, DaemonSet, StatefulSet, or something else. Consider:

1. The registry is stateless (in-memory, ADR-004). Should it be a single-replica Deployment or can it scale horizontally? What breaks if you run two replicas?
2. The sidecar and agent must share an IPC socket. In Kubernetes, how do you co-locate two containers? What Kubernetes primitive keeps them on the same node with shared storage?
3. coturn is a network-level service that peers connect to directly via UDP. Does it belong in the cluster, or should it run outside?
4. How would you handle the `ipc-socket` volume in Kubernetes? Is a named volume the right abstraction, or do you need something else?
5. What would the pod spec look like for an agent + sidecar pair? Sketch the YAML.

---

## Further Reading

- `docs/DEPLOYMENT_GUIDE.md` -- step-by-step deployment instructions for chatixia-mesh
- `docs/ADR.md`, ADR-015 -- the decision record for adopting Docker Compose
- `docs/ADR.md`, ADR-006 -- ephemeral TURN credential design
- [Docker multi-stage builds documentation](https://docs.docker.com/build/building/multi-stage/)
- [Cloudflare Tunnel documentation](https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/)
- [coturn project](https://github.com/coturn/coturn) -- open-source TURN/STUN server
- [RFC 5766](https://www.rfc-editor.org/rfc/rfc5766) -- Traversal Using Relays around NAT (TURN)
