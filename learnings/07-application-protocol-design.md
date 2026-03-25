# Lesson 07: Application Protocol Design -- MeshMessage and Task Lifecycle

**Prerequisites:** [Lesson 05: Signaling Protocol Design](05-signaling-protocol-design.md), [Lesson 06: Inter-Process Communication](06-inter-process-communication.md)

**Time estimate:** 75-90 minutes

**Key source files:**
- `sidecar/src/protocol.rs` -- MeshMessage struct and all message type constants
- `registry/src/hub.rs` -- Task struct, TaskSubmission, HubState, task lifecycle
- `agent/chatixia/core/mesh_skills.py` -- handle_delegate with P2P-first and HTTP fallback
- `agent/chatixia/core/mesh_client.py` -- MeshClient.request() for correlated request/response

---

## Introduction

Lessons 05 and 06 covered how chatixia-mesh establishes connections (signaling) and how the sidecar bridges WebRTC to Python (IPC). Both of those are *transport* protocols -- they move bytes between processes. This lesson covers what those bytes actually mean.

Application protocol design answers three questions:

1. **What is the shape of a message?** -- Fields, types, serialization format.
2. **What do different message types mean?** -- The vocabulary of the protocol.
3. **What sequences of messages constitute a conversation?** -- Request/response patterns, state machines, timeouts.

chatixia-mesh defines three application-level protocols, each riding on a different transport. This lesson examines all three, with the deepest focus on `MeshMessage` -- the protocol that agents use to communicate with each other.

---

## 1. Layered Protocols

Every message in chatixia-mesh travels through multiple layers. The application protocol defines what the message *means*. The transport protocol defines how the message *gets there*. Understanding which layer does what is essential for debugging and extending the system.

```
+---------------------------------------------------------------+
|                     Application Layer                         |
|                                                               |
|  MeshMessage          IpcMessage          SignalingMessage     |
|  (agent-to-agent)     (sidecar-to-agent)  (peer-to-registry)  |
+---------------------------------------------------------------+
|                      Transport Layer                          |
|                                                               |
|  WebRTC DataChannel   Unix Domain Socket   WebSocket          |
|  (SCTP over DTLS)     (JSON lines)         (JSON frames)      |
+---------------------------------------------------------------+
|                      Network Layer                            |
|                                                               |
|  UDP (direct/TURN)    Local filesystem     TCP (HTTP upgrade)  |
+---------------------------------------------------------------+
```

Each column is an independent communication channel:

| Channel | Application Protocol | Transport | Endpoints |
|---------|---------------------|-----------|-----------|
| Agent-to-agent | `MeshMessage` | WebRTC DataChannel | Sidecar <-> Sidecar |
| Sidecar-to-agent | `IpcMessage` | Unix domain socket (JSON lines) | Sidecar <-> Python agent |
| Peer-to-registry | `SignalingMessage` | WebSocket (JSON frames) | Sidecar <-> Registry |

The same `MeshMessage` often crosses two of these channels in sequence. When Agent A sends a task to Agent B, the message starts as a `MeshMessage`, gets wrapped in an `IpcMessage` to cross the Unix socket, then gets unwrapped and sent as a raw `MeshMessage` over the DataChannel, then gets wrapped in another `IpcMessage` on the receiving side to reach Agent B's Python process:

```
Agent A (Python)                                        Agent B (Python)
     |                                                       ^
     | IpcMessage{type:"send",                               | IpcMessage{type:"message",
     |   payload:{target_peer, message: MeshMessage}}         |   payload:{from_peer, message: MeshMessage}}
     v                                                       |
Sidecar A ---- MeshMessage (raw JSON over DataChannel) ----> Sidecar B
```

This layering means each component only needs to understand its own protocol. The Python agent never deals with WebRTC. The sidecar never interprets task payloads. The registry never sees agent-to-agent message content.

---

## 2. MeshMessage Format

The `MeshMessage` struct is defined in `sidecar/src/protocol.rs` and mirrored in Python at `agent/chatixia/core/mesh_client.py`. It is the single message format for all agent-to-agent communication over DataChannels.

### The struct (Rust)

```rust
// sidecar/src/protocol.rs

pub struct MeshMessage {
    #[serde(rename = "type")]
    pub msg_type: String,
    #[serde(default)]
    pub request_id: String,
    #[serde(default)]
    pub source_agent: String,
    #[serde(default)]
    pub target_agent: String,
    #[serde(default)]
    pub payload: serde_json::Value,
}
```

### The dataclass (Python)

```python
# agent/chatixia/core/mesh_client.py

@dataclass
class MeshMessage:
    msg_type: str
    request_id: str = ""
    source_agent: str = ""
    target_agent: str = ""
    payload: dict[str, Any] = field(default_factory=dict)
```

Five fields. That is the entire envelope. Let's examine each one.

### Field reference

| Field | Wire name | Required | Purpose |
|-------|-----------|----------|---------|
| `msg_type` | `"type"` | Yes | Determines how the receiver should interpret the message. The only mandatory field. |
| `request_id` | `"request_id"` | No | Correlates requests with responses. Generated by the sender (UUID hex, 12 chars). Empty for fire-and-forget messages. |
| `source_agent` | `"source_agent"` | No | Agent ID of the sender. Used for routing responses and attribution. |
| `target_agent` | `"target_agent"` | No | Agent ID of the intended recipient. `"*"` for broadcasts. |
| `payload` | `"payload"` | No | Arbitrary JSON. Contents depend on `msg_type`. |

Note the `#[serde(rename = "type")]` annotation in Rust -- the struct field is `msg_type` (because `type` is a reserved word in Rust), but it serializes to `"type"` on the wire. The Python side follows the same convention: the dataclass field is `msg_type`, but `to_dict()` maps it to `"type"`.

### Design decisions

**Why a flat struct with a type discriminator?** Alternatives include tagged unions (Rust enums) or separate structs per message type. The flat-with-discriminator approach was chosen because:

1. Both Rust and Python need to parse the same wire format. Tagged unions in Rust's serde use `{"type": "...", ...}` anyway -- the flat struct makes the shared format explicit.
2. `payload` is `serde_json::Value` (Rust) / `dict` (Python), meaning each message type can carry completely different data without changing the envelope.
3. New message types can be added by defining a new constant string -- no struct changes, no schema migration.

**Why are most fields optional (defaulted)?** A simple `ping` message only needs `{"type": "ping"}`. Requiring all five fields for every message type would add noise. The `#[serde(default)]` annotation means missing fields deserialize to empty strings or null, not parse errors. This makes the protocol forward-compatible: old code can receive new message types and ignore unknown fields gracefully.

### Message type constants

All valid `msg_type` values are defined as string constants in `sidecar/src/protocol.rs`:

```rust
pub mod mesh_types {
    pub const PING: &str = "ping";
    pub const PONG: &str = "pong";

    pub const TASK_REQUEST: &str = "task_request";
    pub const TASK_RESPONSE: &str = "task_response";
    pub const TASK_STREAM_CHUNK: &str = "task_stream_chunk";

    pub const SKILL_QUERY: &str = "skill_query";
    pub const SKILL_RESPONSE: &str = "skill_response";

    pub const AGENT_STATUS: &str = "agent_status";

    pub const AGENT_PROMPT: &str = "agent_prompt";
    pub const AGENT_RESPONSE: &str = "agent_response";
    pub const AGENT_STREAM_CHUNK: &str = "agent_stream_chunk";
}
```

These fall into four categories:

**Connectivity testing:**
- `ping` / `pong` -- Lightweight heartbeat. A peer sends `ping`; the other replies `pong`. Used to verify a DataChannel is alive. No `request_id` needed since the response is always immediate and unambiguous.

**Task delegation (request/response):**
- `task_request` -- "Please execute this task." Carries a `request_id` so the sender can match the eventual response. Payload includes `message` and optionally `skill`.
- `task_response` -- "Here is the result of that task." Carries the same `request_id` from the original request. Payload includes `result` or `error`.
- `task_stream_chunk` -- Streaming variant. Carries the same `request_id` and a partial result. Used for long-running tasks that produce incremental output.

**Skill discovery (request/response):**
- `skill_query` -- "What skills do you have?" or "Can you do X?"
- `skill_response` -- "Here are my skills" or "Yes/no."

**Agent communication (fire-and-forget):**
- `agent_status` -- Broadcast of an agent's current state (skills, health, load).
- `agent_prompt` -- A direct message or broadcast to another agent. No response expected.
- `agent_response` -- Optional reply to an agent_prompt. Not correlated by request_id.
- `agent_stream_chunk` -- Streaming version of agent_response for incremental delivery.

### Example messages on the wire

A `task_request`:
```json
{
  "type": "task_request",
  "request_id": "a1b2c3d4e5f6",
  "source_agent": "research-agent",
  "target_agent": "summarizer-agent",
  "payload": {
    "message": "Summarize the latest findings on quantum computing",
    "skill": "summarize"
  }
}
```

A `task_response` (matched by `request_id`):
```json
{
  "type": "task_response",
  "request_id": "a1b2c3d4e5f6",
  "source_agent": "summarizer-agent",
  "target_agent": "research-agent",
  "payload": {
    "result": "Quantum computing has advanced in three key areas..."
  }
}
```

A `ping` (minimal message):
```json
{
  "type": "ping"
}
```

An `agent_prompt` broadcast:
```json
{
  "type": "agent_prompt",
  "source_agent": "coordinator-agent",
  "target_agent": "*",
  "payload": {
    "message": "Status check: report your current workload",
    "broadcast": true
  }
}
```

---

## 3. Task Lifecycle: The State Machine

When an agent delegates work to another agent, the work is tracked as a **task**. Tasks have a lifecycle defined by the `Task` struct in `registry/src/hub.rs` and managed by `HubState`.

### The Task struct

```rust
// registry/src/hub.rs

pub struct Task {
    pub id: String,
    pub skill: String,
    pub target_agent_id: String,
    pub source_agent_id: String,
    pub assigned_agent_id: String,
    pub payload: serde_json::Value,
    pub state: String,           // "pending", "assigned", "completed", "failed"
    pub result: String,
    pub error: String,
    pub created_at: f64,         // Unix epoch seconds
    pub updated_at: f64,
    pub ttl: u64,                // Time-to-live in seconds (default: 300)
}
```

### State machine

Tasks move through exactly four states:

```
                          +-------------------+
                          |                   |
              submit_task |     pending       |
          +-------------->|                   |
          |               +--------+----------+
          |                        |
          |          get_pending_for_agent()
          |          (agent polls or claims)
          |                        |
          |                        v
          |               +-------------------+
          |               |                   |
          |               |     assigned      |
          |               |                   |
          |               +--------+----------+
          |                        |
          |              +---------+---------+
          |              |                   |
          |     update_task()          update_task()
          |     state="completed"      state="failed"
          |              |                   |
          |              v                   v
          |     +----------------+  +----------------+
          |     |                |  |                 |
          |     |   completed    |  |     failed      |
          |     |                |  |                 |
          |     +----------------+  +--------+--------+
          |                                  ^
          |                                  |
          |         expire_tasks_loop()      |
          |         (TTL exceeded)           |
          +----------------------------------+
                 (also from pending)
```

### State transitions

| From | To | Triggered by | What happens |
|------|----|-------------|--------------|
| (none) | `pending` | `submit_task()` HTTP handler | A new task is created with a UUID, timestamped, and inserted into `HubState.tasks`. |
| `pending` | `assigned` | `get_pending_for_agent()` | An agent claims the task by matching on `target_agent_id` or `skill`. The task's `assigned_agent_id` is set. |
| `assigned` | `completed` | `update_task()` HTTP handler | The executing agent POSTs the result. `task.result` is populated. |
| `assigned` | `failed` | `update_task()` HTTP handler | The executing agent POSTs an error. `task.error` is populated. |
| `pending` or `assigned` | `failed` | `expire_tasks_loop()` | Background loop runs every 30 seconds. If `now - created_at > ttl`, the task is marked failed with error `"TTL expired"`. |

### TTL-based expiration

The `TaskSubmission` struct accepts a `ttl` field with a default of 300 seconds (5 minutes):

```rust
#[serde(default = "default_ttl")]
pub ttl: u64,

fn default_ttl() -> u64 {
    300
}
```

The `expire_tasks_loop` runs as a background tokio task, spawned in `registry/src/main.rs`:

```rust
let hub = state.hub.clone();
tokio::spawn(async move { hub.expire_tasks_loop().await });
```

Every 30 seconds, it iterates all tasks. If a task is not in a terminal state (`completed` or `failed`) and has exceeded its TTL, it is failed with `"TTL expired"`. This prevents abandoned tasks from accumulating indefinitely.

The 30-second sweep interval means a task can live up to 30 seconds beyond its TTL before being expired. This is acceptable for a system with 300-second default TTLs -- the extra 30 seconds is noise. For shorter TTLs (the `mesh_send` fallback uses 60 seconds), the imprecision is slightly more noticeable but still tolerable.

### Terminal states are permanent

Once a task reaches `completed` or `failed`, the expiration loop skips it:

```rust
if task.state == "completed" || task.state == "failed" {
    continue;
}
```

There is no mechanism to retry, re-queue, or resurrect a failed task. The source agent is responsible for deciding whether to resubmit.

---

## 4. The Dual Execution Path

The most important design pattern in chatixia-mesh is the **dual execution path**: every operation that sends work to another agent tries the fast P2P DataChannel first, then falls back to the slower HTTP task queue if P2P is unavailable.

This pattern is implemented in `handle_delegate` in `agent/chatixia/core/mesh_skills.py`. Let's walk through both paths in detail.

### Path 1: P2P DataChannel (fast path, <100ms typical)

The P2P path is taken when three conditions are all true:

1. The `_mesh_client` exists and is connected to the sidecar (`_mesh_client.connected`).
2. The target agent's sidecar peer is in the connected peers set (`_mesh_client.is_peer_connected(target_peer)`).
3. No exception occurs during the P2P send.

Here is the code:

```python
# agent/chatixia/core/mesh_skills.py -- handle_delegate (P2P path)

if _mesh_client and _mesh_client.connected:
    target_peer = f"{target_agent_id}-sidecar"

    if _mesh_client.is_peer_connected(target_peer):
        msg = MeshMessage(
            msg_type="task_request",
            source_agent=agent_id,
            target_agent=target_agent_id,
            payload={"message": message, "skill": skill},
        )

        if not wait:
            await _mesh_client.send(target_peer, msg)
            return f"Task delegated to {target_agent_id} via P2P (fire-and-forget)"

        try:
            response = await _mesh_client.request(target_peer, msg, timeout=120.0)
            payload = response.get("payload", {})
            error = payload.get("error", "")
            if error:
                return f"Task failed: {error}"
            return payload.get("result", "(no result)")
        except asyncio.TimeoutError:
            return f"Timeout: P2P task to {target_agent_id} timed out after 120s"
        except Exception as e:
            logger.warning("P2P delegate failed, falling back to registry: %s", e)
            # Fall through to HTTP fallback
```

The critical call is `_mesh_client.request()`. Let's trace exactly what it does:

```python
# agent/chatixia/core/mesh_client.py -- MeshClient.request()

async def request(self, target_peer, message, timeout=30.0):
    if not message.request_id:
        message.request_id = uuid.uuid4().hex[:12]

    future = loop.create_future()
    self._pending_responses[message.request_id] = future

    await self.send(target_peer, message)

    try:
        return await asyncio.wait_for(future, timeout=timeout)
    finally:
        self._pending_responses.pop(message.request_id, None)
```

Step by step:

1. **Generate a request_id** -- A 12-character hex string from UUID4. This is the correlation key.
2. **Register a pending future** -- The `_pending_responses` dict maps `request_id` to an asyncio `Future`. When a response arrives with the same `request_id`, the future is resolved.
3. **Send the message** -- The `MeshMessage` is serialized to JSON, wrapped in an `IpcMessage` with type `"send"`, and written to the Unix socket.
4. **Wait for the response** -- `asyncio.wait_for` blocks until the future resolves or the timeout fires.
5. **Cleanup** -- Whether the response arrives or times out, the pending entry is removed.

On the receiving side, the `_dispatch` method in `MeshClient` checks incoming messages for matching `request_id` values:

```python
# agent/chatixia/core/mesh_client.py -- _dispatch()

if msg_type == "message":
    payload = data.get("payload", {})
    inner = payload.get("message", {})
    req_id = inner.get("request_id", "")
    if req_id and req_id in self._pending_responses:
        self._pending_responses[req_id].set_result(inner)
        return
```

This is a simple correlation mechanism: send a request with a unique ID, store a future keyed by that ID, resolve the future when a response carrying the same ID arrives.

### Complete P2P message sequence

```
Agent A           Sidecar A          Sidecar B           Agent B
   |                  |                  |                  |
   |--IpcMessage----->|                  |                  |
   |  type:"send"     |                  |                  |
   |  payload:        |                  |                  |
   |    target_peer:  |                  |                  |
   |    "agent-b-sc"  |                  |                  |
   |    message:      |                  |                  |
   |      MeshMessage |                  |                  |
   |      type:       |                  |                  |
   |      "task_req"  |                  |                  |
   |      request_id: |                  |                  |
   |      "a1b2c3..."  |                  |                  |
   |                  |---MeshMessage--->|                  |
   |                  |  (DataChannel)   |                  |
   |                  |                  |--IpcMessage----->|
   |                  |                  |  type:"message"  |
   |                  |                  |  payload:        |
   |                  |                  |    from_peer:    |
   |                  |                  |    message:      |
   |                  |                  |      MeshMessage |
   |                  |                  |                  |
   |                  |                  |                  |
   |                  |                  |  (agent processes|
   |                  |                  |   the task)      |
   |                  |                  |                  |
   |                  |                  |<--IpcMessage-----|
   |                  |                  |  type:"send"     |
   |                  |                  |  payload:        |
   |                  |                  |    target_peer:  |
   |                  |                  |    message:      |
   |                  |                  |      MeshMessage |
   |                  |                  |      type:       |
   |                  |                  |      "task_resp" |
   |                  |                  |      request_id: |
   |                  |                  |      "a1b2c3..." |
   |                  |<--MeshMessage----|                  |
   |                  |  (DataChannel)   |                  |
   |<--IpcMessage-----|                  |                  |
   |  type:"message"  |                  |                  |
   |  payload:        |                  |                  |
   |    message:      |                  |                  |
   |      request_id: |                  |                  |
   |      "a1b2c3..." |                  |                  |
   |                  |                  |                  |
   | Future resolved  |                  |                  |
   | (matched by      |                  |                  |
   |  request_id)     |                  |                  |
```

Total hops: 6 (3 in each direction). Total network crossings: 2 (one DataChannel message each way). Typical latency: <100ms on a LAN, depending on agent processing time.

### Path 2: HTTP task queue (fallback path, 3-15s typical)

If the P2P path is unavailable (mesh client not connected, target peer not reachable, or P2P send failed with an exception), `handle_delegate` falls through to the HTTP fallback:

```python
# agent/chatixia/core/mesh_skills.py -- handle_delegate (HTTP fallback)

result = _post(
    f"{registry}/api/hub/tasks",
    {
        "skill": skill,
        "target_agent_id": target_agent_id,
        "source_agent_id": agent_id,
        "payload": {"message": message},
        "ttl": 300,
    },
)

task_id = result.get("task_id", "")
if not task_id:
    return f"Error: Failed to submit task: {result}"

if not wait:
    return f"Task submitted via registry: task_id={task_id}"

# Poll for result
deadline = asyncio.get_event_loop().time() + 120
while asyncio.get_event_loop().time() < deadline:
    await asyncio.sleep(3)
    status = _get(f"{registry}/api/hub/tasks/{task_id}")
    state = status.get("state", "pending")
    if state == "completed":
        return status.get("result", "(no result)")
    if state == "failed":
        return f"Task failed: {status.get('error', 'unknown')}"

return f"Timeout: task {task_id} still pending after 120s"
```

Step by step:

1. **Submit the task** -- POST to `/api/hub/tasks`. The registry creates a `Task` with state `"pending"` and returns a `task_id`.
2. **Poll for completion** -- Every 3 seconds, GET `/api/hub/tasks/{task_id}` and check the state. Continue until `completed`, `failed`, or the 120-second deadline passes.

Meanwhile, on the target agent's side, a separate mechanism (the agent's task polling loop) calls `get_pending_for_agent()` on the registry, which atomically claims matching tasks by setting them to `"assigned"`. The agent executes the task and POSTs the result back to `/api/hub/tasks/{task_id}` with `state: "completed"` or `state: "failed"`.

### Complete HTTP fallback sequence

```
Agent A                Registry                Agent B
   |                      |                      |
   |--POST /api/hub/----->|                      |
   |  tasks               |                      |
   |  {skill, target,     |                      |
   |   payload, ttl:300}  |                      |
   |                      |                      |
   |<--{task_id}----------|                      |
   |                      |                      |
   |                      |<--poll/claim---------|
   |                      |  get_pending_for_    |
   |                      |  agent("agent-b",    |
   |                      |  ["summarize",...])   |
   |                      |                      |
   |                      |--[task]------------->|
   |                      |  state: "assigned"   |
   |                      |                      |
   |  (polling every 3s)  |                      |
   |--GET /api/hub/------>|                      |
   |  tasks/{task_id}     |                      |
   |<--state:"assigned"---|                      |
   |                      |                      |
   |  (3 seconds later)   |   (agent processes)  |
   |--GET /api/hub/------>|                      |
   |  tasks/{task_id}     |                      |
   |<--state:"assigned"---|                      |
   |                      |                      |
   |                      |<--POST /api/hub/-----|
   |                      |  tasks/{task_id}     |
   |                      |  {state:"completed", |
   |                      |   result:"..."}      |
   |                      |                      |
   |  (3 seconds later)   |                      |
   |--GET /api/hub/------>|                      |
   |  tasks/{task_id}     |                      |
   |<--state:"completed"--|                      |
   |   result:"..."       |                      |
```

Total HTTP requests: at minimum 4 (1 submit + 1 claim + 1 result update + 1 successful poll), typically more due to polling. Latency is dominated by the 3-second polling interval -- even if the task completes in 50ms, the requester will not learn about it for up to 3 seconds.

### Comparing the two paths

| Aspect | P2P DataChannel | HTTP Task Queue |
|--------|----------------|-----------------|
| Latency | <100ms (LAN), 50-200ms (internet) | 3-15s (dominated by poll interval) |
| Hops | Agent -> Sidecar -> DataChannel -> Sidecar -> Agent (direct) | Agent -> Registry -> Agent -> Registry -> Agent (mediated) |
| Reliability | Requires active DataChannel | Always works if registry is reachable |
| Registry load | None (registry not involved) | 4+ HTTP requests per task |
| Correlation | `request_id` matching in asyncio Future | `task_id` matching via polling |
| Timeout | 120s (configurable per call) | 120s polling deadline + 300s TTL |
| Encryption | DTLS (peer-to-peer, end-to-end) | TLS to registry (registry sees content) |

---

## 5. Graceful Degradation

The dual execution path is part of a broader design principle: **the system never fails, it only slows down**. chatixia-mesh implements a three-tier transport hierarchy. Each tier has worse performance characteristics but broader reachability.

```
Tier 1: Direct P2P          Tier 2: TURN Relay          Tier 3: HTTP Fallback
+----------------+         +-------------------+         +-------------------+
|                |         |                   |         |                   |
|  Agent A       |         |  Agent A          |         |  Agent A          |
|    |           |         |    |              |         |    |              |
|  Sidecar A     |         |  Sidecar A        |         |    |              |
|    |           |         |    |              |         |    |              |
|    | UDP       |         |    | UDP          |         |    | HTTP         |
|    | (direct)  |         |    v              |         |    v              |
|    |           |         |  TURN Server      |         |  Registry         |
|    |           |         |    |              |         |    |              |
|    | UDP       |         |    | UDP          |         |    | HTTP         |
|    |           |         |    v              |         |    v              |
|  Sidecar B     |         |  Sidecar B        |         |    |              |
|    |           |         |    |              |         |    |              |
|  Agent B       |         |  Agent B          |         |  Agent B          |
+----------------+         +-------------------+         +-------------------+
```

### Transport tier reference

| Tier | Transport | Latency | When used | Privacy |
|------|-----------|---------|-----------|---------|
| 1 - Direct P2P | WebRTC DataChannel (UDP, direct) | <100ms | Both peers on same LAN, or both have public IPs, or NAT traversal succeeds via STUN | End-to-end encrypted (DTLS). Registry never sees content. |
| 2 - TURN Relay | WebRTC DataChannel (UDP, relayed) | 100-500ms | Direct connection fails (symmetric NAT, restrictive firewall). Traffic relayed through TURN server. | Encrypted (DTLS). TURN server sees encrypted packets but not content. |
| 3 - HTTP Fallback | HTTP task queue via registry | 3-15s | No WebRTC connectivity at all. Sidecar down, no TURN available, or network blocks UDP entirely. | Registry sees task content in plaintext (within TLS). |

The tier selection is implicit, not configured. ICE negotiation (Lesson 03) automatically tries direct, then STUN-assisted, then TURN-relayed connections. If all WebRTC options fail, the `handle_delegate` code detects that the peer is not connected and falls through to HTTP.

From the agent's perspective, a `delegate` call always returns a result (or times out). The agent does not need to know which tier was used. The only observable difference is latency.

### Why three tiers instead of just HTTP?

It would be simpler to route everything through the registry. But:

1. **Latency** -- P2P is 30-100x faster than HTTP polling. For interactive agent conversations, 3-15 seconds per message is unusable.
2. **Privacy** -- P2P messages are end-to-end encrypted. The registry (control plane) never sees data plane content.
3. **Scalability** -- P2P offloads traffic from the registry. With 10 agents sending 100 messages/second, that is 1000 req/s the registry does not need to handle.
4. **Resilience** -- If the registry goes down, agents with established DataChannels can continue communicating. Only new connections and task queue operations fail.

---

## 6. Fire-and-Forget vs Request/Response

chatixia-mesh supports two fundamentally different communication patterns: **fire-and-forget** (send a message, do not wait) and **request/response** (send a request, wait for a correlated response). Understanding when each is appropriate is a core protocol design skill.

### Fire-and-forget: `mesh_send` and `mesh_broadcast`

The `handle_mesh_send` and `handle_mesh_broadcast` functions in `mesh_skills.py` implement fire-and-forget messaging. They use the `agent_prompt` message type:

```python
# From handle_mesh_send
msg = MeshMessage(
    msg_type="agent_prompt",      # fire-and-forget type
    source_agent=agent_id,
    target_agent=target_agent_id,
    payload={"message": message, "direct": True},
)
await _mesh_client.send(target_peer, msg)   # send(), not request()
```

Key characteristics:

- Uses `_mesh_client.send()`, which writes to the socket and returns immediately.
- No `request_id` is set (defaults to empty string).
- No response is expected. The sender does not know if the message was received, processed, or even delivered.
- The `agent_prompt` message type signals to the receiver that no response is required.
- Broadcasts set `target_agent` to `"*"` and use `_mesh_client.broadcast()`.

Fire-and-forget is appropriate for:
- Status announcements ("I am online with these skills")
- Notifications ("A new document has been indexed")
- Broadcasts where individual responses would overwhelm the sender

### Request/response: `delegate`

The `handle_delegate` function implements request/response. It uses the `task_request` / `task_response` message types:

```python
# From handle_delegate (P2P path)
msg = MeshMessage(
    msg_type="task_request",       # request/response type
    source_agent=agent_id,
    target_agent=target_agent_id,
    payload={"message": message, "skill": skill},
)
response = await _mesh_client.request(target_peer, msg, timeout=120.0)
```

Key characteristics:

- Uses `_mesh_client.request()`, which generates a `request_id`, registers a future, sends the message, and blocks until the response arrives or the timeout fires.
- The receiver sends back a `task_response` with the same `request_id`.
- The sender's `_dispatch` method matches the `request_id` and resolves the pending future.
- Timeout is explicit (120 seconds for delegate, 30 seconds default in `request()`).

Request/response is appropriate for:
- Task delegation ("Summarize this document and give me the result")
- Skill queries ("What can you do?")
- Any operation where the sender needs the output to continue

### The `wait` parameter

`handle_delegate` has a `wait` parameter that converts it from request/response to fire-and-forget:

```python
if not wait:
    await _mesh_client.send(target_peer, msg)
    return f"Task delegated to {target_agent_id} via P2P (fire-and-forget)"
```

When `wait=False`, the delegate sends a `task_request` (which normally expects a response) but does not wait for one. This is a hybrid: the receiver still processes it as a task and may send a response, but the sender has moved on. This is useful for asynchronous workflows where the result can be consumed later through a different channel.

### Summary table

| Aspect | Fire-and-forget | Request/response |
|--------|----------------|------------------|
| Skills | `mesh_send`, `mesh_broadcast` | `delegate` |
| Message type | `agent_prompt` | `task_request` / `task_response` |
| `request_id` | Empty | Generated UUID (12 hex chars) |
| Client method | `MeshClient.send()` / `broadcast()` | `MeshClient.request()` |
| Blocking | No | Yes (until response or timeout) |
| Delivery guarantee | None (best effort) | Timeout detection (not guaranteed delivery) |
| HTTP fallback | POST task with short TTL (60s), return immediately | POST task, poll for result every 3s |

---

## Exercises

### Exercise 1: Draw the complete task state machine

Draw the task state machine with all four states (`pending`, `assigned`, `completed`, `failed`) and every transition between them. For each transition, label:

- The function or handler that triggers the transition
- Who calls it (source agent, target agent, or background loop)
- What data changes on the `Task` struct

Include the "no transition" cases -- which state pairs have no valid transition? What happens if someone tries to update a task that is already `completed`?

Consult `registry/src/hub.rs` for the authoritative implementation.

### Exercise 2: Trace a P2P delegate call

Agent A calls `handle_delegate(message="Summarize X", target_agent_id="agent-b")` and Agent B is directly connected via DataChannel.

List every message that crosses a process boundary, in order. For each message, specify:

1. The sender and receiver processes (e.g., "Agent A Python" -> "Sidecar A")
2. The protocol used (IPC, DataChannel)
3. The message type and key fields
4. The direction (request or response)

Count the total number of messages. How many network hops occur? What is the minimum possible latency assuming 1ms per IPC crossing and 10ms per DataChannel crossing?

### Exercise 3: Trace the same call via HTTP fallback

Now trace the same `handle_delegate` call, but Agent B is NOT connected via DataChannel (the mesh client is `None` or the peer is not in the connected set).

List every HTTP request and response, in order. For each, specify:

1. The caller and the URL
2. The HTTP method (GET/POST)
3. The request body (for POST) or query parameters (for GET)
4. The response body

Assume Agent B's polling loop claims the task 2 seconds after submission, Agent B completes the task 1 second after claiming it, and Agent A's poll hits 3 seconds after submission. How many total HTTP requests occur? What is the end-to-end latency?

Compare your answer to Exercise 2. What is the latency ratio between the two paths?

### Exercise 4: Design a streaming protocol

The codebase defines `task_stream_chunk` and `agent_stream_chunk` message types but does not yet implement a complete streaming protocol. Design one.

Your protocol must handle:

1. **Stream initiation** -- How does the sender signal that a response will be streamed rather than delivered as a single message?
2. **Continuation** -- How does the receiver know more chunks are coming?
3. **Completion** -- How does the receiver know the stream is finished?
4. **Error during stream** -- How does the sender signal an error after some chunks have already been sent?
5. **Ordering** -- How does the receiver reconstruct the correct order if chunks arrive out of order?

Define the `payload` structure for each chunk type. Write example JSON messages for a three-chunk stream (start, middle, end) and an error case. Explain how `request_id` is used to correlate chunks with the original request.

Consider: should the existing `MeshMessage` envelope be sufficient, or does streaming require changes to the struct? What are the trade-offs of adding a `sequence_number` field to `MeshMessage` vs putting it in the payload?

---

## Summary

This lesson covered the application-level protocol that gives meaning to the bytes flowing through chatixia-mesh's transport layers.

**MeshMessage** is a five-field JSON envelope -- `type`, `request_id`, `source_agent`, `target_agent`, `payload` -- that carries all agent-to-agent communication. Its deliberately minimal design makes it easy to implement in multiple languages and extend with new message types.

**Tasks** follow a four-state lifecycle (`pending` -> `assigned` -> `completed` | `failed`) managed by the registry's hub module. TTL-based expiration prevents abandoned tasks from accumulating.

**The dual execution path** tries P2P DataChannel first (fast, private, decentralized) and falls back to the HTTP task queue (slower, mediated, but always available). The `request_id` field enables request/response correlation over the stateless DataChannel; the `task_id` field serves the same purpose over HTTP polling.

**Graceful degradation** across three transport tiers -- direct P2P, TURN relay, HTTP fallback -- means the system never stops working. It only slows down.

**Fire-and-forget** (`mesh_send`, `mesh_broadcast`) and **request/response** (`delegate`) are the two fundamental communication patterns, each with appropriate message types and client methods.

In the next lesson, [Lesson 08: Authentication and Security](08-authentication-and-security.md), you will learn how the system verifies agent identity and protects these messages from tampering and eavesdropping.
