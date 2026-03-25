# Peer-to-Peer Networking: When Servers Get Out of the Way

## Prerequisites

- [Lesson 01: Networking Foundations](./01-networking-foundations.md) -- TCP/IP, DNS, ports, sockets, HTTP basics.

## What You'll Learn

- Why most internet traffic uses a client-server model and when P2P is a better fit.
- What NATs are, why they exist, and how they block inbound connections.
- How STUN, TURN, and UDP hole-punching overcome NAT barriers.
- How ICE orchestrates candidate gathering, connectivity checks, and path selection.
- Why P2P systems still need a signaling server to bootstrap connections.

---

## 1. Client-Server vs Peer-to-Peer

The vast majority of internet traffic follows the **client-server** model: your browser (client) sends a request to a server, and the server sends back a response. This pattern dominates because it is simple, well-understood, and maps naturally to how services are built -- one party publishes a stable address, the other connects to it.

```text
Client-Server Model

  ┌────────┐         ┌────────────┐
  │ Client │────────>│   Server   │
  │        │<────────│ 203.0.113.5│
  └────────┘         └────────────┘
   (dynamic IP,         (static IP,
    behind NAT)          port 443)
```

The server has a stable, publicly routable IP address. Clients can be anywhere -- behind home routers, on cellular networks, on corporate VPNs. The server does not need to know the client's address in advance; it only needs to respond to whoever connects.

### When P2P makes sense

Peer-to-peer (P2P) removes the central server from the data path. Peers talk directly to each other:

```text
Peer-to-Peer Model

  ┌────────┐                     ┌────────┐
  │ Peer A │<═══════════════════>│ Peer B │
  └────────┘   direct connection └────────┘
```

P2P is worth the added complexity when one or more of these conditions hold:

1. **Latency matters.** Routing through a server adds a round trip. In video calls (WebRTC), gaming, and real-time collaboration, that extra hop is noticeable.
2. **Bandwidth is expensive at the center.** BitTorrent moves terabytes of data without any single server paying the bandwidth bill. Each peer uploads to other peers.
3. **Privacy or trust.** If data passes through a server, the server operator can inspect it. P2P with end-to-end encryption (like DTLS in WebRTC) means only the communicating peers see the plaintext.
4. **Resilience.** If there is no central server, there is no single point of failure. Blockchain networks and mesh networks rely on this property.

| Use Case | Protocol | Why P2P |
|----------|----------|---------|
| File sharing | BitTorrent | Bandwidth distributed across peers |
| Video/voice calls | WebRTC | Low latency, end-to-end encryption |
| Blockchain | Bitcoin/Ethereum | No central authority, censorship resistance |
| Agent mesh networks | chatixia-mesh | Direct agent-to-agent communication, registry not in data path |

The trade-off is complexity. Establishing a direct connection between two peers behind NATs is substantially harder than connecting to a server with a known address.

---

## 2. The NAT Problem

### Why NATs exist

When the internet was designed, every device was meant to have its own globally unique IP address. IPv4 provides roughly 4.3 billion addresses. That seemed like plenty in the 1980s. It was not.

By the late 1990s, addresses were running out. The long-term fix is IPv6 (which provides 2^128 addresses), but adoption has been gradual. The short-term fix, deployed everywhere, is **Network Address Translation (NAT)**.

A NAT device (usually your home router) assigns private IP addresses to devices on the local network (e.g., 192.168.1.x) and shares a single public IP address for all outbound traffic:

```text
Private Network (192.168.1.0/24)            Internet
                                          ┌──────────────┐
┌───────────┐                             │              │
│ Laptop    │ 192.168.1.10                │  Web Server  │
│           ├──┐                          │  93.184.216.34│
└───────────┘  │     ┌──────────────┐     │              │
               ├────>│  NAT Router  │────>│              │
┌───────────┐  │     │ Public IP:   │     └──────────────┘
│ Phone     │──┘     │ 198.51.100.7 │
│           │        └──────────────┘
│192.168.1.11│
└───────────┘

  Outgoing packet from Laptop:
    src: 192.168.1.10:54321 --> rewritten to --> src: 198.51.100.7:40000
  Reply from server:
    dst: 198.51.100.7:40000 --> rewritten to --> dst: 192.168.1.10:54321
```

The NAT router maintains a **mapping table** that tracks which internal device sent which outgoing packet, so it can route replies back to the correct device.

### Why NAT breaks inbound connections

NAT works fine for client-server traffic: the client initiates the connection, the NAT creates a mapping, and replies flow back through that mapping. But what if another device on the internet wants to *initiate* a connection to your laptop? It cannot. There is no mapping in the NAT table for unsolicited inbound traffic, so the router drops the packet.

This is the fundamental problem for P2P: **both peers are typically behind NATs, and neither can accept incoming connections from the other.**

```text
The NAT Problem for P2P

  ┌────────┐       ┌─────────┐           ┌─────────┐       ┌────────┐
  │ Peer A │──────>│ NAT A   │     ?     │  NAT B  │<──────│ Peer B │
  │10.0.0.5│       │Public:  │ ========> │ Public: │       │10.0.0.9│
  └────────┘       │1.2.3.4  │  DROPPED  │5.6.7.8  │       └────────┘
                   └─────────┘           └─────────┘
                     No mapping exists      No mapping exists
                     for inbound from B     for inbound from A
```

### Types of NAT

NATs differ in how strictly they filter inbound packets. This matters because some types can be traversed with simple techniques, while others cannot.

#### Full Cone NAT (least restrictive)

Once an internal host sends a packet to any external address, the NAT creates a mapping. Any external host can send packets to that mapped port, even if the internal host never contacted them.

```text
Full Cone NAT

  Internal          NAT                    External
  10.0.0.5:5000 -> 1.2.3.4:40000

  Mapping: 10.0.0.5:5000 <--> 1.2.3.4:40000
  Rule: ANY external host can send to 1.2.3.4:40000
        and it will be forwarded to 10.0.0.5:5000

  ┌──────────┐  ┌──────────┐  ┌──────────┐
  │ Internal │──│ NAT      │──│ Anyone   │  <-- any host can reach
  │ 10.0.0.5 │  │1.2.3.4   │  │ on the   │      the mapped port
  │ :5000    │  │ :40000   │  │ internet │
  └──────────┘  └──────────┘  └──────────┘
```

#### Restricted Cone NAT

The mapping only accepts inbound packets from an IP address the internal host has previously sent to. The port does not matter.

```text
Restricted Cone NAT

  10.0.0.5:5000 sent to 9.9.9.9:80
  Mapping: 10.0.0.5:5000 <--> 1.2.3.4:40000

  Rule: Only 9.9.9.9 (any port) can send to 1.2.3.4:40000
        Other IPs are dropped.
```

#### Port-Restricted Cone NAT

Like restricted cone, but also checks the port. Only the exact IP:port pair the internal host contacted can send back.

```text
Port-Restricted Cone NAT

  10.0.0.5:5000 sent to 9.9.9.9:80
  Mapping: 10.0.0.5:5000 <--> 1.2.3.4:40000

  Rule: Only 9.9.9.9:80 can send to 1.2.3.4:40000
        9.9.9.9:443 is dropped.
        8.8.8.8:80 is dropped.
```

#### Symmetric NAT (most restrictive)

A new mapping is created for every unique destination. If 10.0.0.5:5000 sends to 9.9.9.9:80, the external port might be 40000. If it sends to 8.8.8.8:443, the external port might be 40001. Each mapping only accepts replies from the specific destination it was created for.

```text
Symmetric NAT

  10.0.0.5:5000 -> 9.9.9.9:80   ==>  1.2.3.4:40000  (mapping 1)
  10.0.0.5:5000 -> 8.8.8.8:443  ==>  1.2.3.4:40001  (mapping 2)

  Rule: 9.9.9.9:80  can only reach via :40000
        8.8.8.8:443 can only reach via :40001
        No one else can reach either port

  This makes hole-punching very difficult because the port
  a STUN server sees is different from the port a peer would see.
```

Symmetric NAT is common in enterprise networks and some cellular carriers. It is the hardest type to traverse.

#### Carrier-Grade NAT (CGNAT)

Some ISPs place customers behind an additional layer of NAT at the carrier level. Your home router does NAT, and then the ISP's equipment does NAT again. This is called CGNAT (RFC 6598, using the 100.64.0.0/10 address range).

```text
Carrier-Grade NAT (double NAT)

  ┌────────┐    ┌──────────┐    ┌──────────────┐    ┌──────────┐
  │ Device │───>│ Home NAT │───>│  ISP CGNAT   │───>│ Internet │
  │10.0.0.5│    │192.168.1.1│   │100.64.0.0/10 │    │          │
  └────────┘    └──────────┘    └──────────────┘    └──────────┘
   private       private         carrier-grade        public
   address       address         private address      address

  Two layers of address translation.
  STUN sees the CGNAT's public IP, not the home router's.
  Hole-punching is unreliable. TURN is often required.
```

CGNAT is increasingly common as IPv4 addresses become scarcer. It compounds the NAT traversal problem because there are two levels of translation to punch through.

---

## 3. NAT Traversal Techniques

### UDP Hole-Punching

UDP hole-punching exploits the fact that most NATs create bidirectional mappings for outbound UDP packets. If both peers send UDP packets to each other's public address at roughly the same time, both NATs create mappings, and subsequent packets flow through.

The trick: both peers need to know each other's public IP and port. They learn this via a coordination server (often a STUN server or a signaling server).

```text
UDP Hole-Punching (step by step)

  Step 1: Both peers send to a known server to discover their public address.

  Peer A (10.0.0.5)          NAT A            Server (S)         NAT B         Peer B (10.0.0.9)
       │                      │                  │                  │                │
       │──── UDP to S ───────>│── 1.2.3.4:40000 >│                  │                │
       │                      │                  │<  5.6.7.8:50000 ─│<─── UDP to S ──│
       │                      │                  │                  │                │
       │<── "you are          │                  │                  │                │
       │    1.2.3.4:40000" ───│                  │── "you are       │                │
       │                      │                  │   5.6.7.8:50000">│──────────────> │
       │                      │                  │                  │                │

  Step 2: Server tells each peer the other's public address.
          (Or peers exchange this info through signaling.)

  Step 3: Both peers send UDP packets to each other simultaneously.

  Peer A                     NAT A                             NAT B              Peer B
       │                      │                                  │                │
       │── UDP to 5.6.7.8:50000 >│                               │                │
       │                      │── pkt ──────────────────────────>│ (creates mapping│
       │                      │   NAT B sees src 1.2.3.4:40000  │  for 1.2.3.4)  │
       │                      │                                  │───────────────> │
       │                      │                                  │                │
       │                      │<──────────────────────── pkt ────│                │
       │                      │  NAT A sees src 5.6.7.8:50000   │<── UDP to      │
       │ <────────────────────│  (mapping exists, allow)         │  1.2.3.4:40000 │
       │                      │                                  │                │

  Both NATs now have mappings. Bidirectional UDP flows.
```

Hole-punching works well with full cone, restricted cone, and port-restricted cone NATs. It generally fails with symmetric NAT because the port assigned for the STUN server query is different from the port that would be assigned for the peer.

### STUN: Session Traversal Utilities for NAT

STUN (RFC 8489) is a lightweight protocol that answers one question: **"What is my public IP address and port?"**

A STUN server sits on the public internet. A peer behind a NAT sends a STUN Binding Request over UDP. The STUN server reads the source IP and port from the UDP packet header (after NAT translation) and returns them in a Binding Response. The peer now knows its **server-reflexive address** -- the public IP:port that external hosts see.

```text
STUN: Discovering Your Public Address

  ┌──────────┐     ┌──────────┐     ┌──────────────────┐
  │  Peer    │────>│   NAT    │────>│   STUN Server    │
  │10.0.0.5  │     │          │     │ stun.example.com │
  │:5000     │     │1.2.3.4   │     │ :3478            │
  └──────────┘     │:40000    │     └──────────────────┘
                   └──────────┘
  Peer sends:   STUN Binding Request from 10.0.0.5:5000
  NAT rewrites: src becomes 1.2.3.4:40000
  STUN replies: "Your public address is 1.2.3.4:40000"

  The peer now has a "server-reflexive candidate":
    candidate: 1.2.3.4:40000 (srflx)
```

STUN servers are cheap to operate because they handle only small request/response pairs -- no media relay. Google operates free STUN servers (`stun:stun.l.google.com:19302`) used by many WebRTC applications.

STUN alone is not enough when:
- The peer is behind a symmetric NAT (the port seen by STUN differs from the port a peer would see).
- UDP is entirely blocked by a firewall.

### TURN: Traversal Using Relays around NAT

TURN (RFC 8656) is the fallback when direct connectivity is impossible. A TURN server allocates a public relay address and forwards all traffic between peers through itself.

```text
TURN: Relay When Direct Connection Fails

  ┌──────────┐     ┌──────────┐     ┌──────────────────┐     ┌──────────┐     ┌──────────┐
  │  Peer A  │────>│  NAT A   │────>│   TURN Server    │<────│  NAT B   │<────│  Peer B  │
  │          │     │          │     │ (relay address:   │     │          │     │          │
  │          │     │          │     │  203.0.113.5:9000)│     │          │     │          │
  └──────────┘     └──────────┘     └──────────────────┘     └──────────┘     └──────────┘

  1. Peer A sends to TURN server (permitted by NAT A -- it is outbound).
  2. TURN server relays to Peer B's public address (permitted by NAT B -- it is a reply).
  3. Traffic flows bidirectionally through the relay.

  The TURN server sees all traffic but cannot decrypt it (DTLS encryption).
```

TURN is expensive because the server relays all data (bandwidth cost). It partially negates the P2P advantage -- there is now a server in the data path. However, it preserves end-to-end encryption (DTLS), and it ensures connectivity when nothing else works.

TURN servers typically require authentication. The most common approach for WebRTC is **ephemeral credentials** using a shared secret and HMAC-SHA1 (coturn's `use-auth-secret` mode). The signaling server generates time-limited credentials that the TURN server validates without a database lookup.

```text
Ephemeral TURN Credential Flow

  1. Signaling server generates:
     username = "{unix_timestamp + ttl}:mesh"
     credential = base64(HMAC-SHA1(shared_secret, username))

  2. Client receives credentials in ICE config.

  3. TURN server validates:
     - Parse expiry timestamp from username
     - Recompute HMAC-SHA1 with the same shared secret
     - Compare with provided credential
     - Check timestamp has not expired

  No database needed. Server and TURN share only a secret.
```

### In chatixia-mesh: NAT traversal configuration

The registry serves ICE server configuration via `GET /api/config`. This endpoint always includes a STUN server and optionally includes TURN:

```text
GET /api/config response:

{
  "iceServers": [
    { "urls": ["stun:stun.l.google.com:19302"] },
    {
      "urls": ["turn:your-host:3478"],
      "username": "1711234567:mesh",
      "credential": "base64-hmac-sha1-hash"
    }
  ]
}
```

The sidecar (`sidecar/src/webrtc_peer.rs`) reads `TURN_URL` and `TURN_SECRET` from environment variables. If `TURN_SECRET` is set, it generates ephemeral credentials using HMAC-SHA1 -- the same algorithm coturn uses in `use-auth-secret` mode. The coturn configuration lives in `infra/coturn/turnserver.conf`.

The registry's `auth.rs` module implements the same credential generation for the `GET /api/config` endpoint, so both the sidecar (for its own connections) and external clients (via the API) receive valid TURN credentials.

---

## 4. ICE: Interactive Connectivity Establishment

ICE (RFC 8445) is the framework that ties STUN, TURN, and direct connectivity together. It is not a single technique but an orchestration protocol that **gathers all possible ways to reach a peer, tests them, and selects the best one.**

### Candidate gathering

When a peer wants to establish a connection, ICE first gathers **candidates** -- potential addresses the remote peer could use to reach it. There are three types:

| Type | Name | Source | Example |
|------|------|--------|---------|
| **host** | Host candidate | Local network interface | `192.168.1.10:5000` |
| **srflx** | Server-reflexive | STUN server response | `1.2.3.4:40000` |
| **relay** | Relay candidate | TURN server allocation | `203.0.113.5:9000` |

```text
ICE Candidate Gathering

  ┌──────────────────────────────────────────────────────────┐
  │  Peer A gathers candidates:                              │
  │                                                          │
  │  1. Host candidate (local interface)                     │
  │     192.168.1.10:5000         (priority: highest)        │
  │                                                          │
  │  2. Server-reflexive (from STUN)                         │
  │     1.2.3.4:40000             (priority: medium)         │
  │                                                          │
  │  3. Relay (from TURN allocation)                         │
  │     203.0.113.5:9000          (priority: lowest)         │
  │                                                          │
  └──────────────────────────────────────────────────────────┘
```

Candidates are prioritized: host candidates first (direct LAN is fastest), then server-reflexive (public internet, no relay), then relay (functional but adds latency and server load).

### Connectivity checks

Once both peers have gathered their candidates and exchanged them (via the signaling channel), ICE forms **candidate pairs** -- every combination of a local candidate and a remote candidate. It then performs connectivity checks on each pair by sending STUN Binding Requests directly between peers.

```text
ICE Connectivity Checks

  Peer A candidates:              Peer B candidates:
    host:  192.168.1.10:5000        host:  192.168.1.20:6000
    srflx: 1.2.3.4:40000           srflx: 5.6.7.8:50000
    relay: 203.0.113.5:9000         relay: 203.0.113.5:9001

  Candidate pairs (tested in priority order):

    Pair 1: A.host    <--> B.host     (both on same LAN?)
    Pair 2: A.host    <--> B.srflx    (A local, B through NAT?)
    Pair 3: A.srflx   <--> B.host     (B local, A through NAT?)
    Pair 4: A.srflx   <--> B.srflx   (both through NATs?)
    Pair 5: A.host    <--> B.relay    (A direct, B relayed?)
    Pair 6: A.relay   <--> B.host     (B direct, A relayed?)
    Pair 7: A.srflx   <--> B.relay    (A NAT, B relayed?)
    Pair 8: A.relay   <--> B.srflx   (B NAT, A relayed?)
    Pair 9: A.relay   <--> B.relay    (both relayed, last resort)

  ICE tests pairs from highest to lowest priority.
  First pair that succeeds wins.
```

### Path selection

ICE uses a priority formula defined in RFC 8445 to rank candidate pairs. The formula weighs the candidate type (host > srflx > relay), the network interface (e.g., Ethernet over Wi-Fi), and a component ID. The first pair to complete a connectivity check becomes the **selected pair**, and all media/data flows through it.

If the selected path degrades (e.g., the network changes), ICE can perform new checks and switch to a better pair -- this is called **ICE restart**.

```text
ICE State Machine (simplified)

  ┌──────────────┐
  │   new        │  (no candidates yet)
  └──────┬───────┘
         │ gather candidates
  ┌──────▼───────┐
  │  gathering   │  (querying STUN, allocating TURN)
  └──────┬───────┘
         │ candidates gathered
  ┌──────▼───────┐
  │  checking    │  (testing candidate pairs)
  └──────┬───┬───┘
         │   │ all pairs fail
         │   │
         │   └──────────────┐
         │ valid pair found │
  ┌──────▼───────┐   ┌──────▼───────┐
  │  connected   │   │   failed     │  (no connectivity -- fall back to HTTP)
  │  (data flows │   └──────────────┘
  │   on selected│
  │   pair)      │
  └──────────────┘
```

### In chatixia-mesh: ICE in the sidecar

The sidecar's `webrtc_peer.rs` creates an `RTCPeerConnection` with the ICE servers from the environment. As ICE gathers candidates, each one is sent to the remote peer through the signaling server (registry). The `setup_ice_forwarding` function wires up the `on_ice_candidate` callback to serialize candidates as `ice_candidate` signaling messages:

```text
Sidecar ICE Flow

  Sidecar A                    Registry (signaling)              Sidecar B
     │                              │                               │
     │── register ─────────────────>│                               │
     │                              │<──────────────── register ────│
     │<── peer_list [B] ───────────│── peer_list [A] ─────────────>│
     │                              │                               │
     │  (A creates offer)           │                               │
     │── SDP offer (target: B) ───>│── SDP offer ────────────────>│
     │                              │                               │  (B creates answer)
     │<──────────────── SDP answer ─│<── SDP answer (target: A) ───│
     │                              │                               │
     │── ICE candidate ───────────>│── ICE candidate ────────────>│
     │<──────────────── ICE cand. ──│<── ICE candidate ────────────│
     │── ICE candidate ───────────>│── ICE candidate ────────────>│
     │     ...                      │     ...                       │
     │                              │                               │
     │<══════════ DataChannel (direct P2P, DTLS) ═════════════════>│
     │              (registry exits the data path)                  │
```

---

## 5. Signaling: The Bootstrap Problem

Here is a paradox: to establish a P2P connection, two peers need to exchange information (SDP offers/answers, ICE candidates). But they cannot exchange information until they have a connection. This is the **bootstrap problem**.

### The role of a signaling server

A **signaling server** is a rendezvous point where peers discover each other and exchange the metadata needed to establish a direct connection. It is not part of the data path -- once the P2P connection is up, the signaling server could shut down and existing connections would continue.

The signaling server handles:

1. **Discovery** -- which peers are online and available.
2. **SDP exchange** -- the offer/answer model that describes each peer's capabilities and network information.
3. **ICE candidate exchange** -- forwarding candidate addresses between peers during connectivity checks.

```text
Signaling Server as Matchmaker

  ┌──────────┐                                          ┌──────────┐
  │  Peer A  │                                          │  Peer B  │
  └────┬─────┘                                          └────┬─────┘
       │                                                     │
       │  1. "I want to connect to B"                        │
       │     (SDP offer: my codecs, my ICE credentials)      │
       │─────────────>┌──────────────┐                       │
       │              │  Signaling   │──────────────────────> │
       │              │   Server     │  2. "A wants to       │
       │              │              │     connect to you"    │
       │              │              │     (forwards offer)   │
       │              │              │                        │
       │              │              │ <─────────────────────│
       │<─────────────│              │  3. "Here is my       │
       │              └──────────────┘     answer"            │
       │                                   (SDP answer)       │
       │                                                     │
       │  4. Exchange ICE candidates through signaling       │
       │─────────────── candidates ────────────────────────> │
       │<────────────── candidates ─────────────────────────│
       │                                                     │
       │  5. Direct P2P connection established               │
       │<═══════════════════════════════════════════════════>│
       │     (signaling server no longer needed)              │
```

### The SDP offer/answer model

SDP (Session Description Protocol, RFC 8866) is a text format that describes a multimedia session. In WebRTC, SDP offers and answers contain:

- Supported codecs and media types.
- ICE credentials (username fragment and password for STUN checks).
- DTLS fingerprint (for verifying the peer's certificate).
- Candidate information (sometimes included directly, sometimes trickled separately).

The flow is always: one peer creates an **offer**, the other creates an **answer**. This asymmetry avoids the problem of two peers simultaneously trying to propose incompatible configurations.

### Signaling is not standardized

Unlike STUN, TURN, and ICE (which have RFCs), the signaling mechanism is intentionally left unspecified by WebRTC. Any transport works: WebSocket, HTTP long-polling, carrier pigeon. What matters is that the offer, answer, and ICE candidates reach the other peer.

### In chatixia-mesh: the registry as signaling server

The chatixia-mesh registry (`registry/src/signaling.rs`) serves as the signaling server. Sidecars connect via WebSocket at `/ws?token=<jwt>`. The registry:

1. Authenticates the sidecar via JWT (obtained by exchanging an API key at `POST /api/token`).
2. Tracks connected peers in a `SignalingState` map.
3. Sends a `peer_list` message to each new peer, listing all other connected peers.
4. Relays `sdp_offer`, `sdp_answer`, and `ice_candidate` messages between peers (using `target_id` to route).
5. Verifies that the `peer_id` in each message matches the JWT's `sub` claim (prevents impersonation).

The registry also provides the ICE server configuration (`GET /api/config`), combining its roles as signaling server and STUN/TURN configuration provider. The three connectivity tiers degrade gracefully:

| Tier | Path | Latency | When used |
|------|------|---------|-----------|
| 1 | Direct P2P DataChannel | <100ms | Both peers have open UDP path (same LAN, or cooperating NATs) |
| 2 | TURN relay | 50-200ms | NAT/firewall blocks direct UDP, TURN server available |
| 3 | HTTP task queue (via registry) | 3-15s | All UDP blocked, no TURN configured |

Skill handlers (`delegate`, `mesh_send`, `mesh_broadcast`) attempt the P2P path first. If the target peer is not connected via DataChannel, they fall back to the registry's HTTP task queue. The system never fails -- it only slows down.

---

## Exercises

### Exercise 1: Determine your NAT type

Use a STUN test tool or website to determine what type of NAT you are behind. Some options:

- Run a WebRTC test page (search for "WebRTC NAT type test" in your browser) and inspect the ICE candidates that are gathered.
- Use a command-line STUN client like `stun` or `stuntman` to query `stun.l.google.com:19302`.
- Check the output: if you see only `host` candidates, you may be on a public IP. If you see `srflx` candidates, you are behind a NAT. Note the type if the tool reports it (full cone, restricted, symmetric).

Questions to answer:
- What is your private IP address?
- What public IP and port does the STUN server report?
- If you run the STUN test twice, does the port change? (If yes, you may be behind a symmetric NAT.)

### Exercise 2: ICE on the same LAN

Trace the ICE sequence for two chatixia-mesh agents running on the same local network (e.g., both on 192.168.1.x).

- What candidate types will each sidecar gather?
- When ICE runs connectivity checks, which candidate pair will succeed first?
- What is the expected latency for this path?
- Does STUN or TURN play any role in this scenario?

### Exercise 3: ICE across networks

Trace the ICE sequence for one agent on a home network behind a NAT and another on a VPS with a public IP (no NAT).

- What candidates does the home agent gather? What about the VPS agent?
- Which candidate pair will ICE select as the winner?
- Will the connection use the STUN-discovered (server-reflexive) address, or will the VPS's host candidate be used directly?
- Under what circumstances would this setup fall to TURN (Tier 2)?

### Exercise 4: Why TURN is necessary (and costly)

Explain in your own words:
- Give two real-world network configurations where STUN and hole-punching fail and TURN is the only option.
- Why does TURN partially negate the advantage of P2P? Consider bandwidth, latency, cost, and privacy.
- In chatixia-mesh, the system falls back to Tier 3 (HTTP task queue) if TURN is unavailable. Compare the trade-offs of running a TURN server vs. accepting Tier 3 latency for different workload types (real-time collaboration vs. batch task delegation).

---

## Related Lessons

- [Lesson 01: Networking Foundations](./01-networking-foundations.md) -- prerequisite.
- Lesson 03: WebRTC Deep Dive -- DTLS, SCTP, DataChannels, the full WebRTC stack.
- Lesson 04: The Sidecar Pattern -- why chatixia-mesh separates WebRTC into a Rust sidecar.

## Further Reading

- RFC 8445 -- Interactive Connectivity Establishment (ICE). The complete specification for candidate gathering, pairing, and selection.
- RFC 8489 -- Session Traversal Utilities for NAT (STUN). Defines the Binding Request/Response protocol.
- RFC 8656 -- Traversal Using Relays around NAT (TURN). Defines relay allocation and data forwarding.
- RFC 8866 -- Session Description Protocol (SDP). The text format for describing multimedia sessions.
- RFC 6598 -- IANA-Reserved IPv4 Prefix for Shared Address Space (CGNAT). Defines the 100.64.0.0/10 range.
- [WebRTC for the Curious](https://webrtcforthecurious.com/) -- free online book covering WebRTC internals.
- [ICE, STUN, and TURN explanation (MDN)](https://developer.mozilla.org/en-US/docs/Web/API/WebRTC_API/Protocols) -- Mozilla's WebRTC protocol overview.
- chatixia-mesh source: `registry/src/auth.rs` (ICE config endpoint, TURN credential generation), `sidecar/src/webrtc_peer.rs` (ICE forwarding, peer connection setup), `sidecar/src/signaling.rs` (signaling WebSocket client).
