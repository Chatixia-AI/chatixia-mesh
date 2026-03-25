# Lesson 10: The Sidecar Pattern -- Encapsulating Complexity Across Process Boundaries

**Prerequisites:** [Lesson 03: WebRTC Fundamentals](03-webrtc-fundamentals.md), [Lesson 06: Inter-Process Communication](06-inter-process-communication.md)

**Time estimate:** 60-90 minutes

**Key source files:**
- `sidecar/src/main.rs` -- entry point, component wiring
- `sidecar/src/mesh.rs` -- MeshManager, peer and channel tracking
- `sidecar/src/webrtc_peer.rs` -- WebRTC connection lifecycle (offer/answer)
- `sidecar/src/ipc.rs` -- Unix socket server, JSON-line protocol
- `sidecar/src/protocol.rs` -- all message types (Signaling, Mesh, IPC)
- `agent/chatixia/core/mesh_client.py` -- sidecar spawning, binary resolution, IPC client
- `sidecar/Dockerfile` -- multi-stage Rust build

---

## What is the sidecar pattern?

A sidecar is a helper process that runs alongside a primary application process. It handles cross-cutting concerns -- responsibilities that every service needs but that do not belong in the application's core logic. The name comes from motorcycle sidecars: attached to the main vehicle, traveling together, but serving a different purpose.

The pattern has a simple structure:

```
+---------------------------+     +---------------------------+
|     Primary Process       |     |     Sidecar Process       |
|                           |     |                           |
|   Application logic       |     |   Cross-cutting concern   |
|   (business rules,        |<--->|   (networking, logging,   |
|    AI inference,           | IPC |    security, telemetry)   |
|    user interaction)       |     |                           |
+---------------------------+     +---------------------------+
```

The two processes share a machine (or a pod, in Kubernetes terms) and communicate through a local channel -- a Unix socket, a loopback TCP connection, or shared memory. From the outside, they look and deploy as a single unit. From the inside, they are isolated: different binaries, often different languages, with a well-defined contract between them.

### Real-world examples

The sidecar pattern is widely used in production systems:

**Envoy (service mesh proxy).** Every microservice gets an Envoy sidecar that handles load balancing, circuit breaking, mTLS, and observability. The application makes plain HTTP calls to localhost; Envoy intercepts them and routes them to the correct upstream with retries, timeouts, and encryption. Istio and other service meshes are built on this model.

**Dapr (application runtime).** Dapr runs as a sidecar that provides building blocks -- pub/sub, state management, service invocation, secrets -- through a local HTTP or gRPC API. Applications call Dapr at localhost:3500 and get portable abstractions over infrastructure services.

**Fluentd / Fluent Bit (logging).** These log collectors run alongside application processes, tailing log files or receiving structured logs over a local socket. The application writes logs in its native format; the sidecar handles parsing, filtering, buffering, and forwarding to a centralized store.

**Linkerd (mTLS proxy).** Similar to Envoy, Linkerd's proxy sidecar handles mutual TLS, providing encryption between services without requiring each application to manage certificates.

The common thread: the primary process stays focused on its domain. The sidecar absorbs infrastructure complexity behind a local API.

---

## Why a sidecar for WebRTC?

chatixia-mesh agents are written in Python. They need to communicate directly with each other over WebRTC DataChannels -- encrypted, peer-to-peer connections that traverse NAT and firewalls. This is a significant amount of protocol machinery: SDP negotiation, ICE candidate gathering, DTLS handshake, SCTP framing, and DataChannel lifecycle management.

The project considered three options for giving Python agents WebRTC capability:

1. **Use aiortc (Python WebRTC library).** aiortc is the main Python WebRTC implementation. It works for simple cases but has known fragility in production -- particularly around DTLS (the encryption layer). Debugging failures requires understanding both Python async internals and the WebRTC state machine simultaneously.

2. **Embed Rust in Python via PyO3 (FFI).** Write the WebRTC code in Rust and call it from Python using PyO3 bindings. This keeps everything in one process but creates a tight coupling between the Rust and Python async runtimes.

3. **Run a Rust sidecar process.** Write all WebRTC logic in Rust, run it as a separate process, and bridge it to Python through IPC.

The project chose option 3 and documented the reasoning in ADR-001:

> **Context:** Python agents need to communicate over WebRTC DataChannels, but the Python WebRTC ecosystem (aiortc) is fragile, hard to debug, and lacks production-grade DTLS support.
>
> **Decision:** Each Python agent spawns a Rust sidecar process that handles all WebRTC/signaling complexity. The agent communicates with its sidecar via a Unix domain socket using a simple JSON-line protocol.

The consequences, quoted directly from the ADR:

- (+) Robust WebRTC via webrtc-rs (well-maintained, DTLS built-in)
- (+) Python agents stay simple -- no WebRTC dependencies
- (+) Sidecar can be reused for agents in other languages
- (-) Extra process per agent; slightly more complex deployment
- (-) IPC adds a small latency hop (~1ms)

The sidecar isolates approximately 1,100 lines of Rust across six modules (`main.rs`, `mesh.rs`, `webrtc_peer.rs`, `signaling.rs`, `ipc.rs`, `protocol.rs`) into a single binary. The Python agent never imports a WebRTC library. It sends and receives JSON lines over a Unix socket -- a protocol simple enough to implement in any language.

---

## The boundary contract

The most important thing about the sidecar pattern is not what lives inside the sidecar. It is the contract between the sidecar and the primary process.

In chatixia-mesh, that contract is the IPC protocol: JSON lines over a Unix domain socket. Every message is a single JSON object followed by a newline character. The protocol defines two directions of communication.

### Agent to sidecar (commands)

The agent sends commands to the sidecar to interact with the mesh:

```json
{"type": "send",       "payload": {"target_peer": "peer-abc", "message": {...}}}
{"type": "broadcast",  "payload": {"message": {...}}}
{"type": "list_peers", "payload": {}}
{"type": "connect",    "payload": {"target_peer_id": "peer-abc"}}
```

### Sidecar to agent (events)

The sidecar notifies the agent about mesh events:

```json
{"type": "message",           "payload": {"from_peer": "peer-abc", "message": {...}}}
{"type": "peer_connected",    "payload": {"peer_id": "peer-abc"}}
{"type": "peer_disconnected", "payload": {"peer_id": "peer-abc"}}
{"type": "peer_list",         "payload": {"peers": ["peer-abc", "peer-def"]}}
```

This protocol is defined in Rust as the `IpcMessage` struct (in `sidecar/src/protocol.rs`):

```rust
pub struct IpcMessage {
    #[serde(rename = "type")]
    pub msg_type: String,
    #[serde(default)]
    pub payload: serde_json::Value,
}
```

And mirrored on the Python side through the `MeshClient._send_ipc` method (in `agent/chatixia/core/mesh_client.py`):

```python
async def _send_ipc(self, msg: dict[str, Any]) -> None:
    """Send a JSON-line message to the sidecar."""
    assert self._writer is not None
    line = json.dumps(msg) + "\n"
    self._writer.write(line.encode())
    await self._writer.drain()
```

### Why the boundary matters

The IPC protocol is a stable contract. As long as both sides agree on the message format, either side can change its internals independently:

- The sidecar could switch from webrtc-rs to a different WebRTC library, or even to a C++ implementation via FFI. The agent would not notice.
- The agent could be rewritten from Python to Go, TypeScript, or Java. It would just need to connect to the Unix socket and speak JSON lines.
- The sidecar could add internal features -- connection pooling, message batching, priority queues -- without changing the IPC protocol.

This is the defining property of the sidecar pattern: the boundary contract decouples evolution. The two processes can be developed, tested, and deployed on different schedules as long as the contract holds.

---

## Inside the sidecar

The sidecar binary (`chatixia-sidecar`) has five internal modules. Here is how they compose:

```
+-----------------------------------------------------------------------+
|                          chatixia-sidecar                              |
|                                                                       |
|  +------------------+     +-----------------+     +----------------+  |
|  |    signaling.rs   |     |  webrtc_peer.rs  |     |    mesh.rs     |  |
|  |                  |     |                 |     |                |  |
|  |  WebSocket to    |---->|  Create offer   |---->|  MeshManager   |  |
|  |  registry:       |     |  Handle offer   |     |                |  |
|  |  - SDP relay     |     |  ICE forwarding |     |  peers:        |  |
|  |  - ICE relay     |     |  DataChannel    |     |    DashMap     |  |
|  |  - peer_list     |     |  setup          |     |  channels:     |  |
|  |                  |     |                 |     |    DashMap     |  |
|  +------------------+     +-----------------+     +----------------+  |
|           ^                                              |            |
|           |                                              |            |
|           |         +------------------+                 |            |
|           |         |   protocol.rs    |                 |            |
|           |         |                  |                 |            |
|           +---------|  SignalingMessage |                 |            |
|                     |  MeshMessage     |                 |            |
|                     |  IpcMessage      |                 |            |
|                     +------------------+                 |            |
|                                                          v            |
|                     +------------------+                              |
|                     |     ipc.rs       |                              |
|                     |                  |                              |
|                     |  Unix socket     |<------- JSON lines -------> |
|                     |  server          |       (to/from agent)       |
|                     +------------------+                              |
+-----------------------------------------------------------------------+
```

### Entry point: `main.rs`

The entry point wires the four runtime components together using tokio channels.

From `sidecar/src/main.rs`:

```rust
// Create mesh manager
let mesh = Arc::new(mesh::MeshManager::new(token.peer_id.clone()));

// Channel for outbound signaling messages
let (sig_tx, sig_rx) = mpsc::unbounded_channel::<String>();

// Channel for messages from mesh -> IPC (to Python agent)
let (to_agent_tx, to_agent_rx) = mpsc::unbounded_channel::<protocol::IpcMessage>();
```

Three components run concurrently in separate tokio tasks:

1. **IPC server** -- listens on the Unix socket for agent commands, forwards mesh events to the agent.
2. **Signaling client** -- connects to the registry via WebSocket, relays SDP offers/answers and ICE candidates.
3. **`tokio::select!`** -- watches both tasks and shuts down if either exits.

The `sig_tx` channel connects outbound signaling from WebRTC peer management back to the WebSocket writer. The `to_agent_tx` channel connects inbound mesh messages and peer lifecycle events to the IPC writer. These two channels are the internal circulatory system of the sidecar.

### MeshManager: `mesh.rs`

The `MeshManager` is the central data structure. It tracks all peer connections and their DataChannels using concurrent hash maps:

```rust
pub struct MeshManager {
    pub local_peer_id: String,
    peers: DashMap<String, MeshPeer>,       // peer_id -> MeshPeer
    channels: DashMap<String, Arc<RTCDataChannel>>,  // peer_id -> DataChannel
}
```

`DashMap` is a concurrent hash map from the `dashmap` crate. It allows multiple tokio tasks to read and write peer state without holding a mutex across await points -- an important property in async Rust where holding a standard `Mutex` across `.await` can cause deadlocks.

Each `MeshPeer` holds a reference to both the underlying `RTCPeerConnection` and its `RTCDataChannel`:

```rust
pub struct MeshPeer {
    pub peer_id: String,
    pub pc: Arc<RTCPeerConnection>,
    pub dc: Option<Arc<RTCDataChannel>>,
}
```

The `MeshManager` provides four key operations:

- `add_peer` / `remove_peer` -- lifecycle management.
- `set_channel` -- called once the DataChannel opens, enables sending.
- `send_to` -- serialize a `MeshMessage` to JSON and send it over a peer's DataChannel.
- `broadcast` -- send to all connected peers by spawning a task per channel.
- `connected_peers` -- list peer IDs that have open DataChannels (used by `list_peers` IPC command).

### WebRTC peer connection lifecycle: `webrtc_peer.rs`

This module handles two distinct roles in the WebRTC handshake:

**Offerer (`initiate_connection`).** When the sidecar learns about a new peer from the registry's `peer_list` message, it initiates a connection:

1. Create a new `RTCPeerConnection` with ICE servers (STUN + optional TURN).
2. Set up ICE candidate forwarding (send candidates to remote peer via signaling).
3. Set up connection state tracking (remove peer on disconnect, notify agent).
4. Create a DataChannel named `"mesh"`.
5. Wire up the DataChannel's message handler to forward received messages to the agent via IPC.
6. Register the peer and channel in `MeshManager`.
7. Create an SDP offer, set it as the local description.
8. Send the offer to the remote peer through the signaling channel.

**Answerer (`handle_offer`).** When the sidecar receives an SDP offer from another peer:

1. Create a new `RTCPeerConnection` with ICE servers.
2. Set up ICE forwarding and state tracking (same as offerer).
3. Register an `on_data_channel` callback -- the answerer does not create the DataChannel; it receives it from the offerer.
4. Set the remote description (the received offer).
5. Register the peer in `MeshManager`.
6. Create an SDP answer, set it as the local description.
7. Send the answer back through signaling.

The key asymmetry: the offerer calls `pc.create_data_channel("mesh", None)`, while the answerer calls `pc.on_data_channel(...)` to wait for the channel from the offerer. Both sides end up with the same result -- a bidirectional DataChannel -- but through different paths.

### DataChannel message flow

Once the DataChannel is open, messages flow through this path:

```
Agent A                Sidecar A                         Sidecar B                Agent B
  |                       |                                  |                       |
  |-- send (IPC) -------->|                                  |                       |
  |                       |-- MeshMessage (DataChannel) ---->|                       |
  |                       |                                  |-- message (IPC) ----->|
  |                       |                                  |                       |
```

The DataChannel handler in `webrtc_peer.rs` deserializes incoming bytes into a `MeshMessage`, wraps it in an `IpcMessage` of type `"message"`, and sends it to the agent over the `to_agent_tx` channel:

```rust
dc.on_message(Box::new(move |msg: DataChannelMessage| {
    let text = String::from_utf8_lossy(&msg.data);
    match serde_json::from_str::<MeshMessage>(&text) {
        Ok(mesh_msg) => {
            let ipc_msg = IpcMessage {
                msg_type: ipc_types::MESSAGE.into(),
                payload: serde_json::json!({
                    "from_peer": from_peer,
                    "message": mesh_msg,
                }),
            };
            let _ = to_agent.send(ipc_msg);
        }
        Err(e) => {
            warn!("[DC] failed to parse message from {}: {}", from_peer, e);
        }
    }
}));
```

On the sending side, the IPC server in `ipc.rs` receives a `"send"` command from the agent, extracts the target peer and message, and calls `mesh.send_to()`:

```rust
ipc_types::SEND => {
    let target = msg.payload.get("target_peer")
        .and_then(|t| t.as_str()).unwrap_or("");
    if let Some(message) = msg.payload.get("message") {
        if let Ok(mesh_msg) = serde_json::from_value::<MeshMessage>(message.clone()) {
            if let Err(e) = mesh.send_to(target, &mesh_msg).await {
                warn!("[IPC] send to {} failed: {}", target, e);
            }
        }
    }
}
```

---

## Process management

The Python agent is responsible for the sidecar's lifecycle: starting it, waiting for it to be ready, detecting crashes, and shutting it down cleanly.

### Binary resolution

Before spawning the sidecar, the agent must find the binary. The `_resolve_sidecar_binary` function in `mesh_client.py` implements a three-stage resolution:

```python
def _resolve_sidecar_binary(configured: str) -> str:
    # 1. Explicit path (absolute or relative) that exists on disk
    p = Path(configured).expanduser()
    if p.exists() and os.access(p, os.X_OK):
        return str(p.resolve())

    # 2. SIDECAR_BINARY environment variable
    env_binary = os.environ.get("SIDECAR_BINARY")
    if env_binary:
        ep = Path(env_binary).expanduser()
        if ep.exists() and os.access(ep, os.X_OK):
            return str(ep.resolve())

    # 3. Search PATH
    found = shutil.which(configured)
    if found:
        return found
```

The resolution order prioritizes specificity:

1. **Configured path** -- set in `agent.yaml` under `sidecar.binary`. If the user provides an absolute path to a specific binary, use it.
2. **`SIDECAR_BINARY` environment variable** -- useful for Docker or CI environments where the binary location is controlled externally.
3. **PATH lookup** -- the default for development setups where `chatixia-sidecar` is installed via `cargo install` or built and added to PATH.

If none of these resolve, the function raises a `RuntimeError` with install instructions. This explicit failure is important -- a missing sidecar is not a recoverable error, and the error message tells the user exactly how to fix it.

### Spawning and readiness

The `_spawn_sidecar` method starts the binary as a subprocess:

```python
async def _spawn_sidecar(self) -> None:
    binary = _resolve_sidecar_binary(self._sidecar_binary)
    env = os.environ.copy()
    env["IPC_SOCKET"] = self._socket_path

    self._sidecar_proc = subprocess.Popen(
        [binary],
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
```

The agent passes its own environment to the sidecar with one addition: `IPC_SOCKET` is set to the path where the agent will connect. This ensures both sides agree on the socket location without requiring external configuration.

After spawning, the agent polls for readiness by checking whether the socket file exists:

```python
for _ in range(50):  # 5 seconds, checking every 0.1s
    if Path(self._socket_path).exists():
        break
    # Check if sidecar exited early
    if self._sidecar_proc and self._sidecar_proc.poll() is not None:
        stderr = (self._sidecar_proc.stderr.read() or b"").decode().strip()
        raise RuntimeError(
            f"Sidecar exited with code {self._sidecar_proc.returncode}"
            + (f": {stderr}" if stderr else "")
        )
    await asyncio.sleep(0.1)
```

This polling loop handles two failure modes:

- **Sidecar crashes on startup.** The `poll()` check detects that the process has exited, reads stderr for the error message, and raises an exception immediately instead of waiting the full 5 seconds.
- **Sidecar is slow to start.** The 50-iteration loop with 0.1s sleeps gives the sidecar up to 5 seconds to create the socket. In practice, this takes less than a second.

### Clean shutdown

When the agent stops, it terminates the sidecar:

```python
async def stop(self) -> None:
    self._connected = False
    if self._listen_task:
        self._listen_task.cancel()
    if self._writer:
        self._writer.close()
    if self._sidecar_proc:
        self._sidecar_proc.terminate()
        self._sidecar_proc.wait(timeout=5)
```

The shutdown sequence is ordered: first cancel the listen loop (stop reading), then close the socket writer (stop writing), then terminate the sidecar process and wait up to 5 seconds for it to exit. If the sidecar does not exit within 5 seconds, `wait` will raise a `TimeoutExpired` exception -- though in practice, the sidecar responds to `SIGTERM` promptly because tokio's runtime shuts down when the main tasks exit.

### Docker deployment

The sidecar has a multi-stage Dockerfile (in `sidecar/Dockerfile`):

```dockerfile
# --- Build stage ---
FROM rust:1.88-bookworm AS builder
WORKDIR /src
COPY Cargo.toml Cargo.toml
COPY registry/Cargo.toml registry/Cargo.toml
COPY sidecar/Cargo.toml sidecar/Cargo.toml

# Create stub sources for layer caching
RUN mkdir -p registry/src sidecar/src \
    && echo "fn main() {}" > registry/src/main.rs \
    && echo "fn main() {}" > sidecar/src/main.rs
RUN cargo build --release -p chatixia-sidecar && rm -rf registry/src sidecar/src

# Copy real source and rebuild
COPY registry/src registry/src
COPY sidecar/src sidecar/src
RUN touch sidecar/src/main.rs && cargo build --release -p chatixia-sidecar

# --- Runtime stage ---
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /src/target/release/chatixia-sidecar /usr/local/bin/chatixia-sidecar
ENTRYPOINT ["chatixia-sidecar"]
```

The build uses a dependency-caching trick: it copies only the `Cargo.toml` manifests first, builds with stub `main.rs` files to compile all dependencies, then copies the real source and rebuilds. This means dependency compilation is cached in a Docker layer and only the application code needs recompilation on changes. The final image is based on `debian:bookworm-slim`, containing only the compiled binary and CA certificates (needed for HTTPS connections to the registry).

In the `docker-compose.yml`, the sidecar and agent share a volume for the IPC socket:

```
volumes:
  ipc-socket: {}  # Shared between sidecar and agent containers
```

This volume (mounted at `/run/chatixia/sidecar.sock`) is how the containerized agent reaches the containerized sidecar -- the same Unix socket mechanism, just mediated through a Docker volume instead of a shared filesystem.

---

## Trade-offs

The sidecar pattern is not free. Here are the concrete costs in chatixia-mesh:

### Extra process per agent

Every running agent requires a sidecar process. A deployment with 10 agents has 20 processes (10 agents + 10 sidecars). Each sidecar maintains its own WebSocket connection to the registry and its own set of WebRTC peer connections. Memory usage is modest -- the sidecar binary is small and Rust's memory footprint is predictable -- but the operational surface area is larger than a single-process design.

### IPC latency

Messages between the agent and sidecar cross a process boundary via Unix socket. This adds roughly 1ms of latency per message hop. For the kinds of messages agents exchange (task requests, prompts, status updates), this is negligible -- the LLM inference that follows takes seconds. But it would matter for a high-frequency trading system or a real-time game.

### Deployment complexity

The sidecar binary must be compiled for each target platform. A Mac ARM developer, a Linux x86_64 server, and a Raspberry Pi ARM device each need a different binary. The Python agent, by contrast, runs anywhere Python runs. This is the "binary distribution problem" that pure scripting languages avoid. The project mitigates this through cargo install (compiles from source) and Docker images (pre-compiled for each platform).

### The HTTP fallback already works

chatixia-mesh's skill handlers (in `mesh_skills.py`) implement a fallback pattern: try the P2P DataChannel first, fall back to the registry's HTTP task queue if the peer is not directly reachable. This means the sidecar is an optimization layer. The system functions without it -- just slower and with less privacy (messages flow through the registry instead of directly between peers).

This is an important architectural property. The sidecar adds capability (direct P2P, DTLS encryption, NAT traversal) but is not a hard dependency for basic operation. If the sidecar crashes, the agent can still submit tasks via HTTP.

---

## Alternatives considered

The sidecar pattern was one of several options. Here is why the alternatives were not chosen.

### FFI via PyO3

PyO3 allows calling Rust code directly from Python. The WebRTC logic could be compiled as a Python extension module, eliminating the IPC hop entirely.

**Why not:** PyO3 binds the Rust code to the Python runtime. The sidecar could not be reused by a Go or Java agent without rewriting the binding layer. Additionally, mixing tokio (Rust's async runtime) with asyncio (Python's async runtime) through pyo3-asyncio is possible but adds complexity -- you are debugging two async runtimes in one process. If the Rust code panics or segfaults, it takes the Python process with it. The sidecar pattern provides crash isolation: a sidecar crash is observable and recoverable without losing the agent's in-memory state.

### Shared library (cdylib)

Compile the WebRTC code as a C-compatible shared library (`.so` / `.dylib`) and call it via Python's `ctypes` or CFFI.

**Why not:** Similar to PyO3 but with a lower-level binding mechanism. The serialization boundary would still exist (converting Python objects to C-compatible structs), and the crash isolation problem remains. Shared libraries also complicate deployment -- you need to distribute the right `.so` for each platform and manage dynamic linking.

### In-process Rust via pyo3-asyncio

A variant of the PyO3 approach that specifically bridges the async runtimes. The Rust code runs on tokio within the Python process, and pyo3-asyncio translates between Rust futures and Python coroutines.

**Why not:** This is the most technically elegant option but the least proven. pyo3-asyncio is a relatively young library, and debugging issues at the boundary between two async runtimes requires expertise in both. The sidecar approach is "boring technology" -- Unix sockets and JSON lines are understood by every language and every debugger.

### Why chatixia-mesh chose separate processes

The decision comes down to three properties:

1. **Isolation.** A crashing sidecar does not crash the agent. A crashing agent does not crash the sidecar (though the sidecar has no purpose without its agent, so it will eventually exit). Each process can be restarted independently.

2. **Language independence.** The IPC protocol is language-agnostic. Any language that can connect to a Unix socket and write JSON can use the sidecar. This means the investment in the Rust WebRTC implementation is not locked to Python.

3. **Debuggability.** Two separate processes can be inspected, traced, and profiled independently. You can run `strace` on the sidecar while running a Python debugger on the agent. You can test the sidecar with a simple `socat` command that sends JSON lines. You can replace the agent with a shell script for testing. Process boundaries make systems more observable.

---

## Exercises

### Exercise 1: Identify sidecar candidates

Think about a system you work on (or a well-known system you understand). List three cross-cutting concerns that could be extracted into a sidecar process. For each one, describe:

- What the sidecar would do.
- What the IPC protocol between the primary process and the sidecar would look like (message types, direction, format).
- What benefit extraction would provide (crash isolation, language independence, independent scaling, etc.).

### Exercise 2: Replace Unix socket IPC with gRPC

The current IPC protocol uses JSON lines over a Unix socket. Propose a design that replaces it with gRPC between the sidecar and agent.

Consider these questions:

- What would the `.proto` file look like? Define the service, RPCs, and message types.
- What changes in the sidecar (`ipc.rs`)?
- What changes in the agent (`mesh_client.py`)?
- What stays the same? (Hint: the `MeshManager`, `webrtc_peer.rs`, and `signaling.rs` should not change at all.)
- What do you gain? What do you lose compared to JSON lines?

### Exercise 3: Deployment diagram

Draw a deployment diagram (on paper or in ASCII art) showing:

- One physical or virtual machine.
- Two Python agents running on that machine.
- Two sidecar processes, one per agent.
- The registry (running elsewhere or on the same machine).
- All connections: Unix sockets (agent-to-sidecar), WebSocket (sidecar-to-registry), WebRTC DataChannel (sidecar-to-sidecar).

Label each connection with its protocol and direction. Pay attention to port numbers and socket paths -- how do two sidecars on the same machine avoid conflicting?

### Exercise 4: Binary distribution strategy

The sidecar is a compiled Rust binary. Propose a distribution strategy that supports three target platforms:

- macOS ARM (Apple Silicon developer machines)
- Linux x86_64 (cloud servers)
- Linux ARM (Raspberry Pi)

Your strategy should address:

- How are binaries built? (CI cross-compilation, per-platform build agents, or something else?)
- How are binaries distributed? (GitHub Releases, package managers, cargo install, container images?)
- How does the Python agent find the right binary for its platform?
- What happens if a user is on a platform you have not pre-built for?

---

## Summary

The sidecar pattern separates cross-cutting concerns into a companion process. In chatixia-mesh, this means all WebRTC complexity -- signaling, DTLS, ICE traversal, DataChannel management -- lives in a Rust binary that the Python agent never directly calls. The two communicate through a JSON-lines protocol over a Unix socket.

The pattern's value comes from the boundary contract. The IPC protocol is simple, language-agnostic, and stable. As long as both sides agree on the message format, either side can evolve independently. The sidecar can switch WebRTC libraries; the agent can be rewritten in a different language. Neither change affects the other.

The costs are real: an extra process per agent, ~1ms IPC latency, and the need to distribute a compiled binary for each target platform. But these costs are predictable and manageable. The benefits -- crash isolation, language independence, and debuggability -- compound as the system grows.

The sidecar is not the only way to solve this problem. FFI, shared libraries, and in-process async bridges are all viable. chatixia-mesh chose the sidecar because it prioritizes operational simplicity over theoretical elegance: Unix sockets and JSON lines are boring, well-understood technology that works in every debugger and every programming language.
