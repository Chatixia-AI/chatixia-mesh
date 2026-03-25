# Lesson 03 -- WebRTC Fundamentals: The Protocol Stack for Real-Time P2P

**Prerequisites:** Lesson 02 (Networking Foundations)

---

## 1. What is WebRTC?

WebRTC (Web Real-Time Communication) is an open standard for peer-to-peer communication. Most people associate it with video calls in a browser, but that is only one use case. WebRTC defines three APIs:

1. **getUserMedia** -- captures audio and video from local devices (microphones, cameras, screen share). Not relevant to chatixia-mesh.
2. **RTCPeerConnection** -- manages the full lifecycle of a peer-to-peer connection: ICE negotiation, DTLS encryption, and multiplexing of media and data streams.
3. **RTCDataChannel** -- sends arbitrary data (text, binary) between peers over an established RTCPeerConnection. This is the API chatixia-mesh uses.

The key insight: **WebRTC is not just for browsers.** Server-side implementations exist in several languages:

| Library     | Language | Notable users                        |
|-------------|----------|--------------------------------------|
| Pion        | Go       | LiveKit, Ion-SFU                     |
| webrtc-rs   | Rust     | chatixia-mesh sidecar                |
| aiortc      | Python   | Research, testing                    |
| libwebrtc   | C++      | Chromium, Electron (the reference)   |

chatixia-mesh uses **webrtc-rs** (the `webrtc = "0.17"` crate) inside its Rust sidecar. The Python agent never touches WebRTC directly -- it communicates with the sidecar over a Unix socket using a JSON-line IPC protocol. This separation is deliberate: WebRTC is complex, and isolating it in a compiled sidecar keeps the agent framework simple.

---

## 2. The Protocol Stack

WebRTC builds a surprisingly deep stack on top of UDP. Every layer solves a specific problem. Here is the full picture for DataChannels (the mode chatixia-mesh uses):

```
+---------------------------------------------+
|            Application (MeshMessage)         |   Your JSON messages
+---------------------------------------------+
|              DataChannel API                 |   Named channels, open/close events
+---------------------------------------------+
|                   SCTP                       |   Reliable/unreliable delivery,
|         (Stream Control Transmission)        |   message framing, flow control
+---------------------------------------------+
|                   DTLS                       |   Encryption + mutual authentication
|        (Datagram Transport Layer Security)   |   (self-signed certs, no CA)
+---------------------------------------------+
|                   ICE                        |   NAT traversal, candidate
|    (Interactive Connectivity Establishment)  |   gathering, connectivity checks
+---------------------------------------------+
|              STUN / TURN                     |   Discover public IP (STUN),
|                                              |   relay if direct fails (TURN)
+---------------------------------------------+
|                   UDP                        |   Unreliable datagrams
+---------------------------------------------+
|                   IP                         |   Routing
+---------------------------------------------+
```

### What each layer does

**UDP** -- The transport foundation. WebRTC chose UDP over TCP because real-time media cannot tolerate TCP's head-of-line blocking and retransmission delays. For DataChannels, SCTP (above) adds reliability back when needed.

**STUN (Session Traversal Utilities for NAT)** -- A lightweight protocol that lets a peer discover its public IP address and port by sending a request to a STUN server. The STUN server echoes back the source address it sees. This tells the peer: "the outside world sees you as 203.0.113.5:54321." chatixia-mesh uses Google's public STUN server (`stun:stun.l.google.com:19302`) by default.

**TURN (Traversal Using Relays around NAT)** -- A fallback relay for when direct connectivity fails (symmetric NATs, restrictive firewalls). Traffic flows through the TURN server instead of peer-to-peer. It works, but adds latency and server cost. chatixia-mesh supports an optional coturn TURN server configured via `TURN_URL` and `TURN_SECRET` environment variables.

**ICE (Interactive Connectivity Establishment)** -- The orchestration layer. ICE gathers candidates (local addresses, STUN-discovered addresses, TURN relay addresses), exchanges them with the remote peer via the signaling channel, and runs connectivity checks on every candidate pair. It picks the best working path. ICE is what makes WebRTC work across NATs, VPNs, and mixed networks.

**DTLS (Datagram Transport Layer Security)** -- Encryption for datagrams. Similar to TLS but designed for UDP (where packets can arrive out of order or be lost). DTLS negotiates encryption keys and authenticates each peer using self-signed certificates. No certificate authority is needed -- fingerprints are verified through the signaling channel. More on this in Section 4.

**SCTP (Stream Control Transmission Protocol)** -- Runs on top of DTLS. Provides reliable delivery, message framing, and flow control -- features that UDP lacks. SCTP supports multiple independent streams, and each stream can be configured for reliable-ordered, reliable-unordered, or unreliable delivery. More on this in Section 5.

**DataChannel API** -- The application-level interface. Each DataChannel has a label (a name) and configuration options (ordered, maxRetransmits, maxPacketLifeTime). Applications send and receive messages through DataChannels without worrying about the layers below.

---

## 3. SDP: Session Description Protocol

Before two peers can connect, they need to agree on capabilities: what codecs they support, how to reach each other, and what encryption to use. This negotiation happens through **SDP (Session Description Protocol)**, exchanged via the **offer/answer pattern**.

### The offer/answer exchange

```
  Peer A (offerer)                Signaling Server              Peer B (answerer)
       |                               |                              |
       |--- create offer SDP --------->|                              |
       |    (set local description)    |                              |
       |                               |--- forward offer SDP ------->|
       |                               |                              |
       |                               |    (set remote description)  |
       |                               |    (create answer SDP)       |
       |                               |    (set local description)   |
       |                               |                              |
       |                               |<-- forward answer SDP -------|
       |    (set remote description)   |                              |
       |                               |                              |
       |<========== ICE candidates exchanged (trickled) =============>|
       |                               |                              |
       |<=============== DTLS handshake (direct P2P) ================>|
       |                               |                              |
       |<=============== SCTP association established ===============>|
       |                               |                              |
       |<=============== DataChannel open ===========================>|
```

1. Peer A creates an **offer** -- an SDP blob describing what it supports.
2. Peer A sets this as its **local description** and sends it to Peer B through the signaling server.
3. Peer B receives the offer, sets it as its **remote description**, creates an **answer** (its own SDP), and sets the answer as its local description.
4. Peer B sends the answer back through the signaling server.
5. Peer A receives the answer and sets it as its remote description.
6. Both peers start exchanging ICE candidates (often "trickled" -- sent as they are discovered, not batched).

### Annotated SDP blob

Here is a simplified SDP offer with annotations. A real SDP is longer, but these are the fields that matter:

```
v=0                                          # SDP version (always 0)
o=- 4611731400430051336 2 IN IP4 127.0.0.1   # Origin: session ID, version
s=-                                          # Session name (unused)
t=0 0                                        # Timing: start/stop (0 = permanent)

# --- ICE credentials ---
a=ice-ufrag:EsAw                             # ICE username fragment
a=ice-pwd:bP+XJMM09aR8AiX1jdRzXV            # ICE password (shared secret for STUN)

# --- DTLS fingerprint ---
a=fingerprint:sha-256 49:66:12:17:0A:...    # Hash of the self-signed DTLS certificate
a=setup:actpass                              # DTLS role: willing to be client or server

# --- Media/application description ---
m=application 9 UDP/DTLS/SCTP webrtc-datachannel   # DataChannel over SCTP/DTLS/UDP
c=IN IP4 0.0.0.0                             # Connection address (placeholder)
a=mid:0                                      # Media ID
a=sctp-port:5000                             # SCTP port number

# --- ICE candidates (may be trickled separately) ---
a=candidate:1 1 udp 2113937151 192.168.1.10 54321 typ host
#              ^component  ^priority    ^address   ^port  ^type
# "host" = local network address

a=candidate:2 1 udp 1845501695 203.0.113.5 54321 typ srflx raddr 192.168.1.10 rport 54321
# "srflx" = server-reflexive (discovered via STUN, this is the public IP)

a=candidate:3 1 udp 8331263 turn.example.com 3478 typ relay raddr 203.0.113.5 rport 54321
# "relay" = TURN relay (fallback path)
```

Key fields to understand:

- **ice-ufrag / ice-pwd** -- Short-term credentials for STUN connectivity checks. Each peer generates its own pair.
- **fingerprint** -- The SHA-256 hash of the peer's self-signed DTLS certificate. This is how peers verify each other's identity without a CA. The signaling channel must be trusted to deliver this correctly.
- **setup** -- The DTLS role negotiation. `actpass` means "I can act as either client or server." The answerer typically picks `active` (client).
- **m=application** -- Declares a DataChannel session (as opposed to audio or video).
- **sctp-port** -- The SCTP port used inside the DTLS tunnel.
- **candidate lines** -- Each candidate is a potential network path. ICE tries them all and picks the best one.

### Candidate types

| Type   | Name              | What it is                                     | Priority |
|--------|-------------------|-------------------------------------------------|----------|
| host   | Host candidate    | Local IP address (e.g., 192.168.1.10)           | Highest  |
| srflx  | Server-reflexive  | Public IP discovered via STUN                   | Medium   |
| prflx  | Peer-reflexive    | Discovered during connectivity checks           | Medium   |
| relay  | Relay candidate   | TURN relay address                              | Lowest   |

ICE prefers host candidates (direct LAN), then server-reflexive (direct through NAT), and falls back to relay (through TURN server) only when nothing else works.

---

## 4. DTLS: Encryption without PKI

Traditional TLS relies on a **Public Key Infrastructure (PKI)**: a certificate authority (CA) signs a server's certificate, and the client trusts the CA. This works for the web where servers have domain names, but it is impractical for peer-to-peer connections between ephemeral agents that have no domain names and no pre-existing trust relationship.

WebRTC solves this with **DTLS using self-signed certificates** and **fingerprint verification via signaling**.

### How it works

```
  Peer A                            Peer B
    |                                  |
    | 1. Generate self-signed cert     | 1. Generate self-signed cert
    |    Compute SHA-256 fingerprint   |    Compute SHA-256 fingerprint
    |                                  |
    | 2. Include fingerprint in SDP    | 2. Include fingerprint in SDP
    |    offer, send via signaling     |    answer, send via signaling
    |                                  |
    |--- DTLS ClientHello ------------>|
    |<-- DTLS ServerHello + Cert ------|  3. Peer A checks: does the
    |--- DTLS Cert + Finished -------->|     certificate hash match the
    |<-- DTLS Finished ----------------|     fingerprint from SDP?
    |                                  |  4. Peer B does the same check.
    |                                  |
    |==== Encrypted SCTP traffic =====>|  5. All subsequent traffic is
    |<==== Encrypted SCTP traffic =====|     encrypted with negotiated keys.
```

1. Each peer generates a self-signed certificate and computes its SHA-256 fingerprint.
2. The fingerprint is embedded in the SDP offer/answer and sent through the signaling channel.
3. During the DTLS handshake, each peer presents its certificate.
4. Each peer verifies that the certificate's fingerprint matches what was received in the SDP.
5. If the fingerprints match, the connection is authenticated and encrypted.

### Contrast with TLS/mTLS

| Property                | TLS (web)                           | DTLS (WebRTC)                      |
|--------------------------|-------------------------------------|------------------------------------|
| Certificate authority    | Required (Let's Encrypt, etc.)     | Not needed                         |
| Certificate type         | CA-signed                           | Self-signed                        |
| Identity verification    | Domain name in certificate          | Fingerprint in SDP via signaling   |
| Transport                | TCP (reliable stream)               | UDP (unreliable datagrams)         |
| Packet reordering        | Handled by TCP                      | Handled by DTLS sequence numbers   |
| Mutual authentication    | Optional (mTLS)                     | Always mutual                      |
| Trust anchor             | Pre-installed CA root certificates  | Signaling channel integrity        |

The critical security assumption: **the signaling channel must be trustworthy.** If an attacker can modify SDP messages in transit (replacing fingerprints), they can perform a man-in-the-middle attack. In chatixia-mesh, signaling goes through the registry over WebSocket, authenticated with JWT tokens. This makes the signaling channel the root of trust for all peer-to-peer encryption.

---

## 5. SCTP and DataChannels

### What is SCTP?

SCTP (Stream Control Transmission Protocol) was originally designed for telecom signaling. In WebRTC, it runs on top of DTLS (which runs on top of UDP), providing features that UDP alone cannot:

- **Message framing** -- SCTP delivers complete messages, not byte streams. You send a 200-byte JSON blob and receive a 200-byte JSON blob. No need to delimit messages yourself (unlike TCP, where you must handle framing).
- **Reliable delivery** -- SCTP can retransmit lost packets, like TCP. But unlike TCP, this is configurable per stream.
- **Ordered delivery** -- SCTP can deliver messages in order within a stream. Also configurable.
- **Multiple streams** -- A single SCTP association supports up to 65,535 independent streams. Each DataChannel maps to one stream. Streams are independent, so a lost packet on stream 3 does not block delivery on stream 7.
- **Flow control** -- SCTP has its own congestion control, preventing a fast sender from overwhelming a slow receiver.

### DataChannel delivery modes

Each DataChannel can be configured with a different delivery mode:

| Mode                | Ordered | Reliable | Use case                                    |
|---------------------|---------|----------|---------------------------------------------|
| Reliable-ordered    | Yes     | Yes      | Chat messages, RPC, task delegation         |
| Reliable-unordered  | No      | Yes      | File transfer chunks (order irrelevant)     |
| Unreliable-ordered  | Yes     | No       | Latest-value sensors (skip stale readings)  |
| Unreliable-unordered| No      | No       | Game state updates, live telemetry          |

**Reliable-ordered** is the default and what chatixia-mesh uses. For small JSON payloads like `MeshMessage` (typically under 1 KB), the overhead is negligible and the guarantees are essential -- you do not want task requests to arrive out of order or get silently dropped.

### Head-of-line blocking

Reliable-ordered mode has a trade-off: **head-of-line (HOL) blocking**. If packet N is lost, packets N+1, N+2, ... are buffered until packet N is retransmitted and received, even if they are already available. This is the same problem that TCP has.

```
Sender:   [pkt 1] [pkt 2] [pkt 3] [pkt 4] [pkt 5]
               |       |     X        |       |
               v       v              v       v
Receiver: [pkt 1] [pkt 2]   lost   [pkt 4] [pkt 5]  <-- buffered, waiting
                                     ^^^^^^^^^^^^
                           These cannot be delivered to the application
                           until pkt 3 is retransmitted and received.
```

For chatixia-mesh this is acceptable because:

1. JSON payloads are small (well under the SCTP MTU), so each message fits in a single packet.
2. Message ordering matters for request/response correlation.
3. The mesh operates over reliable networks (LAN, VPN) where packet loss is rare.

If you were building a system with large payloads or lossy networks, you might use reliable-unordered (for bulk transfer) or unreliable-unordered (for real-time telemetry where only the latest value matters).

### DataChannel as the application API

From the application's perspective, a DataChannel is simple:

- It has a **label** (a string name). chatixia-mesh uses `"mesh"` as the label.
- You can **send** text or binary messages.
- You receive messages via an **on_message** callback.
- You get **on_open** and **on_close** lifecycle events.

The sidecar creates the DataChannel with:

```rust
let dc = pc.create_data_channel("mesh", None).await?;
```

`None` means default configuration: reliable, ordered. The `"mesh"` label identifies this channel. A single RTCPeerConnection could host multiple DataChannels (each with a different label), but chatixia-mesh uses one per peer connection.

---

## 6. The Connection Lifecycle

Here is the complete sequence from "no connection" to "DataChannel open," step by step. This maps directly to the code in the chatixia-mesh sidecar.

```
  Sidecar A (offerer)          Registry (signaling)          Sidecar B (answerer)
       |                              |                              |
  [1]  | create RTCPeerConnection     |                              |
       | configure ICE servers        |                              |
       |                              |                              |
  [2]  | create DataChannel("mesh")   |                              |
       |                              |                              |
  [3]  | create offer (SDP)           |                              |
       | set local description        |                              |
       |                              |                              |
  [4]  |-- offer SDP (WebSocket) ---->|                              |
       |                              |-- forward offer SDP -------->|
       |                              |                              |
  [5]  |                              |        create RTCPeerConnection
       |                              |        configure ICE servers  |
       |                              |        set remote description |
       |                              |        (the offer)            |
       |                              |                              |
  [6]  |                              |        create answer (SDP)    |
       |                              |        set local description  |
       |                              |                              |
  [7]  |                              |<-- answer SDP (WebSocket) ---|
       |<-- forward answer SDP -------|                              |
       | set remote description       |                              |
       |                              |                              |
  [8]  |<======= ICE candidates trickled in both directions =======>|
       |  (host, srflx, relay)        |  (host, srflx, relay)        |
       |                              |                              |
  [9]  |<========== ICE connectivity checks (STUN) ================>|
       |  try each candidate pair     |                              |
       |  select best working path    |                              |
       |                              |                              |
  [10] |<============= DTLS handshake (direct P2P) ================>|
       |  verify certificate           |  verify certificate          |
       |  fingerprints                 |  fingerprints                |
       |  negotiate encryption keys    |                              |
       |                              |                              |
  [11] |<============= SCTP association established ===============>|
       |                              |                              |
  [12] |<============= DataChannel "mesh" open ====================>|
       |                              |                              |
       | on_open callback fires       |  on_data_channel callback    |
       | notify Python agent:         |  fires, then on_open         |
       |   "peer_connected"           |  notify Python agent:        |
       |                              |    "peer_connected"           |
```

### Steps mapped to code

| Step | Code location (sidecar)            | What happens                                          |
|------|------------------------------------|-------------------------------------------------------|
| 1    | `webrtc_peer::create_peer_connection` | Build API, configure ICE servers from env           |
| 2    | `webrtc_peer::initiate_connection` | `pc.create_data_channel("mesh", None)`               |
| 3    | `webrtc_peer::initiate_connection` | `pc.create_offer()`, `pc.set_local_description()`    |
| 4    | `webrtc_peer::initiate_connection` | Send offer via `sig_tx` (signaling WebSocket)        |
| 5    | `webrtc_peer::handle_offer`       | Create new peer connection, `pc.set_remote_description()` |
| 6    | `webrtc_peer::handle_offer`       | `pc.create_answer()`, `pc.set_local_description()`   |
| 7    | `webrtc_peer::handle_offer`       | Send answer via `sig_tx`                              |
| 8    | `webrtc_peer::setup_ice_forwarding` | `on_ice_candidate` callback sends candidates via signaling |
| 9    | webrtc-rs internals               | ICE agent runs connectivity checks automatically     |
| 10   | webrtc-rs internals               | DTLS handshake, fingerprint verification             |
| 11   | webrtc-rs internals               | SCTP association over the DTLS tunnel                |
| 12   | `webrtc_peer::setup_datachannel_handler` | `on_open` fires, IPC `peer_connected` sent to Python agent |

### Why does this take 5-10 seconds?

Compare WebRTC connection setup to a plain TCP+TLS connection:

```
TCP + TLS 1.3 (50-100ms):

  Client                Server
    |--- SYN ----------->|        1 RTT: TCP handshake
    |<-- SYN-ACK --------|
    |--- ACK ----------->|
    |--- ClientHello --->|        1 RTT: TLS handshake (1-RTT with TLS 1.3)
    |<-- ServerHello ----|
    |<-- Finished -------|
    |--- Finished ------->|
    |--- Data ----------->|       Ready. Total: ~2 RTTs = 50-100ms on LAN


WebRTC DataChannel (5-10 seconds):

  Peer A                Registry              Peer B
    |-- offer (SDP) ------>|                     |
    |                      |-- offer (SDP) ----->|     1. SDP exchange via signaling
    |                      |<-- answer (SDP) ----|        (~100-500ms, depends on signaling)
    |<-- answer (SDP) -----|                     |
    |                                            |
    |--- ICE candidates (trickled) ------------->|     2. ICE candidate gathering
    |<-- ICE candidates (trickled) --------------|        (~1-5s: STUN queries, TURN allocation)
    |                                            |
    |<=== STUN connectivity checks (many) =====>|     3. ICE connectivity checks
    |    try pair 1... fail                      |        (~1-3s: try every candidate pair)
    |    try pair 2... fail                      |
    |    try pair 3... success!                  |
    |                                            |
    |<======= DTLS handshake ==================>|     4. DTLS handshake (~50-100ms)
    |<======= SCTP association ================>|     5. SCTP setup (~10ms)
    |<======= DataChannel open ================>|     6. Ready
```

The bottleneck is ICE. Gathering candidates requires querying STUN/TURN servers (network round trips to external servers). Then every candidate pair must be tested with STUN connectivity checks. With N candidates on each side, there are up to N*M pairs to test. On a simple LAN this might take 1-2 seconds; across the internet with TURN fallback, it can take 5-10 seconds or more.

### When is it worth paying?

The 5-10 second setup cost is a **one-time cost per peer pair.** Once the connection is established, messages flow with latency comparable to raw UDP (sub-millisecond on LAN, low single-digit milliseconds across the internet). The connection persists until one peer disconnects.

| Scenario                                   | TCP+TLS      | WebRTC DataChannel   | Winner   |
|--------------------------------------------|--------------|----------------------|----------|
| Single request-response                    | 50-100ms      | 5-10s                | TCP      |
| 1000 messages over 1 hour                  | 50-100ms + ongoing  | 5-10s + ongoing | WebRTC (amortized) |
| Peers behind different NATs               | Requires public server | Works P2P      | WebRTC   |
| Need a central server anyway              | Natural fit   | Overhead             | TCP      |
| Real-time bidirectional messaging          | Possible      | Designed for this    | WebRTC   |
| Browser-to-server                          | Native        | Overkill             | TCP      |

For chatixia-mesh, agents maintain long-lived connections and exchange many messages. The setup cost is paid once when a sidecar connects to the mesh, and then all subsequent task delegation, skill queries, and agent prompts flow over the established DataChannels with minimal latency. The P2P architecture also means agents can communicate directly without routing every message through a central server.

---

## In chatixia-mesh

The WebRTC stack is entirely contained in the **Rust sidecar** (`sidecar/`). Here is how the concepts from this lesson map to the codebase:

| Concept               | Implementation                                                          |
|------------------------|-------------------------------------------------------------------------|
| WebRTC library         | `webrtc = "0.17"` crate (webrtc-rs) in `sidecar/Cargo.toml`           |
| Peer connection        | `webrtc_peer.rs` -- `create_peer_connection()`, ICE server config      |
| SDP offer/answer       | `webrtc_peer.rs` -- `initiate_connection()` (offerer), `handle_offer()` (answerer) |
| ICE candidate exchange | `webrtc_peer.rs` -- `setup_ice_forwarding()` via signaling WebSocket   |
| Signaling relay        | `signaling.rs` -- WebSocket client handles offer/answer/ice messages   |
| DataChannel creation   | `webrtc_peer.rs` -- `pc.create_data_channel("mesh", None)` (reliable, ordered) |
| Message format         | `protocol.rs` -- `MeshMessage` struct (JSON over DataChannel)          |
| Peer tracking          | `mesh.rs` -- `MeshManager` tracks connections and channels per peer    |
| IPC to Python agent    | `ipc.rs` -- JSON lines over Unix socket, `peer_connected`/`peer_disconnected` events |
| STUN server            | Google's public STUN (`stun:stun.l.google.com:19302`), hardcoded      |
| TURN server            | Optional coturn, configured via `TURN_URL` and `TURN_SECRET` env vars  |

The Python agent receives `peer_connected` and `peer_disconnected` IPC events but never participates in ICE negotiation, DTLS handshakes, or SCTP setup. From the agent's perspective, the mesh is a simple message bus: send JSON, receive JSON.

---

## Exercises

### Exercise 1: Label the protocol stack

Fill in the blank for what each layer provides in the WebRTC DataChannel stack:

```
+---------------------------------------------+
|            Application                       |   ______________________________
+---------------------------------------------+
|              DataChannel API                 |   ______________________________
+---------------------------------------------+
|                   SCTP                       |   ______________________________
+---------------------------------------------+
|                   DTLS                       |   ______________________________
+---------------------------------------------+
|                   ICE                        |   ______________________________
+---------------------------------------------+
|              STUN / TURN                     |   ______________________________
+---------------------------------------------+
|                   UDP                        |   ______________________________
+---------------------------------------------+
```

For each layer, write one sentence describing its primary responsibility.

### Exercise 2: Read an SDP offer

Given this simplified SDP offer, answer the questions below:

```
v=0
o=- 8834123908523456 2 IN IP4 127.0.0.1
s=-
t=0 0
a=ice-ufrag:Kx4R
a=ice-pwd:mN8qX2pLv9aR7cJdY5wT3b
a=fingerprint:sha-256 A1:B2:C3:D4:E5:F6:07:18:29:3A:4B:5C:6D:7E:8F:90:A1:B2:C3:D4:E5:F6:07:18:29:3A:4B:5C:6D:7E:8F:90
a=setup:actpass
m=application 9 UDP/DTLS/SCTP webrtc-datachannel
c=IN IP4 0.0.0.0
a=mid:0
a=sctp-port:5000
a=candidate:1 1 udp 2113937151 10.0.0.42 60001 typ host
a=candidate:2 1 udp 1845501695 85.214.33.7 60001 typ srflx raddr 10.0.0.42 rport 60001
a=candidate:3 1 udp 8331263 turn.infra.example.com 3478 typ relay raddr 85.214.33.7 rport 60001
```

Questions:

1. List all ICE candidates. For each, state the type (host/srflx/relay), the IP address, and the port.
2. What is the DTLS fingerprint? What hash algorithm is used?
3. What is the SCTP port? What does the `m=application` line tell you about the session?
4. If Peer B's NAT blocks direct UDP, which candidate type will ICE fall back to?
5. Why is the `a=setup:actpass` line important for the DTLS handshake?

### Exercise 3: DataChannel delivery modes

Explain the difference between these three DataChannel delivery modes. For each, give one concrete use case where it would be the best choice:

1. **Reliable-ordered** -- Messages are guaranteed to arrive, and in the order they were sent.
2. **Reliable-unordered** -- Messages are guaranteed to arrive, but may arrive in any order.
3. **Unreliable-unordered** -- Messages may be lost, and may arrive in any order.

Then answer:

- chatixia-mesh uses reliable-ordered for its `"mesh"` DataChannel. Why is this the right choice for JSON `MeshMessage` payloads like `task_request` and `task_response`?
- What is head-of-line blocking, and under what network conditions would it become a problem for chatixia-mesh?
- If you were adding a live agent telemetry feature (CPU usage, memory, updated every 100ms), which mode would you choose and why?

### Exercise 4: Connection setup cost comparison

WebRTC DataChannel connection setup takes 5-10 seconds. TCP+TLS 1.3 takes 50-100ms. Answer the following:

1. Break down where the time goes in WebRTC connection setup. Which phase is the bottleneck and why?
2. Why does WebRTC not just use TCP+TLS like everything else on the web?
3. Consider a mesh of 5 agents that maintains persistent connections. How many WebRTC connections are needed for a full mesh? What is the total setup time if connections are established sequentially? What about in parallel?
4. chatixia-mesh agents maintain long-lived DataChannel connections and exchange hundreds of messages per session. Calculate the amortized per-message overhead of the 5-10 second setup cost over 500 messages. Compare this to the per-message overhead if the system used HTTP request/response instead (assume 50ms per HTTP round trip).
5. Name two scenarios where the WebRTC setup cost would NOT be worth paying, and TCP/HTTP would be a better choice.

---

**Next lesson:** Lesson 04 will cover signaling -- how chatixia-mesh's registry relays SDP offers, answers, and ICE candidates over WebSocket to bootstrap these WebRTC connections.
