# Lesson 05: Designing a Signaling Protocol -- JSON over WebSocket

**Prerequisites:** [Lesson 02: Peer-to-Peer Networking](02-peer-to-peer-networking.md), [Lesson 03: WebRTC Fundamentals](03-webrtc-fundamentals.md)

**Time estimate:** 60-90 minutes

**Key source files:**
- `sidecar/src/protocol.rs` -- `SignalingMessage` struct definition
- `registry/src/signaling.rs` -- `handle_message` function, message relay logic
- `sidecar/src/signaling.rs` -- `handle_signaling_message`, sidecar-side processing
- `registry/src/main.rs` -- WebSocket upgrade handler, sender verification
- `registry/src/pairing.rs` -- `approved_peer_ids`, peer list filtering

---

## What is a Protocol?

A protocol is a contract between two or more systems about how they will communicate. It defines three things:

1. **Message format** -- the structure and encoding of data on the wire
2. **Message sequence** -- the order in which messages are exchanged
3. **Message semantics** -- what each message means and what the receiver should do

Consider HTTP. Its format is text headers followed by an optional body. Its sequence is request-then-response. Its semantics are defined by methods (GET means retrieve, POST means create). Any HTTP client can talk to any HTTP server because both sides agree on all three dimensions.

Protocols exist at every layer of a system. TCP defines how bytes are delivered reliably. TLS defines how encryption is negotiated. WebSocket defines how messages are framed over a persistent connection. And on top of all of those, applications define their own protocols for their specific needs.

chatixia-mesh defines three application protocols:

| Protocol | Transport | Purpose |
|----------|-----------|---------|
| **Signaling** | JSON over WebSocket | Coordinate WebRTC connection setup between peers |
| **Mesh** | JSON over WebRTC DataChannel | Agent-to-agent communication (tasks, prompts, status) |
| **IPC** | JSON lines over Unix socket | Bridge between sidecar and Python agent |

This lesson focuses on the first: the signaling protocol. It is the smallest and most critical of the three. Without it, no two peers can ever establish a direct connection.

### Why Define Your Own Protocol?

You might ask: why not just use an existing protocol like gRPC or MQTT?

The answer is that signaling has very specific requirements that generic protocols address poorly:

- **Low message volume, high importance.** A typical signaling exchange involves fewer than 10 messages, but each one is essential for connection setup. Heavyweight frameworks add latency and complexity for no benefit.
- **Asymmetric routing.** Messages must be relayed from one peer to another through a central server. This is not request-response (HTTP) or pub-sub (MQTT) -- it is directed relay.
- **Tight integration with WebRTC.** The payload contains SDP and ICE data whose format is defined by WebRTC standards. The signaling protocol is a thin envelope around WebRTC-specific content.

JSON over WebSocket gives us exactly what we need: persistent connections, low overhead, human-readable messages for debugging, and the ability to add new message types without changing infrastructure.

---

## The SignalingMessage Struct

The signaling protocol is built on a single message type with four fields. Here is the definition from `sidecar/src/protocol.rs`:

```rust
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SignalingMessage {
    #[serde(rename = "type")]
    pub msg_type: String,
    pub peer_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_id: Option<String>,
    #[serde(default)]
    pub payload: serde_json::Value,
}
```

And on the wire, a signaling message looks like this:

```json
{
  "type": "offer",
  "peer_id": "agent-001",
  "target_id": "agent-002",
  "payload": { "sdp": "v=0\r\no=- 123456 ..." }
}
```

Four fields. That is the entire protocol definition. Let us examine each one.

### `type` -- What Kind of Message

The `type` field determines how the message is processed. The registry and sidecar both use `match` statements on this value. There are five recognized types:

| Type | Direction | Purpose |
|------|-----------|---------|
| `register` | Sidecar -> Registry | "I'm online, tell me who else is here" |
| `peer_list` | Registry -> Sidecar | "Here are the other peers you can connect to" |
| `offer` | Sidecar -> Registry -> Sidecar | WebRTC SDP offer, relayed to target peer |
| `answer` | Sidecar -> Registry -> Sidecar | WebRTC SDP answer, relayed back to offerer |
| `ice_candidate` | Sidecar -> Registry -> Sidecar | ICE candidate, relayed to target peer |

There is also `heartbeat` (keep-alive, no action) and the catch-all for unknown types (logged and ignored).

Note the `#[serde(rename = "type")]` annotation. In Rust, `type` is a reserved keyword, so the struct field is named `msg_type`. The serde annotation ensures it serializes as `"type"` in JSON, which is the idiomatic key name for discriminated unions in JSON protocols.

### `peer_id` -- Who Sent This

Every signaling message carries the sender's `peer_id`. This serves two purposes:

1. **Routing.** When the registry relays a message, the recipient needs to know who sent it. For example, when Sidecar B receives an offer, it needs to know to send the answer back to Sidecar A.
2. **Verification.** The registry checks that the `peer_id` in the message matches the identity established during WebSocket authentication. This prevents spoofing (covered in detail in the Sender Verification section below).

### `target_id` -- Who Should Receive This

The `target_id` field is optional. It is present on directed messages (`offer`, `answer`, `ice_candidate`) and absent on broadcast-style messages (`register`).

The `#[serde(skip_serializing_if = "Option::is_none")]` annotation means the field is omitted entirely from the JSON when it is `None`. A `register` message looks like this on the wire:

```json
{
  "type": "register",
  "peer_id": "agent-001",
  "payload": null
}
```

No `target_id` key at all. This keeps messages compact and avoids ambiguity between "no target" and "empty target."

### `payload` -- The Data

The `payload` field carries type-specific data. Its structure depends on the `type`:

| Message Type | Payload Content |
|-------------|-----------------|
| `register` | null or empty (no data needed) |
| `peer_list` | `{ "peers": ["agent-002", "agent-003"] }` |
| `offer` | `{ "sdp": "v=0\r\no=- ..." }` |
| `answer` | `{ "sdp": "v=0\r\no=- ..." }` |
| `ice_candidate` | `{ "candidate": "candidate:...", "sdpMid": "0", "sdpMLineIndex": 0 }` |

The type is `serde_json::Value` -- a dynamically-typed JSON value. This is a deliberate design choice. The signaling layer does not need to understand the contents of SDP strings or ICE candidates. It just needs to deliver them to the right peer. Using a generic JSON value means the registry can relay messages without parsing their payloads.

### Why JSON, Not Protobuf?

Protocol Buffers (Protobuf) would give us smaller messages and compile-time type checking of payloads. So why JSON?

1. **Debuggability.** Signaling messages can be read directly in WebSocket inspection tools (browser dev tools, `wscat`, Wireshark). When a connection fails to establish, being able to read the SDP in a WebSocket frame is invaluable.

2. **SDP is already text.** The largest part of any signaling payload is the SDP string, which is a multi-line text format defined by RFC 8866. Wrapping a text blob in a binary encoding saves almost nothing.

3. **Low volume.** A full peer connection setup involves roughly 6-10 signaling messages. Even at 100 peers (4,950 possible connections), the total signaling traffic is negligible. The performance gains from binary encoding would be unmeasurable.

4. **Flexibility.** The `payload` field can carry any JSON structure. Adding a new message type requires no schema changes, no code generation, and no version negotiation. You just start sending messages with a new `type` value.

5. **Ecosystem compatibility.** Every language has a JSON parser. The hub dashboard (TypeScript), future web clients, and debugging scripts can all parse signaling messages without a Protobuf compiler.

The trade-off is real but acceptable: JSON parsing is slower than Protobuf deserialization, and JSON messages are larger. For a protocol that handles tens of messages per connection setup, neither matters.

---

## Sequence Diagram: Two Peers Connecting

The following diagram shows every signaling message exchanged when Sidecar A and Sidecar B establish a WebRTC connection. Read it from top to bottom; time flows downward.

```
  Sidecar A                    Registry                    Sidecar B
     |                            |                            |
     |--- POST /api/token ------->|                            |
     |<-- { token, peer_id } ----|                            |
     |                            |                            |
     |--- WebSocket /ws?token --->|                            |
     |<-- [connection upgrade] ---|                            |
     |                            |                            |
     |  (1) register              |                            |
     |--- { type: "register",  -->|                            |
     |     peer_id: "A" }         |                            |
     |                            |                            |
     |  (2) peer_list             |                            |
     |<-- { type: "peer_list", ---|                            |
     |     payload: { peers:[] }} |                            |
     |                            |                            |
     |                            |<--- POST /api/token -------|
     |                            |--- { token, peer_id } ---->|
     |                            |                            |
     |                            |<--- WebSocket /ws?token ---|
     |                            |--- [connection upgrade] -->|
     |                            |                            |
     |                            |  (3) register              |
     |                            |<-- { type: "register",  ---|
     |                            |     peer_id: "B" }         |
     |                            |                            |
     |                            |  (4) peer_list             |
     |                            |--- { type: "peer_list", -->|
     |                            |     payload: {peers:["A"]}}|
     |                            |                            |
     |                            |  (5) offer                 |
     |  (5) offer                 |<-- { type: "offer",     ---|
     |<-- { type: "offer",     ---|     peer_id: "B",          |
     |     peer_id: "B",          |     target_id: "A",        |
     |     target_id: "A",        |     payload: {sdp:"..."} } |
     |     payload: {sdp:"..."} } |                            |
     |                            |                            |
     |  (6) answer                |                            |
     |--- { type: "answer",    -->|  (6) answer                |
     |     peer_id: "A",          |--- { type: "answer",    -->|
     |     target_id: "B",        |     peer_id: "A",          |
     |     payload: {sdp:"..."} } |     target_id: "B",        |
     |                            |     payload: {sdp:"..."} } |
     |                            |                            |
     |  (7-N) ice_candidate       |  (7-N) ice_candidate       |
     |--- { type: "ice_cand",  -->|--- { type: "ice_cand",  -->|
     |     peer_id: "A",          |     peer_id: "A",          |
     |     target_id: "B" }       |     target_id: "B" }       |
     |                            |                            |
     |  (7-N) ice_candidate       |  (7-N) ice_candidate       |
     |<-- { type: "ice_cand",  ---|<-- { type: "ice_cand",  ---|
     |     peer_id: "B",          |     peer_id: "B",          |
     |     target_id: "A" }       |     target_id: "A" }       |
     |                            |                            |
     |============= ICE connectivity checks ==================|
     |                            |                            |
     |<========= DTLS handshake =============================>|
     |                            |                            |
     |<========= DataChannel open ============================>|
     |                            |                            |
```

Let us trace each step through the actual code.

### Step 0: Token Exchange (Before Signaling)

Before connecting to the WebSocket, each sidecar exchanges its API key (or device token) for a JWT. This happens in `sidecar/src/signaling.rs`:

```rust
pub async fn exchange_token(token_url: &str, api_key: &str) -> Result<TokenResponse> {
    let client = reqwest::Client::new();
    let resp = client
        .post(token_url)
        .header("x-api-key", api_key)
        .send()
        .await?
        .json::<TokenResponse>()
        .await?;
    Ok(resp)
}
```

The response includes a JWT and the `peer_id` assigned to this API key. The JWT contains a `sub` claim set to the `peer_id`, signed by the registry's `SIGNALING_SECRET`.

### Step 1: WebSocket Upgrade and Registration

The sidecar connects to `ws://registry:8080/ws?token=<JWT>`. The registry validates the JWT before upgrading the connection. From `registry/src/main.rs`:

```rust
async fn ws_upgrade(
    ws: WebSocketUpgrade,
    Query(params): Query<WsParams>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let claims = match state.auth.validate_token(&params.token) {
        Ok(c) => c,
        Err(e) => {
            error!("[WS] invalid token: {}", e);
            return StatusCode::UNAUTHORIZED.into_response();
        }
    };
    let peer_id = claims.sub.clone();
    ws.on_upgrade(move |socket| handle_ws(socket, peer_id, state))
        .into_response()
}
```

The `peer_id` is extracted from the JWT `sub` claim and passed into the WebSocket handler. This is the identity that will be used for all sender verification during this connection.

After the WebSocket is established, the sidecar sends a `register` message:

```rust
let register = SignalingMessage {
    msg_type: "register".into(),
    peer_id: peer_id.to_string(),
    target_id: None,
    payload: serde_json::Value::Null,
};
ws_write.send(Message::Text(serde_json::to_string(&register)?.into())).await?;
```

### Steps 2-4: Peer List Response

When the registry receives a `register` message, it responds with a `peer_list` containing all other authorized, connected peers. From `registry/src/signaling.rs`:

```rust
"register" => {
    let peers: Vec<String> = if is_authorized(&msg.peer_id) {
        self.connected_peers()
            .into_iter()
            .filter(|p| p != &msg.peer_id && is_authorized(p))
            .collect()
    } else {
        vec![]
    };
    if let Some(sender) = self.peers.get(&msg.peer_id) {
        let response = serde_json::json!({
            "type": "peer_list",
            "peer_id": "registry",
            "payload": { "peers": peers }
        });
        let _ = sender.send(serde_json::to_string(&response).unwrap());
    }
}
```

When A registers first, no other peers are connected, so A receives an empty `peer_list`. When B registers later, it receives a `peer_list` containing A. This tells B: "A is online and ready for connection."

Notice that the `peer_list` response has `peer_id: "registry"`. This is one of the few messages the registry generates itself rather than relaying from another peer.

### Step 5: Offer

When the sidecar receives a `peer_list`, it initiates WebRTC connections to each peer it is not already connected to. From `sidecar/src/signaling.rs`:

```rust
"peer_list" => {
    if let Some(peers) = msg.payload.get("peers").and_then(|p| p.as_array()) {
        for peer_val in peers {
            if let Some(pid) = peer_val.as_str() {
                if pid != local_peer_id && !mesh.is_connected(pid) {
                    tokio::spawn(async move {
                        webrtc_peer::initiate_connection(
                            &local_id, &target_id, sig_tx, mesh, to_agent,
                        ).await
                    });
                }
            }
        }
    }
}
```

The `initiate_connection` function creates an `RTCPeerConnection`, generates an SDP offer, and sends it through the signaling channel. The registry sees an `offer` message and relays it to the target peer.

### Step 6: Answer

When a sidecar receives an `offer`, it creates its own `RTCPeerConnection`, sets the remote description from the offer's SDP, generates an SDP answer, and sends it back through signaling:

```rust
"offer" => {
    let from_peer = msg.peer_id.clone();
    if let Some(sdp) = msg.payload.get("sdp").and_then(|s| s.as_str()) {
        tokio::spawn(async move {
            webrtc_peer::handle_offer(
                &local_id, &from_peer, &sdp, sig_tx, mesh, to_agent,
            ).await
        });
    }
}
```

The registry relays the answer back to the offerer, who sets it as the remote description:

```rust
"answer" => {
    let from_peer = msg.peer_id.clone();
    if let Some(sdp) = msg.payload.get("sdp").and_then(|s| s.as_str()) {
        if let Some(pc) = mesh.get_pc(&from_peer) {
            let answer = RTCSessionDescription::answer(sdp.to_string()).unwrap();
            pc.set_remote_description(answer).await;
        }
    }
}
```

### Steps 7-N: ICE Candidates

While the SDP exchange is happening, both sides begin gathering ICE candidates (possible network paths). Each candidate is sent through signaling and added to the remote peer connection:

```rust
"ice_candidate" => {
    let from_peer = msg.peer_id.clone();
    if let Some(pc) = mesh.get_pc(&from_peer) {
        let candidate = msg.payload.get("candidate")
            .and_then(|c| c.as_str()).unwrap_or("").to_string();
        let sdp_mid = msg.payload.get("sdpMid")
            .and_then(|s| s.as_str()).map(|s| s.to_string());
        let sdp_mline_index = msg.payload.get("sdpMLineIndex")
            .and_then(|n| n.as_u64()).map(|n| n as u16);
        let init = RTCIceCandidateInit {
            candidate, sdp_mid, sdp_mline_index,
            username_fragment: Some(String::new()),
        };
        pc.add_ice_candidate(init).await;
    }
}
```

The ICE candidate payload contains three fields following the WebRTC standard: `candidate` (the candidate string), `sdpMid` (which media stream it applies to), and `sdpMLineIndex` (the index in the SDP).

Multiple candidates are typically exchanged in both directions. The number depends on the network configuration -- a machine with multiple network interfaces will produce more candidates than one with a single interface.

### After Signaling: DataChannel Opens

Once ICE negotiation finds a working path and DTLS encrypts the connection, the DataChannel opens. From this point forward, the peers communicate directly. The signaling server is no longer involved in their conversation.

This is a critical property of the architecture: **signaling is transient, data flow is direct.** The registry handles the introduction, then steps aside.

---

## Sender Verification

The signaling protocol includes a critical security check: every incoming message's `peer_id` must match the identity established during WebSocket authentication. This happens in the `handle_ws` function in `registry/src/main.rs`:

```rust
Some(Ok(Message::Text(text))) => {
    let text_str: &str = text.as_ref();
    if let Ok(sm) = serde_json::from_str::<signaling::SignalingMessage>(text_str) {
        if sm.peer_id != peer_id {
            error!("[WS] peer_id mismatch: expected={}, got={}",
                   peer_id, sm.peer_id);
            continue;
        }
        let approved = state.pairing.approved_peer_ids();
        let legacy = state.auth.api_key_peer_ids();
        state.signaling.handle_message(sm, &approved, &legacy);
    }
}
```

Let us trace the trust chain:

1. The sidecar exchanges an API key for a JWT. The JWT's `sub` claim is set by the registry based on the API key's mapping (from `api_keys.json` or the pairing system). The sidecar cannot choose its own `peer_id`.
2. When the WebSocket connects, the JWT is validated and the `peer_id` is extracted from `claims.sub`. This `peer_id` is bound to the WebSocket connection for its entire lifetime.
3. On every incoming message, the registry compares `sm.peer_id` (from the JSON) against the `peer_id` from step 2. If they do not match, the message is dropped.

This prevents a class of attacks where a compromised or malicious sidecar sends messages with a forged `peer_id`. Without this check, Sidecar A could send an `offer` with `peer_id: "B"`, tricking Sidecar C into thinking it is negotiating with B when it is really connecting to A.

### The Registry as Trusted Relay

The registry occupies a privileged position in the signaling protocol. It is the only entity that can:

- See all connected peers
- Deliver messages between arbitrary pairs of peers
- Verify sender identity against authenticated credentials
- Control which peers are visible to each other (through approval status filtering)

This is fundamentally a **trusted relay** model. Peers must trust the registry to:

1. Not tamper with relayed messages
2. Not fabricate messages from non-existent peers
3. Correctly enforce sender verification
4. Correctly filter the peer list based on approval status

The signaling protocol does not include end-to-end encryption or message signing between peers. The trust model assumes the registry is honest. This is a reasonable assumption because the registry is operated by the same entity that runs the mesh network. If the registry is compromised, the attacker has far more powerful attacks available (issuing fake JWTs, for example) than forging signaling messages.

---

## Peer List Filtering

Not every connected peer appears in every `peer_list` response. The registry filters the list based on each agent's pairing status.

### The Pairing Status Lifecycle

When an agent goes through the pairing system, it passes through a series of statuses defined in `registry/src/pairing.rs`:

```
pending_approval  -->  approved  -->  revoked
                  \
                   -->  rejected
```

| Status | Meaning |
|--------|---------|
| `pending_approval` | Agent has redeemed an invite code, waiting for admin approval |
| `approved` | Admin has approved the agent; it can participate in the mesh |
| `rejected` | Admin has rejected the agent; it cannot join |
| `revoked` | Previously approved agent whose access has been revoked |

### Two Categories of Authorized Peers

The `handle_message` function in `registry/src/signaling.rs` receives two sets of peer IDs:

```rust
pub fn handle_message(
    &self,
    msg: SignalingMessage,
    approved_peers: &HashSet<String>,
    legacy_peers: &HashSet<String>,
) {
    let is_authorized = |pid: &str| {
        approved_peers.contains(pid) || legacy_peers.contains(pid)
    };
    // ...
}
```

These sets are constructed in `handle_ws` (`registry/src/main.rs`) on every incoming message:

```rust
let approved = state.pairing.approved_peer_ids();
let legacy = state.auth.api_key_peer_ids();
state.signaling.handle_message(sm, &approved, &legacy);
```

- **`approved_peers`** -- Peer IDs of agents that went through the pairing system and were approved. Computed by `PairingState::approved_peer_ids()`, which filters onboarding entries where `status == "approved"`.
- **`legacy_peers`** -- Peer IDs of agents with static API keys (defined in `api_keys.json`). These are auto-authorized and bypass the pairing system.

A peer is authorized if it appears in either set.

### How Filtering Affects the Peer List

When a `register` message arrives, the registry builds the peer list differently depending on the sender's authorization:

```rust
"register" => {
    let peers: Vec<String> = if is_authorized(&msg.peer_id) {
        self.connected_peers()
            .into_iter()
            .filter(|p| p != &msg.peer_id && is_authorized(p))
            .collect()
    } else {
        vec![]
    };
    // ...
}
```

Two rules:

1. **Authorized peers see only other authorized peers.** An approved agent will not see pending, rejected, or revoked peers in its `peer_list`. This prevents unapproved agents from participating in the mesh even if they manage to connect to the WebSocket.

2. **Unauthorized peers see nobody.** A pending or revoked agent receives an empty `peer_list`. It can maintain its WebSocket connection (useful for the pairing flow), but it cannot discover or connect to any other peer.

### Why Filter On Every Message?

Notice that the approved and legacy sets are recomputed on every incoming WebSocket message, not cached at connection time:

```rust
// Inside the message loop:
let approved = state.pairing.approved_peer_ids();
let legacy = state.auth.api_key_peer_ids();
```

This means revocation takes effect immediately. If an admin revokes an agent while it is connected, the next `register` message from any peer will exclude the revoked agent from the peer list. The revoked agent's existing WebRTC connections remain open (signaling cannot close DataChannels), but no new connections will be established to it.

---

## Protocol Evolution

The signaling protocol is intentionally simple, but systems grow. How do you add new message types without breaking existing clients?

### The Unknown-Type Pattern

Look at how both the registry and sidecar handle unrecognized message types:

**Registry** (`registry/src/signaling.rs`):
```rust
other => {
    warn!("[SIG] unknown message type: {}", other);
}
```

**Sidecar** (`sidecar/src/signaling.rs`):
```rust
_ => {
    warn!("[SIG] unhandled message type: {}", msg.msg_type);
}
```

Neither side panics, returns an error, or disconnects. Unrecognized types are logged and ignored. This is the fundamental backward compatibility mechanism: **a new message type is invisible to old clients.**

### Adding a New Message Type: A Walkthrough

Suppose you want to add a `ping`/`pong` message for WebSocket-level health checking (separate from the existing `heartbeat` which is currently a no-op). Here is what you would need to do:

1. **Define the semantics.** A sidecar sends `{ "type": "ping", "peer_id": "A" }`. The registry responds with `{ "type": "pong", "peer_id": "registry" }`.

2. **Add the handler in the registry.** Add a new arm to the `match` in `handle_message`:

```rust
"ping" => {
    if let Some(sender) = self.peers.get(&msg.peer_id) {
        let response = serde_json::json!({
            "type": "pong",
            "peer_id": "registry",
            "payload": { "timestamp": chrono::Utc::now().timestamp_millis() }
        });
        let _ = sender.send(serde_json::to_string(&response).unwrap());
    }
}
```

3. **Handle the response in the sidecar.** Add a new arm to the `match` in `handle_signaling_message`:

```rust
"pong" => {
    info!("[SIG] received pong from registry");
}
```

4. **No changes needed for existing peers.** An old sidecar that does not know about `ping`/`pong` will simply log "unknown message type: pong" if it somehow receives one. No crash, no protocol error, no disconnection.

### What This Pattern Cannot Do

The unknown-type pattern handles additive changes well. It does not handle:

- **Removing a required message type.** If a future version of the registry stops sending `peer_list` responses to `register` messages, old sidecars will hang forever waiting for the list.
- **Changing the meaning of an existing type.** If `offer` starts carrying a different payload format, old sidecars will fail to parse the SDP.
- **Changing the envelope structure.** If you rename `peer_id` to `sender_id`, everything breaks.

For these kinds of changes, you would need a version negotiation mechanism -- for example, a `version` field in the `register` message. chatixia-mesh does not currently implement this, because the system is young and all components are deployed together. When the registry and sidecar are always released as a pair, envelope-level breaking changes can be coordinated.

### The Heartbeat Type: An Example of Graceful Evolution

The `heartbeat` type in the registry demonstrates how a message type can exist as a placeholder for future functionality:

```rust
"heartbeat" => {
    // Keep-alive, no action needed
}
```

The registry accepts `heartbeat` messages but does nothing with them. This allows sidecars to send periodic heartbeats to keep WebSocket connections alive (preventing proxy timeouts) without the registry needing to implement any response logic. If the registry later needs to track WebSocket-level liveness, the handler is already in place -- it just needs to be filled in.

---

## How the Pieces Fit Together

The signaling protocol is a thin layer that serves a single purpose: getting WebRTC peers past the bootstrapping problem. Two peers on different networks cannot exchange SDP offers directly because they do not know each other's network addresses yet. The registry acts as a rendezvous point where peers can find each other and exchange the information needed to establish direct connections.

Once the DataChannel opens, the signaling protocol's job is done. The mesh message protocol (covered in [Lesson 07](07-application-protocol-design.md)) takes over for application-level communication, running directly between peers without any registry involvement.

This separation has a concrete operational benefit: the registry can restart without interrupting active peer-to-peer conversations. Established DataChannels survive registry downtime. Only new connection setup is affected.

The code paths map cleanly to this mental model:

| Component | Responsibility | Does NOT do |
|-----------|---------------|-------------|
| `sidecar/src/protocol.rs` | Defines the message struct | Process or route messages |
| `registry/src/signaling.rs` | Relays messages between peers | Parse payloads, manage WebRTC state |
| `sidecar/src/signaling.rs` | Processes received messages, drives WebRTC | Relay to other peers |
| `registry/src/main.rs` | Authenticates and verifies senders | Understand message semantics |

Each component handles one concern and delegates the rest. The protocol struct is defined in one place and used identically on both sides -- there is no client-vs-server version mismatch risk because the same Rust type is compiled into both binaries.

---

## Exercises

### Exercise 1: Three-Agent Sequence Diagram

Draw a sequence diagram for three agents (A, B, C) joining the mesh in order: A registers first, then B, then C.

Questions to answer:
- How many `register` messages are sent in total?
- How many `peer_list` responses are sent?
- How many `offer` messages are sent?
- How many `answer` messages are sent?
- How many total signaling messages (excluding ICE candidates) are exchanged?
- Who initiates the offer to whom? (Hint: the peer that receives the other in its `peer_list` is the one that initiates.)

Consider: could the total number of offers be reduced if the registry sent updated peer lists to already-connected peers when a new peer registers?

### Exercise 2: Design a Mesh Health Check Message

Design a new signaling message type called `mesh_health` that allows a sidecar to request health information about the mesh from the registry.

Define:
- The request message format (what fields, what goes in the payload)
- The response message format (what data should the registry return)
- Where the handler would go in the registry code (which file, which function)
- How an old sidecar (that does not know about `mesh_health`) would handle receiving the response

Your design should follow the existing conventions: use the `SignalingMessage` struct, keep the payload as untyped JSON, and handle the unknown-type case gracefully.

### Exercise 3: Peer ID Mismatch

Trace what happens when a sidecar sends a message with a `peer_id` that does not match its JWT `sub` claim. Walk through the code path step by step:

1. Start at `handle_ws` in `registry/src/main.rs`
2. Identify the exact line where the check happens
3. What does the registry do with the mismatched message? (Drop it? Forward it? Close the connection?)
4. Does the sidecar receive any indication that its message was rejected?
5. Is this behavior logged? At what level?
6. Could a legitimate (non-malicious) scenario cause this mismatch? If so, what?

### Exercise 4: JSON vs Protobuf Trade-offs

The signaling protocol uses JSON over WebSocket. Argue both sides of switching to Protocol Buffers:

**For Protobuf:**
- What specific benefits would Protobuf provide for the signaling protocol?
- How would the `SignalingMessage` struct change?
- What would the `.proto` file look like?

**Against Protobuf:**
- What is lost by switching to binary encoding?
- How does Protobuf handle the dynamically-typed `payload` field?
- What deployment and build complexity does Protobuf add?

**What changes in the codebase:**
- Which files would need to be modified?
- Would the registry relay logic need to change? (Hint: consider whether the registry needs to parse the message to relay it.)
- Could you do a hybrid approach (binary envelope, text payload) and would it be worth it?

---

**Next lesson:** [Lesson 06: Inter-Process Communication](06-inter-process-communication.md) -- how the sidecar bridges WebRTC to the Python agent over Unix sockets.
