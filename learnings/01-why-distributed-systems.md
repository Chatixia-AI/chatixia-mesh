# Lesson 01 -- Why Distributed Systems? From Monolith to Mesh

## Prerequisites

None. This is the first lesson in the series.

## What You'll Learn

- What a distributed system is and how it differs from a monolithic application
- The Eight Fallacies of Distributed Computing and why they matter
- Four fundamental network topologies and their trade-offs
- The difference between control plane and data plane
- How the chatixia-mesh project maps to these concepts

---

## 1. What Is a Distributed System?

A **distributed system** is a collection of independent processes, running on different machines, that coordinate over a network to accomplish a shared goal. To an outside observer, the system behaves as a single coherent unit -- but internally, each process has its own memory, its own clock, and its own failure modes.

Contrast this with a **monolith**: a single process running on a single machine. All components share the same memory space, the same clock, and the same fate. If the process dies, everything dies. If the machine runs out of RAM, everything stops.

```
Monolith                          Distributed System
+---------------------------+     +--------+   +--------+   +--------+
|  UI + Logic + Data        |     | Node A |<->| Node B |<->| Node C |
|  (one process, one box)   |     +--------+   +--------+   +--------+
+---------------------------+           |                        |
                                        +------------------------+
```

### Why distribute at all?

A monolith is simpler to build, deploy, and debug. You distribute a system when you need something a single machine cannot provide:

- **Resilience** -- if one node fails, the others keep running.
- **Scalability** -- you can add machines instead of buying a bigger one.
- **Geographic reach** -- you place nodes close to users or data sources.
- **Autonomy** -- independent teams or organizations each run their own node.

Every one of these benefits comes with a cost: the network between nodes is unreliable, slow, and insecure. The rest of this lesson explores those costs.

---

## 2. The Eight Fallacies of Distributed Computing

In 1994, Peter Deutsch (with additions by James Gosling) identified eight assumptions that developers new to distributed systems tend to make. Each one is false. Each one will bite you.

### Fallacy 1: The network is reliable

**The assumption:** Messages sent over the network always arrive.

**Reality:** Packets get dropped. Cables get unplugged. Routers reboot. Cloud availability zones go offline. A message sent from Agent A to Agent B may never arrive, arrive twice, or arrive out of order.

**Real example:** An agent sends a `task_request` over a WebRTC DataChannel. The receiving sidecar's machine loses Wi-Fi for two seconds. The message is gone. Without a timeout and retry mechanism, the requesting agent waits forever.

### Fallacy 2: Latency is zero

**The assumption:** Sending a message takes no time.

**Reality:** Light takes 67 milliseconds to travel from New York to London through fiber. Add routing, buffering, encryption handshakes, and kernel scheduling, and a "fast" cross-continent round trip takes 100-200ms. A slow one takes seconds.

**Real example:** An agent running on a Raspberry Pi at home sends a task to an agent in a cloud data center. The developer expects sub-millisecond response times because that is what local function calls take. The actual round-trip latency is 80ms on a good day, 2 seconds when the home router is congested.

### Fallacy 3: Bandwidth is infinite

**The assumption:** You can send as much data as you want.

**Reality:** Network links have finite capacity. A home internet connection might offer 10 Mbps upload. A WebRTC DataChannel over a TURN relay might be limited to 1-2 Mbps. If 20 agents all broadcast large payloads simultaneously, the network saturates.

**Real example:** An agent tries to broadcast a 50 MB dataset to all peers. On a mesh of 10 agents, that is 450 MB of outbound traffic (9 peers times 50 MB). The home connection chokes. Messages queue up. Heartbeats stop arriving. The registry marks agents as offline.

### Fallacy 4: The network is secure

**The assumption:** No one can intercept or tamper with messages.

**Reality:** Without encryption, any device on the network path can read and modify traffic. Even with encryption, endpoints can be compromised. Authentication is a separate problem from encryption.

**Real example:** Agent A connects to the registry over plain HTTP on a coffee shop Wi-Fi network. An attacker on the same network intercepts the API key exchange and obtains a valid JWT. They impersonate Agent A and inject malicious task responses.

This is why chatixia-mesh uses DTLS encryption on DataChannels and JWT authentication on the signaling layer.

### Fallacy 5: Topology doesn't change

**The assumption:** The set of nodes and the connections between them are fixed.

**Reality:** Nodes join and leave. Network routes change. A laptop moves from office Wi-Fi to a mobile hotspot. An agent behind a NAT gets a new IP address after a DHCP lease renewal.

**Real example:** A mesh of five agents is running smoothly. A developer closes their laptop lid. The sidecar on that machine loses all its DataChannel connections. The other four agents must detect the disconnection and update their peer lists. The topology just changed.

### Fallacy 6: There is one administrator

**The assumption:** A single person or team controls the entire network.

**Reality:** In a distributed system, different nodes may be operated by different people, teams, or organizations. They have different upgrade schedules, different security policies, and different priorities.

**Real example:** chatixia-mesh agents can run on a developer's laptop, a Raspberry Pi at home, and a VM in the cloud. Each environment has different firewall rules, different OS versions, and different update cycles. No single administrator controls all of them.

### Fallacy 7: Transport cost is zero

**The assumption:** Sending data over the network is free.

**Reality:** Network communication costs CPU cycles for serialization and encryption, memory for buffers, and sometimes actual money for bandwidth. A TURN relay server costs money to host. Cloud egress charges add up.

**Real example:** Running a coturn TURN relay to connect agents behind NATs costs server fees and bandwidth charges. Each relayed DataChannel message passes through the TURN server, doubling the bandwidth consumption. At scale, this becomes a real line item in the infrastructure budget.

### Fallacy 8: The network is homogeneous

**The assumption:** All nodes use the same hardware, OS, and software versions.

**Reality:** One agent runs Python 3.13 on macOS with a Rust sidecar compiled for ARM. Another runs the same Python code on Ubuntu x86_64. A third runs in a Docker container on a Raspberry Pi with a 32-bit ARM sidecar. Each environment has different performance characteristics, different available system calls, and different bugs.

**Real example:** The chatixia sidecar uses Unix domain sockets for IPC with the Python agent. This works on Linux and macOS. On Windows, Unix sockets behave differently (or require named pipes instead). The "network" between sidecar and agent is not homogeneous.

---

## 3. Topology Models

The **topology** of a distributed system describes how nodes are connected. Different topologies make different trade-offs between simplicity, resilience, and scalability.

### Star (Client-Server)

Every node connects to a single central server. Nodes do not talk to each other directly.

```
        +--------+
        | Server |
        +---+----+
       / |  |  \  \
      /  |  |   \  \
    A    B  C    D   E
```

**Connections:** N (one per node).

**Advantages:**
- Simple to implement and reason about.
- Central point of control for authentication, routing, and monitoring.
- Adding a new node requires only one new connection.

**Disadvantages:**
- Single point of failure: if the server goes down, the entire system is offline.
- Bottleneck: all traffic flows through the server, which must scale to handle it.
- Latency: every message between two nodes takes two hops (sender to server, server to receiver).

**Real-world example:** A traditional web application. All clients talk to the backend server. Clients never talk to each other.

### Ring

Each node connects to exactly two neighbors, forming a closed loop. Messages travel around the ring.

```
    A --- B
    |     |
    E     C
     \   /
      \ /
       D
```

**Connections:** N (one per node, two endpoints each).

**Advantages:**
- No single bottleneck node.
- Simple routing: send the message clockwise until it reaches the target.

**Disadvantages:**
- Fragile: a single node failure breaks the ring and partitions the network.
- High latency: a message may need to traverse N/2 hops on average.
- Rarely used in practice for general-purpose distributed systems.

**Real-world example:** Token Ring networks (historical). Some DHT (distributed hash table) protocols like Chord use ring-like structures.

### Tree (Hierarchical)

Nodes are arranged in a parent-child hierarchy. Messages flow up and down the tree.

```
            Root
           / | \
          A  B  C
         /|    /|\
        D E   F G H
```

**Connections:** N-1 (each node except the root has one parent).

**Advantages:**
- Natural for hierarchical organizations (regions, clusters, teams).
- Efficient broadcasting: the root sends to its children, who forward to theirs.

**Disadvantages:**
- Root is a single point of failure.
- Communication between leaves in different subtrees must go up through the root.
- Deeper trees mean more hops and higher latency.

**Real-world example:** DNS (Domain Name System). Queries flow from local resolver up through a hierarchy of nameservers.

### Full Mesh

Every node connects directly to every other node. No intermediary needed.

```
    A ---- B
    |\    /|
    | \  / |
    |  \/  |
    |  /\  |
    | /  \ |
    |/    \|
    C ---- D
```

**Connections:** N * (N-1) / 2.

This formula comes from combinatorics: each of N nodes connects to N-1 others, but each connection is shared by two nodes, so divide by 2.

| Agents | Connections |
| ------ | ----------- |
| 3      | 3           |
| 5      | 10          |
| 10     | 45          |
| 20     | 190         |
| 50     | 1,225       |
| 100    | 4,950       |

**Advantages:**
- Maximum resilience: no single point of failure. Any node can fail and the rest remain fully connected.
- Minimum latency: every message is a single hop.
- No routing required: the sender connects directly to the receiver.

**Disadvantages:**
- O(N^2) connections. Each new node must establish connections to every existing node.
- O(N^2) memory and CPU overhead for maintaining connections.
- Connection setup cost: if each connection takes 5 seconds (like a WebRTC handshake), adding the 20th node requires 19 new handshakes.
- Impractical beyond ~50 nodes without optimization.

**Real-world example:** chatixia-mesh uses full mesh for its agent-to-agent DataChannels.

### Topology Trade-offs Summary

```
                Simplicity    Resilience    Scalability
Star            High          Low           Moderate
Ring            Moderate      Low           Low
Tree            Moderate      Low           Moderate
Full Mesh       Low           High          Low
```

No topology is universally best. The right choice depends on the number of nodes, the failure requirements, and the communication patterns of the system.

---

## 4. Control Plane vs Data Plane

Every distributed system has two fundamental concerns:

1. **Where should messages go?** (discovery, routing, coordination)
2. **How do messages actually get there?** (transport, delivery)

These concerns are separated into two planes.

### Control Plane

The control plane manages the system. It answers questions like:

- Which nodes are alive?
- What capabilities does each node have?
- How do I reach a specific node?
- Who is allowed to join the network?

The control plane carries metadata, not application data. It is the "brain" of the system.

**Analogy:** Air traffic control. Controllers do not fly the planes. They track positions, assign runways, issue clearances, and resolve conflicts. They coordinate. The actual flight path -- the movement of the aircraft through the sky -- is not their job.

### Data Plane

The data plane carries the actual application data between nodes. It is the "muscle" of the system.

**Analogy:** The flight path itself. The aircraft moving from point A to point B, carrying passengers and cargo. Air traffic control told the pilot which route to take, but the plane flies the route on its own.

### Why Separate Them?

Separating control and data planes provides three key benefits:

1. **Different scaling requirements.** The control plane handles metadata (small, infrequent). The data plane handles application traffic (large, frequent). They need different infrastructure.

2. **Fault isolation.** If the control plane goes down, existing data plane connections keep working. Nodes that are already connected can still communicate. They just cannot discover new nodes or update routing.

3. **Security boundaries.** The control plane can enforce authentication and authorization without seeing the actual data. End-to-end encryption on the data plane means even the control plane operator cannot read messages.

### In chatixia-mesh

chatixia-mesh cleanly separates control plane and data plane:

```
CONTROL PLANE (Registry, HTTP/WebSocket)
+--------------------------------------------------+
|  Agent registration    POST /api/registry/agents  |
|  Health checking       POST /api/hub/heartbeat    |
|  Skill routing         GET  /api/registry/route   |
|  Task queue            POST /api/hub/tasks        |
|  Signaling             WebSocket SDP/ICE relay    |
|  Agent pairing         POST /api/pairing/pair     |
+--------------------------------------------------+

DATA PLANE (WebRTC DataChannels, P2P)
+--------------------------------------------------+
|  task_request / task_response                     |
|  agent_prompt / agent_response                    |
|  skill_query  / skill_response                    |
|  ping / pong                                      |
|  Direct sidecar-to-sidecar, DTLS encrypted        |
+--------------------------------------------------+
```

The registry never sees the content of agent-to-agent messages. It only knows that agents exist, what skills they have, and how to help them find each other. Once two sidecars have established a DataChannel through the signaling process, all application data flows directly between them.

This separation means that if the registry goes down:
- Existing DataChannel connections continue working.
- Agents can still send tasks to each other over the mesh.
- New agents cannot join the network until the registry comes back.
- Health tracking stops, but communication does not.

---

## 5. Case Study: chatixia-mesh at a Glance

chatixia-mesh is an agent-to-agent mesh network where AI agents communicate directly over WebRTC DataChannels. It has four components.

### The Four Components

| Component | Language | Role |
| --------- | -------- | ---- |
| **Registry** | Rust (axum) | Control plane -- signaling, agent registry, task queue, hub API |
| **Sidecar** | Rust (webrtc-rs) | Networking layer -- WebRTC peer connections, IPC bridge to agent |
| **Agent** | Python | Application logic -- skills, LLM calls, task execution |
| **Hub** | React (Vite) | Monitoring dashboard -- agent health, task queue, topology graph |

Each agent process is paired with a sidecar process. The agent handles application logic (what to do); the sidecar handles networking (how to reach other agents). They communicate over a Unix domain socket using a JSON-line protocol. This is the **sidecar pattern** -- isolating networking complexity into a separate process so that the agent code stays simple.

### Architecture Diagram

```
                    +---------------------------------------------+
                    |           Registry (Rust/axum)               |
                    |  Signaling | Registry | Task Queue | Hub API |
                    +----------+------------------+---------------+
                               |                  |
                      WebSocket|                  |HTTP
                      SDP/ICE  |                  |
             +-----------------+------------------+----------+
             |                 |                             |
      +------+------+  +------+------+  ...          +------+------+
      | Sidecar  A  |  | Sidecar  B  |               | Sidecar  N  |
      |   (Rust)    |  |   (Rust)    |               |   (Rust)    |
      +------+------+  +------+------+               +------+------+
             |                |                             |
      IPC    |         IPC    |                      IPC    |
      (Unix  |         (Unix  |                      (Unix  |
      socket)|         socket)|                      socket)|
             |                |                             |
      +------+------+  +------+------+               +------+------+
      |  Agent  A   |  |  Agent  B   |               |  Agent  N   |
      |  (Python)   |  |  (Python)   |               |  (Python)   |
      +-------------+  +-------------+               +-------------+

      Sidecar A <-------- WebRTC DataChannel --------> Sidecar B
                   (direct P2P, DTLS encrypted)
```

### How a Message Travels

When Agent A wants to send a task to Agent B:

1. Agent A writes a JSON command (`send`) to the Unix socket shared with Sidecar A.
2. Sidecar A looks up the DataChannel connected to Sidecar B.
3. Sidecar A sends the `MeshMessage` over the DataChannel (P2P, encrypted).
4. Sidecar B receives the message and writes it to its Unix socket.
5. Agent B reads the message from the socket and executes the task.

The registry is not involved in steps 2 through 4. The data flows directly between the two sidecars.

### What Problem Does It Solve?

AI agents are most useful when they can collaborate. One agent might be good at summarizing text, another at querying databases, another at generating code. For them to work together, they need a way to discover each other and exchange messages.

chatixia-mesh solves this with:
- **Discovery** -- the registry lets agents find each other by skill.
- **Direct communication** -- WebRTC DataChannels let agents talk without routing through a central server.
- **NAT traversal** -- STUN/TURN lets agents behind firewalls and home routers connect to each other.
- **Graceful degradation** -- if P2P fails, agents fall back to the registry's HTTP task queue. The system slows down but never stops.

---

## Exercises

### Exercise 1: Identify Control Plane and Data Plane

Pick a system you use daily (Slack, email, a video game, a streaming service). Draw its topology. Then answer:

- Which components form the control plane?
- Which components form the data plane?
- What happens to the data plane if the control plane goes down?

### Exercise 2: Calculate Mesh Connections

The full mesh formula is: connections = N * (N-1) / 2.

Calculate the number of DataChannel connections needed for a chatixia-mesh deployment with:

- 5 agents
- 10 agents
- 20 agents

If each WebRTC connection takes 5 seconds to establish (ICE negotiation, DTLS handshake), how long does it take for the 20th agent to finish connecting to all 19 existing agents? What does this suggest about the practical upper limit of full mesh?

### Exercise 3: Star vs Mesh at Scale

Write a paragraph (4-6 sentences) arguing that a star topology is a better choice than full mesh for a deployment of 100 agents. Consider the number of connections, the connection setup time, the memory overhead per node, and the operational complexity. You may reference the connection count table from section 3.

### Exercise 4: Fallacies and Heartbeats

chatixia-mesh uses a heartbeat system: each agent sends a small message to the registry every 15 seconds. If the registry has not heard from an agent in 90 seconds, it marks the agent as "stale." After 270 seconds of silence, the agent is marked "offline."

Which of the Eight Fallacies does the heartbeat system address? Explain why the system needs three states (active, stale, offline) instead of just two (active, offline). What could go wrong if the threshold were set to only 20 seconds?

---

## Related Lessons

- Lesson 02: WebRTC Fundamentals -- SDP, ICE, DTLS, DataChannels
- Lesson 03: The Sidecar Pattern -- isolating networking from application logic
- Lesson 04: Signaling and Discovery -- how agents find each other

---

## Further Reading

- Deutsch, P. (1994). *The Eight Fallacies of Distributed Computing.* Sun Microsystems.
- Tanenbaum, A. & Van Steen, M. (2017). *Distributed Systems: Principles and Paradigms.* 3rd ed. Pearson.
- Kleppmann, M. (2017). *Designing Data-Intensive Applications.* O'Reilly Media. Chapters 5-9.
- chatixia-mesh documentation: `docs/SYSTEM_DESIGN.md` for the full architecture specification.
- chatixia-mesh documentation: `docs/WEBRTC_VS_ALTERNATIVES.md` for the transport choice rationale.
