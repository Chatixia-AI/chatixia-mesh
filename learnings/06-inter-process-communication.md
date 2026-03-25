# Lesson 06: IPC Design -- Bridging Languages with Unix Sockets

**Prerequisites:** [Lesson 01 -- Why Distributed Systems](01-why-distributed-systems.md), [Lesson 04 -- Async Programming Patterns](04-async-programming-patterns.md)

**Key source files:**

| File | What to study |
|------|---------------|
| `sidecar/src/ipc.rs` | Rust side: Unix socket server, read loop, command dispatch |
| `sidecar/src/protocol.rs` | `IpcMessage` struct, `ipc_types` constants |
| `agent/chatixia/core/mesh_client.py` | Python side: `MeshClient`, `_send_ipc`, `_listen_loop`, `request()` |
| `sidecar/src/main.rs` | Sidecar entry point, channel wiring |

**Time estimate:** 60--90 minutes

---

## Introduction

The chatixia-mesh sidecar pattern splits the system into two processes: a Rust sidecar that handles WebRTC networking, and a Python agent that runs application logic. These processes need to talk to each other. That communication channel -- the bridge between the two languages and runtimes -- is the subject of this lesson.

This is a classic problem in systems programming: how do you connect two processes that may be written in different languages, run on different runtimes, and have different failure modes? The answer depends on your constraints. In chatixia-mesh, the choice is Unix domain sockets with a JSON-lines protocol. This lesson explains why, how it works, and what alternatives were considered.

---

## 1. Why IPC?

When the sidecar pattern creates a process boundary, you need an inter-process communication (IPC) mechanism. The Python agent cannot call Rust functions directly. The sidecar cannot access the agent's memory. They are separate OS processes with separate address spaces.

This is fundamentally different from calling a function within the same process. In-process calls are fast (nanoseconds), type-safe (the compiler checks arguments), and reliable (no serialization, no network). Cross-process calls require serialization, a transport, error handling for communication failures, and a protocol both sides agree on.

### The IPC spectrum

Here are the major IPC options, ordered roughly from lowest to highest abstraction:

| Mechanism | Latency | Complexity | Language independence | When to use |
|-----------|---------|------------|----------------------|-------------|
| **Shared memory** | Lowest (~100ns) | Very high | Poor -- requires matching memory layouts | High-throughput, same-machine, same language or ABI-compatible |
| **Pipes / stdin-stdout** | Low (~1us) | Low | Good -- byte streams | Simple parent-child processes, streaming data |
| **Unix domain sockets** | Low (~1us) | Medium | Good -- byte streams | Same-machine, bidirectional, multiple message types |
| **TCP sockets** | Medium (~10us+) | Medium | Good -- byte streams | Cross-machine or when you might distribute later |
| **gRPC** | Medium (~100us+) | High | Excellent -- code generation from .proto | Polyglot services, strong typing, schema evolution |
| **HTTP/REST** | Highest (~1ms+) | Medium | Excellent -- ubiquitous | When you need a web API anyway, debugging ease |

chatixia-mesh uses Unix domain sockets. The reasoning:

- **Same machine, always.** The sidecar is a companion process -- it runs on the same host as the agent. There is no need for TCP's network stack overhead.
- **Bidirectional.** Both sides need to initiate messages. The agent sends commands ("send this message to peer X"). The sidecar pushes events ("peer X connected," "message received from peer Y"). Pipes are unidirectional or awkward for bidirectional use.
- **Language independent.** The transport is just bytes. Any language that can open a socket can participate. No FFI, no shared memory layout concerns, no ABI compatibility.
- **Low overhead.** Unix sockets bypass the TCP/IP stack entirely. They go through the kernel's socket layer but skip routing, checksum computation, and network interface processing.
- **Simple lifecycle.** A socket file appears when the sidecar starts and disappears when it stops. The agent can poll for the file to know when the sidecar is ready.

### Why not the alternatives?

**Shared memory** was rejected because Rust and Python have fundamentally different memory models. Rust has ownership and borrowing; Python has garbage collection. Coordinating memory layouts across these two runtimes would be fragile, bug-prone, and would tightly couple the implementations. The performance gain is irrelevant -- IPC is not the bottleneck in a system where messages traverse WebRTC connections.

**Pipes (stdin/stdout)** work well for simple parent-child streaming, but they are unidirectional per pipe. You can use two pipes (one for each direction), but the programming model gets awkward. You lose the ability to have the OS manage the connection lifecycle as a single entity. Unix sockets give you a single bidirectional connection.

**TCP sockets** would work but add unnecessary overhead. The kernel processes TCP through the full network stack even for `localhost` connections: routing table lookup, loopback interface, TCP state machine. Unix sockets skip all of this. Since the sidecar is always on the same machine, TCP's network capability is wasted.

**gRPC** provides excellent type safety and schema evolution through Protocol Buffers. But it requires `.proto` file management, code generation for both Rust and Python, and adds a dependency on the gRPC runtime. For chatixia-mesh's small IPC protocol (four command types, four event types), this machinery is overhead without proportional benefit.

---

## 2. Unix Domain Sockets

A Unix domain socket is a communication endpoint that lives in the filesystem instead of on a network interface. It looks like a file:

```
/tmp/chatixia-sidecar.sock
```

But it is not a regular file. It is a special socket file (type `s` in `ls -la` output) that the kernel uses as a rendezvous point between processes. You cannot `cat` it or edit it in a text editor. You can only connect to it using socket APIs.

### How they differ from TCP

```
TCP socket:                       Unix domain socket:

Application                       Application
    |                                 |
TCP layer (state machine,         Socket layer (direct
  checksums, retransmit)            kernel buffer transfer)
    |                                 |
IP layer (routing, fragmentation) [no network stack]
    |                                 |
Network interface (loopback)      [no network interface]
    |                                 |
Kernel network stack              Kernel VFS (file system)
```

The key differences:

| Property | TCP (`localhost`) | Unix domain socket |
|----------|------------------|--------------------|
| **Addressing** | IP:port (`127.0.0.1:9000`) | Filesystem path (`/tmp/foo.sock`) |
| **Kernel path** | Full network stack | Direct kernel buffer copy |
| **Latency** | ~10us (loopback) | ~1us |
| **Throughput** | Limited by TCP windowing | Limited by kernel buffer size |
| **Port conflicts** | Possible (port already in use) | File-based (can `unlink` old socket) |
| **Cross-machine** | Yes | No -- same machine only |
| **Security** | IP-based ACLs, firewalls | File permissions (owner, group, mode) |
| **Discovery** | Need to know port number | Need to know file path |

### Security via file permissions

One advantage of Unix sockets that is often overlooked: they inherit filesystem security. The socket file has an owner, a group, and permission bits, just like any regular file:

```
srwxr-xr-x  1  user  user  0  /tmp/chatixia-sidecar.sock
```

If you set the socket's permissions to `0600` (owner read/write only), other users on the same machine cannot connect to it. This is simpler and more reliable than firewall rules for same-machine communication.

chatixia-mesh currently creates the socket at `/tmp/chatixia-sidecar.sock`, which is world-readable in the `/tmp` directory. This is acceptable for development but has security implications in production -- see Exercise 4.

### When to use Unix domain sockets

Use Unix domain sockets when:

- Both processes are on the same machine (always true for the sidecar pattern)
- You need bidirectional communication
- You want low latency without TCP's overhead
- You want filesystem-based access control
- You are bridging different languages or runtimes

Do not use Unix domain sockets when:

- Processes might run on different machines (use TCP)
- You need strong schema enforcement (consider gRPC)
- You want browser compatibility (use WebSocket or HTTP)
- You only need simple one-way streaming (pipes may be simpler)

---

## 3. JSON-Lines Protocol

Unix sockets provide a byte stream, not messages. Bytes go in one end and come out the other, but without inherent message boundaries. If you send `{"type":"send"}` followed by `{"type":"broadcast"}`, the receiver might get them as a single read: `{"type":"send"}{"type":"broadcast"}` -- or split across multiple reads at arbitrary points.

You need a **framing protocol** -- a way to mark where one message ends and the next begins. There are several common approaches:

| Framing method | Format | Pros | Cons |
|----------------|--------|------|------|
| **Newline-delimited (JSON-lines)** | `{...}\n{...}\n` | Simple, human-readable, debuggable | Messages cannot contain literal newlines (unless escaped) |
| **Length-prefixed** | `[4-byte length][payload]` | Efficient, handles any binary payload | Not human-readable, harder to debug |
| **HTTP** | `Content-Length: N\r\n\r\n[body]` | Familiar, well-tooled | Verbose headers, request-response only |
| **Protocol Buffers** | Binary varint length + encoded message | Compact, type-safe, schema evolution | Requires codegen, not human-readable |

chatixia-mesh uses **JSON-lines**: one JSON object per line, terminated by a newline character (`\n`). This is the simplest practical choice.

### Why JSON-lines?

**Debuggability.** During development, you can observe the IPC traffic by reading the socket with standard tools. Each line is a complete, self-describing JSON object. You can pipe it through `jq`, log it, copy it into a test. With a binary protocol, you need a custom decoder.

**Language independence.** Every programming language has a JSON parser. Python has `json.loads()`. Rust has `serde_json::from_str()`. JavaScript, Go, Java -- all have JSON support in their standard libraries. No code generation step, no shared schema files, no version synchronization.

**Simplicity.** The parsing logic on both sides is a read loop that splits on newlines:

```
loop:
    line = read_until('\n')
    message = json_parse(line)
    handle(message)
```

This fits in a few lines of code in any language. Compare with Protocol Buffers, which requires installing `protoc`, writing `.proto` files, generating code for each language, and managing the generated files.

**Good enough performance.** JSON parsing is not free -- it is slower than binary protocols. But IPC messages in chatixia-mesh are small (a few hundred bytes to a few kilobytes) and infrequent (tens to hundreds per second at peak). At this scale, JSON parsing overhead is negligible compared to the WebRTC round-trip time that dominates real latency.

### The trade-off

The cost of JSON-lines is straightforward:

- Messages cannot contain literal newline characters in string values without JSON-escaping them (JSON's `\n` escape handles this automatically).
- Parsing is slower than binary formats. For high-throughput use cases (millions of messages per second), you would choose something else.
- No schema enforcement at the protocol level. A typo in a field name is silently accepted. The application code must validate.

For chatixia-mesh's workload -- low-frequency control messages between two co-located processes -- these trade-offs are entirely acceptable.

---

## 4. The IPC Protocol in chatixia-mesh

The IPC protocol defines eight message types split into two categories: commands (agent to sidecar) and events (sidecar to agent).

### Message structure

Every IPC message has the same shape:

```json
{"type": "<message_type>", "payload": { ... }}
```

In Rust, this is the `IpcMessage` struct from `sidecar/src/protocol.rs`:

```rust
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct IpcMessage {
    #[serde(rename = "type")]
    pub msg_type: String,
    #[serde(default)]
    pub payload: serde_json::Value,
}
```

Two fields. That is the entire IPC envelope. The `type` field identifies what kind of message this is. The `payload` field carries type-specific data as an arbitrary JSON value. The `#[serde(rename = "type")]` attribute maps the Rust field name `msg_type` (since `type` is a reserved keyword in Rust) to the JSON key `type`.

### Agent-to-sidecar commands

These are commands the Python agent sends to the Rust sidecar. The agent initiates; the sidecar acts.

#### `send` -- Send to a specific peer

```json
{"type": "send", "payload": {"target_peer": "peer-abc", "message": {...}}}
```

The `message` field contains a full `MeshMessage` (the application-level protocol from Lesson 07). The sidecar looks up the DataChannel for `target_peer` and sends the message over it.

#### `broadcast` -- Send to all peers

```json
{"type": "broadcast", "payload": {"message": {...}}}
```

The sidecar iterates over all connected DataChannels and sends the message to each one.

#### `list_peers` -- Request connected peer list

```json
{"type": "list_peers", "payload": {}}
```

The sidecar responds with a `peer_list` event containing the IDs of all currently connected peers.

#### `connect` -- Initiate connection to a peer

```json
{"type": "connect", "payload": {"target_peer_id": "peer-abc"}}
```

Requests that the sidecar initiate a WebRTC connection to a specific peer through the signaling server.

### Sidecar-to-agent events

These are events the Rust sidecar pushes to the Python agent. The sidecar initiates; the agent reacts.

#### `message` -- Received a message from a peer

```json
{"type": "message", "payload": {"from_peer": "peer-abc", "message": {...}}}
```

The `message` field contains the `MeshMessage` that arrived over the DataChannel. The `from_peer` field identifies who sent it. This is the most important event -- it is how inter-agent communication reaches the Python application.

#### `peer_connected` -- A new peer joined the mesh

```json
{"type": "peer_connected", "payload": {"peer_id": "peer-abc"}}
```

Fired when a WebRTC DataChannel is established with a new peer. The agent can use this to update its local peer list or trigger discovery logic.

#### `peer_disconnected` -- A peer left the mesh

```json
{"type": "peer_disconnected", "payload": {"peer_id": "peer-abc"}}
```

Fired when a DataChannel is closed or a peer connection fails. The agent removes the peer from its local tracking.

#### `peer_list` -- Response to `list_peers`

```json
{"type": "peer_list", "payload": {"peers": ["peer-abc", "peer-def"]}}
```

Sent in response to a `list_peers` command. Contains the IDs of all currently connected peers.

### Message type constants

Both sides define the message type strings as named constants to prevent typos and make the protocol self-documenting. In Rust (`sidecar/src/protocol.rs`):

```rust
pub mod ipc_types {
    // Agent -> Sidecar commands
    pub const SEND: &str = "send";
    pub const BROADCAST: &str = "broadcast";
    pub const CONNECT: &str = "connect";
    pub const LIST_PEERS: &str = "list_peers";

    // Sidecar -> Agent events
    pub const MESSAGE: &str = "message";
    pub const PEER_CONNECTED: &str = "peer_connected";
    pub const PEER_DISCONNECTED: &str = "peer_disconnected";
    pub const PEER_LIST: &str = "peer_list";
}
```

The Python side uses the same strings as literal values in the `MeshClient` methods.

### Message flow diagram

```
Python Agent                    Rust Sidecar                   Remote Peer
     |                               |                              |
     |--- {"type":"send",...} ------>|                              |
     |        (IPC command)          |--- MeshMessage ------------>|
     |                               |        (DataChannel)        |
     |                               |                              |
     |                               |<-- MeshMessage ------------|
     |<-- {"type":"message",...} ----|        (DataChannel)        |
     |        (IPC event)            |                              |
     |                               |                              |
     |--- {"type":"list_peers"} --->|                              |
     |<-- {"type":"peer_list",...} --|                              |
     |                               |                              |
     |                               |   [DataChannel established]  |
     |<-- {"type":"peer_connected"} -|                              |
     |                               |                              |
     |                               |   [DataChannel closed]       |
     |<-- {"type":"peer_disconnected"}                              |
```

### How the Rust side processes commands

The `serve` function in `sidecar/src/ipc.rs` implements the sidecar end of the IPC channel. Here is the core structure:

```rust
pub async fn serve(
    socket_path: &str,
    mut to_agent_rx: mpsc::UnboundedReceiver<IpcMessage>,
    mesh: Arc<MeshManager>,
    to_agent_tx: mpsc::UnboundedSender<IpcMessage>,
) -> Result<()> {
    // Remove old socket file if it exists
    let _ = tokio::fs::remove_file(socket_path).await;

    let listener = UnixListener::bind(socket_path)?;

    // Accept a single connection (one agent per sidecar)
    let (stream, _) = listener.accept().await?;

    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);

    // Task: forward sidecar->agent events
    let write_task = tokio::spawn(async move {
        while let Some(msg) = to_agent_rx.recv().await {
            let mut line = serde_json::to_string(&msg).unwrap();
            line.push('\n');
            if writer.write_all(line.as_bytes()).await.is_err() {
                break;
            }
        }
    });

    // Read agent->sidecar commands
    let mut line_buf = String::new();
    loop {
        line_buf.clear();
        match reader.read_line(&mut line_buf).await {
            Ok(0) => break,     // agent disconnected
            Ok(_) => {
                let trimmed = line_buf.trim();
                match serde_json::from_str::<IpcMessage>(trimmed) {
                    Ok(msg) => handle_agent_command(msg, &mesh, &to_agent_tx).await,
                    Err(e) => warn!("[IPC] failed to parse: {}", e),
                }
            }
            Err(e) => break,    // read error
        }
    }

    write_task.abort();
    Ok(())
}
```

Key design points:

1. **One agent per sidecar.** The server calls `accept()` once and then enters the read loop. This is deliberate simplicity -- each sidecar serves exactly one agent.

2. **Two concurrent tasks.** The read path (agent commands) runs in the main loop. The write path (sidecar events) runs in a spawned task. They share the socket's read and write halves independently using `stream.into_split()`.

3. **Stale socket cleanup.** The first line removes any leftover socket file from a previous crash. Without this, `bind()` would fail with "address already in use."

4. **Channel-based write path.** The write task receives `IpcMessage` values from an `mpsc::UnboundedReceiver`. Other parts of the sidecar (the WebRTC peer handler, the signaling module) send events to the agent by pushing into the corresponding `UnboundedSender`. This decouples message production from serialization and writing.

The `handle_agent_command` function dispatches commands by type:

```rust
async fn handle_agent_command(
    msg: IpcMessage,
    mesh: &Arc<MeshManager>,
    to_agent_tx: &mpsc::UnboundedSender<IpcMessage>,
) {
    match msg.msg_type.as_str() {
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
        ipc_types::BROADCAST => { /* similar: extract message, call mesh.broadcast() */ }
        ipc_types::LIST_PEERS => {
            let peers = mesh.connected_peers();
            let _ = to_agent_tx.send(IpcMessage {
                msg_type: ipc_types::PEER_LIST.into(),
                payload: serde_json::json!({ "peers": peers }),
            });
        }
        other => warn!("[IPC] unknown command: {}", other),
    }
}
```

Notice how `list_peers` is a request-response within the IPC channel itself: the agent sends a command, the sidecar pushes a `peer_list` event back through the `to_agent_tx` channel. This response goes through the write task and back to the agent as a JSON line.

---

## 5. Request/Response Correlation

The IPC protocol is **message-oriented**, not request-response. Either side can send a message at any time. But the Python agent sometimes needs request-response semantics -- "send this task to peer X and wait for the result."

The `MeshClient.request()` method implements this pattern using `request_id` correlation and `asyncio.Future`.

### How it works

```python
async def request(
    self,
    target_peer: str,
    message: MeshMessage,
    timeout: float = 30.0,
) -> dict[str, Any]:
    """Send a request and wait for a response (matched by request_id)."""
    import uuid

    if not message.request_id:
        message.request_id = uuid.uuid4().hex[:12]

    loop = asyncio.get_event_loop()
    future = loop.create_future()
    self._pending_responses[message.request_id] = future

    await self.send(target_peer, message)

    try:
        return await asyncio.wait_for(future, timeout=timeout)
    finally:
        self._pending_responses.pop(message.request_id, None)
```

Step by step:

1. **Generate a unique request ID.** If the message does not already have one, create a random 12-character hex string (e.g., `"a1b2c3d4e5f6"`). This ID will travel with the message through the sidecar, across the DataChannel, to the remote peer, and back.

2. **Create an asyncio Future.** This is a placeholder for a result that does not exist yet. The calling coroutine will `await` this future, suspending itself until the result arrives.

3. **Register the Future.** Store it in `self._pending_responses` keyed by the request ID. When a response arrives later, the listen loop will look up this dictionary.

4. **Send the request.** The message goes through `_send_ipc` to the sidecar, which forwards it over the DataChannel to the target peer.

5. **Wait with timeout.** `asyncio.wait_for(future, timeout=timeout)` will either return when the future is resolved (response arrived) or raise `asyncio.TimeoutError` after `timeout` seconds.

6. **Cleanup.** The `finally` block removes the pending entry regardless of whether the request succeeded, timed out, or raised an exception. This prevents memory leaks from accumulating abandoned futures.

### The response path

When a response arrives, it flows through the listen loop:

```
Remote Peer --> DataChannel --> Sidecar --> IPC socket --> _listen_loop --> _dispatch
```

Inside `_dispatch`, the code checks whether the incoming message matches a pending request:

```python
async def _dispatch(self, data: dict[str, Any]) -> None:
    msg_type = data.get("type", "")

    # Check for pending request/response
    if msg_type == "message":
        payload = data.get("payload", {})
        inner = payload.get("message", {})
        req_id = inner.get("request_id", "")
        if req_id and req_id in self._pending_responses:
            self._pending_responses[req_id].set_result(inner)
            return

    # Dispatch to registered handlers
    handlers = self._handlers.get(msg_type, []) + self._handlers.get("*", [])
    for handler in handlers:
        result = handler(data)
        if asyncio.iscoroutine(result):
            await result
```

The correlation logic:

1. Check if this is a `message` event (a message received from a peer).
2. Extract the inner `MeshMessage` from the IPC payload.
3. Look for a `request_id` in the inner message.
4. If that `request_id` matches a pending future, resolve the future with `set_result()`. This wakes up the coroutine that called `request()`.
5. Return early -- do not dispatch to normal handlers. The response is consumed by the request/response mechanism.

### The correlation pattern visualized

```
                         Python Agent
                    ________________________
                   |                        |
request()          |  1. Generate request_id|
  |                |  2. Create Future      |
  |                |  3. Store in           |
  |                |     _pending_responses |
  |                |  4. Send message       |
  |                |  5. await Future .......|....... (suspended)
  |                |________________________|
  |                                                        |
  |   IPC socket                                           |
  |   ______|_______                                       |
  |  |              |                                      |
  v  v              |                                      |
Sidecar         _listen_loop                               |
  |                 |                                      |
  | DataChannel     |  (later, when response arrives)      |
  v                 |                                      |
Remote Peer ------->|  6. Parse response                   |
                    |  7. Match request_id                 |
                    |  8. future.set_result(response) ---->|
                    |                                 (resumed)
                    |  9. Return response to caller
```

### Why this pattern?

This is a standard technique for implementing request-response over a message-oriented channel. It shows up in many systems:

- HTTP/2 multiplexes multiple request-response pairs over a single TCP connection using stream IDs
- JSON-RPC uses an `id` field for the same purpose
- AMQP has `correlation_id` in message properties

The alternative -- opening a new connection per request -- would be wasteful and would not work over the single IPC socket connection that chatixia-mesh uses.

---

## 6. Lifecycle Management

The Python agent manages the sidecar's entire lifecycle: starting it, waiting for it to be ready, communicating with it, and shutting it down.

### Starting the sidecar

The `MeshClient.start()` method orchestrates the startup sequence:

```python
async def start(self, auto_spawn_sidecar: bool = True) -> None:
    # Remove stale socket from previous crash
    Path(self._socket_path).unlink(missing_ok=True)

    if auto_spawn_sidecar:
        await self._spawn_sidecar()

    # Wait for socket to appear
    for _ in range(50):  # 5 seconds
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

    # ... timeout check, then connect ...
    self._reader, self._writer = await asyncio.open_unix_connection(
        self._socket_path
    )
    self._listen_task = asyncio.create_task(self._listen_loop())
```

The startup has three phases:

**Phase 1: Cleanup.** Remove any stale socket file left over from a previous crash. Without this, the sidecar's `bind()` call would find an existing socket and succeed, but the old socket is not connected to anything. Alternatively, the agent would try to connect to a dead socket. Cleaning up first avoids both problems.

**Phase 2: Spawn and wait.** The agent spawns the sidecar binary and then polls for the socket file to appear. The sidecar creates the socket when it calls `UnixListener::bind()`. The agent checks every 100ms for up to 5 seconds (50 iterations). During this wait, it also checks whether the sidecar process exited early -- if it did, there is no point waiting for a socket that will never appear.

**Phase 3: Connect.** Once the socket exists, the agent connects using `asyncio.open_unix_connection()`, which returns a `(StreamReader, StreamWriter)` pair. It then starts the listen loop as a background task.

### Three-stage binary resolution

Before spawning, the agent needs to find the sidecar binary. The `_resolve_sidecar_binary()` function implements a three-stage lookup:

```python
def _resolve_sidecar_binary(configured: str) -> str:
    # 1. Explicit path (absolute or relative) that exists and is executable
    p = Path(configured).expanduser()
    if p.exists() and os.access(p, os.X_OK):
        return str(p.resolve())

    # 2. SIDECAR_BINARY environment variable
    env_binary = os.environ.get("SIDECAR_BINARY")
    if env_binary:
        ep = Path(env_binary).expanduser()
        if ep.exists() and os.access(ep, os.X_OK):
            return str(ep.resolve())

    # 3. PATH lookup (shutil.which)
    found = shutil.which(configured)
    if found:
        return found

    raise RuntimeError("Sidecar binary '...' not found.\n...")
```

This three-stage design serves different deployment scenarios:

| Stage | Source | Use case |
|-------|--------|----------|
| 1. Configured path | `agent.yaml` `sidecar.binary` field | Development: `target/release/chatixia-sidecar` |
| 2. Environment variable | `SIDECAR_BINARY` env var | Docker/CI: binary at a non-standard location |
| 3. PATH lookup | `shutil.which()` | Production: binary installed to `/usr/local/bin` |

If none of the stages find a valid executable, the function raises a `RuntimeError` with installation instructions. This is a deliberate UX choice -- instead of a cryptic "file not found" error, the user gets actionable steps.

### Spawning the process

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

Key decisions:

- **Environment inheritance.** The sidecar gets a copy of the agent's environment, plus `IPC_SOCKET` set to the socket path. This is how the sidecar knows where to create its socket file.
- **Captured stderr.** If the sidecar crashes during startup, the agent reads its stderr to include in the error message. This makes debugging much easier -- you see the sidecar's error output, not just "process exited with code 1."
- **No shell.** The binary is invoked directly (`[binary]`), not through a shell. This avoids shell injection vulnerabilities and is slightly faster.

### Handling early crashes

The startup loop includes crash detection:

```python
if self._sidecar_proc and self._sidecar_proc.poll() is not None:
    stderr = (self._sidecar_proc.stderr.read() or b"").decode().strip()
    raise RuntimeError(
        f"Sidecar exited with code {self._sidecar_proc.returncode}"
        + (f": {stderr}" if stderr else "")
    )
```

`poll()` checks if the process has exited without blocking. If it returns a value (the exit code), the sidecar has already crashed. The agent reads whatever the sidecar wrote to stderr and includes it in the exception. Common failure modes:

- Missing environment variables (e.g., no `API_KEY`)
- Registry not reachable (network error during token exchange)
- Port/socket conflict (another sidecar is already running)

### Clean shutdown

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

Shutdown proceeds in order:

1. **Set `_connected = False`.** This causes the listen loop to exit on its next iteration.
2. **Cancel the listen task.** The `asyncio.CancelledError` is caught in `_listen_loop()` and handled gracefully.
3. **Close the writer.** This closes the agent's end of the socket, which the sidecar detects as a read of zero bytes (EOF) and exits its read loop.
4. **Terminate the sidecar process.** Sends `SIGTERM` to the sidecar, giving it a chance to clean up. `wait(timeout=5)` blocks until the process exits or 5 seconds elapse.

### The complete lifecycle

```
Agent start
    |
    v
[1] Remove stale socket file
    |
    v
[2] Resolve sidecar binary (configured -> env var -> PATH)
    |
    v
[3] Spawn sidecar process (subprocess.Popen)
    |
    v
[4] Poll for socket file (100ms intervals, 5s timeout)
    |            |
    |       [crash detected? -> raise RuntimeError with stderr]
    |
    v
[5] Connect to Unix socket (asyncio.open_unix_connection)
    |
    v
[6] Start listen loop (asyncio.create_task)
    |
    v
[7] Register internal handlers (peer_connected, peer_disconnected, peer_list)
    |
    v
    ... agent runs, sends commands, receives events ...
    |
    v
[8] stop() called
    |
    v
[9] Cancel listen task, close socket, terminate sidecar
```

---

## Putting It All Together

The IPC layer is a narrow bridge between two different worlds:

- On one side: a Rust process with direct access to WebRTC APIs, managing peer connections, DataChannels, ICE negotiation, and DTLS encryption.
- On the other side: a Python process running AI application logic, managing skills, LLM integrations, and business logic.

The bridge consists of:

1. A **Unix domain socket** at a known filesystem path for low-latency, same-machine communication.
2. A **JSON-lines framing protocol** for simplicity, debuggability, and language independence.
3. An **8-message IPC protocol** (4 commands, 4 events) that cleanly separates concerns between the two sides.
4. A **request/response correlation** mechanism using `request_id` and `asyncio.Future` for when the agent needs to wait for a reply.
5. A **lifecycle management** system that handles binary resolution, process spawning, readiness detection, crash reporting, and clean shutdown.

Each piece is simple on its own. The power comes from how they compose: the agent can issue a single `request()` call and, behind the scenes, it generates a request ID, serializes to JSON, writes to a Unix socket, the sidecar forwards it over a WebRTC DataChannel, the remote peer processes it, sends a response back over another DataChannel, the sidecar writes the response to the IPC socket, the listen loop matches the request ID, and the original Future resolves -- all asynchronously, all without the agent needing to know about WebRTC, DTLS, or ICE.

---

## Exercises

### Exercise 1: Build a JSON-lines client

Write a Python script that connects to a Unix domain socket and participates in a JSON-lines conversation. Your script should:

1. Create a Unix domain socket server at `/tmp/exercise-ipc.sock`.
2. Accept a connection.
3. Read JSON-lines messages from the connection.
4. For each received message, print it and send back an echo response: `{"type": "echo", "payload": <original_message>}`.

Test it by writing a second script that connects and sends a few messages. Observe that each line is a complete, independent message.

**Hints:**
- Use `asyncio.start_unix_server()` for the server.
- Use `asyncio.open_unix_connection()` for the client.
- Remember to add `\n` after each JSON object.
- Use `reader.readline()` to read one JSON-lines message at a time.

### Exercise 2: Design an IPC protocol for a different sidecar

Imagine you are building a Python web application with a Rust encryption sidecar. The sidecar handles all cryptographic operations: encrypting data before storage, decrypting data on retrieval, key rotation, and signing.

Design the IPC protocol:

1. Define the message types for each direction (application to sidecar, sidecar to application).
2. Define the message structure (what fields does each message type have?).
3. Consider: which operations are request-response (the application needs to wait for a result) and which are fire-and-forget?
4. Consider: how would you handle key rotation without downtime?

Write your protocol as a table of message types with example JSON for each.

### Exercise 3: Request/response timeout analysis

The `MeshClient.request()` method uses `asyncio.wait_for(future, timeout=timeout)` with a default timeout of 30 seconds.

Answer these questions:

1. What happens to the `asyncio.Future` object if the response never arrives and the timeout fires? Is there a memory leak? Trace the code path through the `finally` block.
2. What happens if the sidecar crashes while a request is pending? Does the future ever resolve? What exception does the caller see?
3. The timeout is 30 seconds by default. Is this appropriate for an AI agent delegating a task to another agent that might call an LLM? What factors should determine the timeout value?
4. What happens if two requests are sent with the same `request_id`? Could this happen in practice given how IDs are generated?

### Exercise 4: Socket security analysis

The chatixia-mesh sidecar creates its IPC socket at `/tmp/chatixia-sidecar.sock`. Analyze the security implications:

1. On a multi-user Linux system, can other users on the same machine connect to this socket? What are the default permissions on a socket created with `UnixListener::bind()`?
2. The `/tmp` directory is world-writable. Could an attacker create a socket at this path before the sidecar starts? What would happen? (This is called a "symlink attack" or "TOCTOU race.")
3. Propose a more secure socket location. Research `$XDG_RUNTIME_DIR` -- what is it, what guarantees does it provide, and why is it better than `/tmp`?
4. What file permissions should be set on the socket, and how would you set them in Rust (after `UnixListener::bind()`) and in Python (when connecting)?

---

## Summary

This lesson covered how chatixia-mesh bridges two processes written in different languages using Unix domain sockets and a JSON-lines protocol. The key ideas:

- **Unix domain sockets** provide low-latency, bidirectional, same-machine communication with filesystem-based security.
- **JSON-lines** (one JSON object per line) is a simple framing protocol that trades raw performance for debuggability and language independence.
- The **IPC protocol** has 4 command types (agent to sidecar) and 4 event types (sidecar to agent), all sharing a minimal two-field envelope.
- **Request/response correlation** uses a `request_id` field and `asyncio.Future` to layer request-response semantics on top of a message-oriented channel.
- **Lifecycle management** handles the full sidecar process lifecycle: binary resolution, spawning, readiness detection, crash reporting, and clean shutdown.

In the next lesson, [Lesson 07 -- Application Protocol Design](07-application-protocol-design.md), you will study the `MeshMessage` format that travels inside the IPC `message` payload -- the application-level protocol that agents use to communicate across the mesh.
