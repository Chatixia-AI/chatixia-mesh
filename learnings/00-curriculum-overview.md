# Curriculum Overview

A structured learning curriculum that uses chatixia-mesh -- an agent-to-agent mesh network built on WebRTC DataChannels -- as a case study for distributed systems, peer-to-peer networking, and AI agent architecture.

---

## Introduction

### What this curriculum teaches

This curriculum covers four intersecting domains through the lens of a real, working system:

- **Distributed systems** -- how independent processes coordinate without shared memory
- **WebRTC and peer-to-peer networking** -- how browsers and applications establish direct connections through NAT, firewalls, and the open internet
- **Mesh networking** -- how a group of peers self-organize into a resilient communication fabric
- **AI agent architecture** -- how autonomous agents discover each other, delegate work, and collaborate

Each lesson grounds abstract concepts in concrete code. You will read Rust, Python, and TypeScript, trace message flows across process boundaries, and understand why the system was built the way it was -- including its trade-offs and limitations.

### Who this is for

- **Newcomers** to distributed systems who want to understand how real networked software works, beyond tutorials and toy examples
- **Mid-level engineers** looking to deepen their knowledge of WebRTC, peer-to-peer protocols, or multi-agent systems
- **Anyone evaluating chatixia-mesh** who wants to understand the architecture before contributing or deploying

You should be comfortable reading code in at least one language. Prior experience with networking, Rust, or Python is helpful but not required -- each lesson explains the concepts it depends on.

### What you will be able to do after completing this curriculum

- Explain how WebRTC establishes peer-to-peer connections through NAT and firewalls
- Design signaling and application-layer protocols for real-time distributed systems
- Evaluate transport trade-offs (WebRTC vs HTTP vs gRPC) for different deployment scenarios
- Understand the sidecar pattern and why it separates networking concerns from application logic
- Read and modify the chatixia-mesh codebase with confidence
- Deploy a multi-agent mesh network across different network environments
- Write Architecture Decision Records and threat models for your own systems

---

## The chatixia-mesh System

chatixia-mesh is a peer-to-peer network where AI agents communicate directly over WebRTC DataChannels. A central registry handles signaling and discovery but stays out of the data path. The system has four components:

| Component | Language | Role |
|-----------|----------|------|
| **Registry** | Rust (axum) | Signaling server, agent registry, task queue, hub API. The control plane. |
| **Sidecar** | Rust (webrtc-rs) | WebRTC mesh peer that runs alongside each Python agent. Handles all networking complexity. |
| **Agent** | Python | AI agent framework with CLI. Skills, LLM integration, mesh client via IPC. |
| **Hub** | React (Vite) | Monitoring dashboard. Visualizes agents, tasks, and mesh topology. |

The architecture follows a layered communication model:

```
Registry (control plane)
    |
    | WebSocket signaling (SDP/ICE exchange)
    |
Sidecar <-- WebRTC DataChannel (P2P, DTLS encrypted) --> Sidecar
    |                                                       |
    | IPC (Unix socket, JSON lines)                         | IPC
    |                                                       |
  Agent                                                   Agent
```

The registry never sees agent-to-agent message content. Once peers establish DataChannels, they communicate directly. If direct connectivity fails, the system degrades gracefully through TURN relay and finally HTTP task queue -- it never stops working, it only slows down.

Throughout this curriculum, every lesson ties back to a specific part of this system. You will trace SDP offers through WebSocket handlers, follow task requests across DataChannels, and read the actual Rust and Python code that implements each concept.

---

## Curriculum Map

### Tier 1 -- Foundations (Lessons 01-04)

No prerequisites. These lessons establish the vocabulary and mental models you need for everything that follows.

| # | Lesson | What you will learn | Key files |
|---|--------|--------------------|----|
| 01 | [Why Distributed Systems](01-why-distributed-systems.md) | What makes a system "distributed," failure modes, CAP theorem intuition, why chatixia-mesh exists | `docs/SYSTEM_DESIGN.md` |
| 02 | [Peer-to-Peer Networking](02-peer-to-peer-networking.md) | Client-server vs P2P, NAT traversal, STUN/TURN, ICE, why direct connections are hard | `sidecar/src/webrtc_peer.rs`, `infra/coturn/` |
| 03 | [WebRTC Fundamentals](03-webrtc-fundamentals.md) | SDP offer/answer, ICE candidates, DTLS, SCTP, DataChannels -- the full connection lifecycle | `sidecar/src/signaling.rs`, `sidecar/src/webrtc_peer.rs` |
| 04 | [Async Programming Patterns](04-async-programming-patterns.md) | Async/await in Rust (tokio) and Python (asyncio), channels, select loops, why the codebase is async | `sidecar/src/main.rs`, `agent/chatixia/core/mesh_client.py` |

### Tier 2 -- Core Mechanics (Lessons 05-09)

Requires Tier 1. These lessons explain how each layer of the system actually works.

| # | Lesson | What you will learn | Key files |
|---|--------|--------------------|----|
| 05 | [Signaling Protocol Design](05-signaling-protocol-design.md) | How the registry relays SDP/ICE messages, WebSocket message format, peer tracking, why signaling is separate from data | `registry/src/signaling.rs`, `sidecar/src/signaling.rs` |
| 06 | [Inter-Process Communication](06-inter-process-communication.md) | Unix sockets, JSON-line protocol, how the sidecar bridges WebRTC to Python, message framing | `sidecar/src/ipc.rs`, `agent/chatixia/core/mesh_client.py` |
| 07 | [Application Protocol Design](07-application-protocol-design.md) | MeshMessage format, request/response correlation, message types, designing protocols for extensibility | `sidecar/src/protocol.rs`, `agent/chatixia/core/mesh_client.py` |
| 08 | [Authentication and Security](08-authentication-and-security.md) | API key exchange, JWT lifecycle, DTLS encryption, sender verification, ephemeral TURN credentials | `registry/src/auth.rs`, `sidecar/src/main.rs` |
| 09 | [AI Agent Architecture](09-ai-agent-architecture.md) | Agent lifecycle, skill registration, task delegation, LLM integration, the agent-as-a-service model | `agent/chatixia/runner.py`, `agent/chatixia/core/mesh_skills.py` |

### Tier 3 -- System Design (Lessons 10-14)

Requires Tier 2. These lessons examine architectural decisions, patterns, and trade-offs.

| # | Lesson | What you will learn | Key files |
|---|--------|--------------------|----|
| 10 | [The Sidecar Pattern](10-sidecar-pattern.md) | Why networking lives in a separate process, language boundary crossing, failure isolation, deployment implications | `sidecar/`, `agent/chatixia/core/mesh_client.py` |
| 11 | [Transport Comparison](11-transport-comparison.md) | WebRTC vs HTTP vs gRPC -- latency, NAT traversal, encryption, ecosystem maturity, when each is the right choice | `docs/WEBRTC_VS_ALTERNATIVES.md` |
| 12 | [State Management Without a Database](12-state-management-without-a-database.md) | In-memory state with DashMap, eventual consistency via heartbeats, what happens on restart, when you need persistence | `registry/src/registry.rs`, `registry/src/hub.rs` |
| 13 | [Building Monitoring Dashboards](13-building-monitoring-dashboards.md) | Polling vs push, topology visualization, health indicators, designing for operational visibility | `hub/src/` |
| 14 | [Threat Modeling](14-threat-modeling.md) | Attack surfaces, trust boundaries, authentication gaps, mitigations, writing a threat model for your own system | `docs/THREAT_MODEL.md` |

### Tier 4 -- Operations (Lessons 15-17)

Requires Tier 3. These lessons cover deploying, documenting, and testing the system.

| # | Lesson | What you will learn | Key files |
|---|--------|--------------------|----|
| 15 | [Deployment Patterns](15-deployment-patterns.md) | Docker Compose, Cloudflare Tunnel, TURN relay setup, connectivity tiers, cross-network deployment | `docker-compose.yml`, `docs/DEPLOYMENT_GUIDE.md` |
| 16 | [Architecture Decision Records](16-architecture-decision-records.md) | Why ADRs matter, how to write them, reading chatixia-mesh's ADR log, making decisions explicit | `docs/ADR.md` |
| 17 | [Testing Distributed Systems](17-testing-distributed-systems.md) | Unit testing async code, integration testing across process boundaries, simulating network failures | `registry/`, `agent/` |

---

## Dependency Graph

The following diagram shows which lessons must be completed before others. Read from top to bottom. Lessons at the same level can be taken in any order.

```
                    Tier 1 -- Foundations
                    (no prerequisites)

          01 Why             02 Peer-to-Peer     04 Async
          Distributed        Networking           Programming
          Systems                |                Patterns
             |                   |                   |
             |                03 WebRTC              |
             |              Fundamentals             |
             |                   |                   |
             +-------+-----------+--------+----------+
                     |                    |
                    Tier 2 -- Core Mechanics
                    (requires Tier 1)

          05 Signaling   06 IPC   07 Application   08 Auth
          Protocol                Protocol         & Security
             |              |         |                |
             +---------+----+---------+-------+--------+
                       |                      |
                   09 AI Agent            (all of
                   Architecture            Tier 2)
                       |                      |
                       +----------+-----------+
                                  |
                    Tier 3 -- System Design
                    (requires Tier 2)

          10 Sidecar    11 Transport   12 State      13 Monitoring
          Pattern       Comparison     Management    Dashboards
             |              |              |              |
             +------+-------+------+-------+------+------+
                    |              |              |
                14 Threat                   (all of
                Modeling                    Tier 3)
                    |                          |
                    +-----------+--------------+
                                |
                    Tier 4 -- Operations
                    (requires Tier 3)

          15 Deployment     16 ADRs     17 Testing
          Patterns                      Distributed
                                        Systems
```

---

## Learning Paths

Not everyone needs every lesson. Here are focused reading orders for specific goals.

### "I want to understand WebRTC"

Follow the networking track from fundamentals through signaling to transport trade-offs.

**Path:** 01 -> 02 -> 03 -> 05

| Lesson | Why |
|--------|-----|
| 01 Why Distributed Systems | Context for why P2P matters |
| 02 Peer-to-Peer Networking | NAT traversal, STUN/TURN, ICE |
| 03 WebRTC Fundamentals | SDP, DTLS, DataChannels |
| 05 Signaling Protocol Design | How chatixia-mesh coordinates WebRTC setup |

**Time estimate:** 4-6 hours

### "I want to build AI agents"

Follow the agent track from distributed basics through agent architecture.

**Path:** 01 -> 04 -> 06 -> 07 -> 09

| Lesson | Why |
|--------|-----|
| 01 Why Distributed Systems | Why agents need to coordinate |
| 04 Async Programming Patterns | Agent code is async Python |
| 06 Inter-Process Communication | How agents talk to the mesh |
| 07 Application Protocol Design | The message format agents use |
| 09 AI Agent Architecture | Skills, delegation, LLM integration |

**Time estimate:** 5-7 hours

### "I want to design distributed systems"

The broadest path, covering architecture, state, protocols, and trade-offs.

**Path:** 01 -> 02 -> 04 -> 07 -> 10 -> 11 -> 12 -> 14

| Lesson | Why |
|--------|-----|
| 01 Why Distributed Systems | Foundational mental models |
| 02 Peer-to-Peer Networking | P2P topology and connectivity |
| 04 Async Programming Patterns | Concurrency in distributed code |
| 07 Application Protocol Design | Designing wire protocols |
| 10 The Sidecar Pattern | Process decomposition |
| 11 Transport Comparison | Choosing the right transport |
| 12 State Management Without a Database | In-memory state trade-offs |
| 14 Threat Modeling | Security analysis for distributed systems |

**Time estimate:** 8-12 hours

### "I just want to deploy and operate"

Skip the theory, go straight to running and managing the system.

**Path:** 01 -> 15 -> 16 -> 17

| Lesson | Why |
|--------|-----|
| 01 Why Distributed Systems | Minimum context for operations |
| 15 Deployment Patterns | Docker, tunnels, TURN, cross-network setup |
| 16 Architecture Decision Records | Understanding why things are the way they are |
| 17 Testing Distributed Systems | Verifying the system works |

**Time estimate:** 4-6 hours

---

## Time Estimates

Each lesson is designed to take **60-90 minutes** at a comfortable pace. This includes reading the lesson text, examining the referenced source files, and working through any exercises.

| Tier | Lessons | Estimated time |
|------|---------|---------------|
| Tier 1 -- Foundations | 4 lessons | 4-6 hours |
| Tier 2 -- Core Mechanics | 5 lessons | 5-7 hours |
| Tier 3 -- System Design | 5 lessons | 5-7 hours |
| Tier 4 -- Operations | 3 lessons | 3-5 hours |
| **Full curriculum** | **17 lessons** | **25-30 hours** |

The focused learning paths above take 4-12 hours depending on the track.

You do not need to complete lessons in a single sitting. Each lesson is self-contained once its prerequisites are met.

---

## Supplementary Materials

These resources support the lessons but can also be used independently.

| Resource | File | Description |
|----------|------|-------------|
| Glossary | [`glossary.md`](glossary.md) | Definitions for all domain-specific terms used across lessons. Consult when you encounter unfamiliar terminology. |
| Reading List | [`reading-list.md`](reading-list.md) | Curated external resources -- RFCs, papers, blog posts, and documentation -- organized by topic. For going deeper after a lesson. |
| Diagrams | [`diagrams/`](diagrams/) | Architecture diagrams, sequence diagrams, and protocol flows referenced by individual lessons. |

The project's own documentation is also a learning resource:

| Document | What it teaches |
|----------|----------------|
| `docs/SYSTEM_DESIGN.md` | Architecture overview, communication layers, authentication flow, scalability |
| `docs/COMPONENTS.md` | Complete codebase map -- every file, struct, route, and environment variable |
| `docs/ADR.md` | 18 architecture decisions with context, rationale, and consequences |
| `docs/THREAT_MODEL.md` | Security analysis -- attack surfaces, trust boundaries, mitigations |
| `docs/WEBRTC_VS_ALTERNATIVES.md` | Transport comparison with devil's advocate analysis |
| `docs/DEPLOYMENT_GUIDE.md` | Cross-network deployment with Cloudflare Tunnel and TURN relay |
