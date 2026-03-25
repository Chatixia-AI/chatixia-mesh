# Glossary

Definitions for all domain-specific terms used across the chatixia-mesh curriculum. Consult this glossary when you encounter unfamiliar terminology in any lesson.

Terms from the project glossary (`docs/GLOSSARY.md`) are included alongside additional terms introduced in the learning materials. Each entry notes where in the curriculum the concept first appears.

---

## A

**ADR (Architecture Decision Record)**
A short document that captures a single architectural decision -- the context, the decision itself, and its consequences. ADRs create a decision log so future contributors understand why the system is built the way it is, not just how.
*First introduced: Lesson 16*

**Agent**
A Python AI process with skills, LLM integration, and mesh networking. Each agent has one sidecar. Agents register with the registry, advertise their capabilities, and execute tasks either from the hub queue or via direct P2P messages.
*First introduced: Lesson 01*

**API Key**
Pre-shared credential (e.g., `ak_dev_001`) exchanged for a JWT to authenticate with the registry. API keys are stored in `api_keys.json` and map to a `peer_id` and `role`.
*First introduced: Lesson 08*

## C

**C10K Problem**
The challenge of handling 10,000 simultaneous network connections on a single server, first articulated by Dan Kegel in 1999. It drove the shift from thread-per-connection to event-driven architectures. Modern async runtimes like tokio and asyncio descend directly from solutions to the C10K problem.
*First introduced: Lesson 04*

**CGNAT**
Carrier-Grade NAT -- ISP-level NAT that places entire neighborhoods behind a single public IP. More restrictive than consumer NAT; peers behind CGNAT often cannot establish direct connections and require TURN relay.
*First introduced: Lesson 02*

**CI/CD**
Continuous Integration / Continuous Delivery -- the practice of automatically building, testing, and publishing software on every change. chatixia-mesh uses GitHub Actions for Rust/Python/Hub CI, Docker image builds on pull requests, and OIDC-based PyPI publishing on release.
*First introduced: Lesson 15*

**Cone NAT**
A NAT type where, once a device sends a packet to any external address, the NAT creates a mapping that allows replies from any external address (full cone), any port on the same address (restricted cone), or only the exact address and port (port-restricted cone). Cone NATs are more permissive than symmetric NATs and generally allow STUN-based P2P connections.
*First introduced: Lesson 02*

**Control Plane**
The part of the system that handles discovery, routing, authentication, and coordination. In chatixia-mesh: the registry (HTTP + WebSocket signaling). The control plane knows who is connected but does not carry application data.
*First introduced: Lesson 01*

**Cooperative Multitasking**
A concurrency model where tasks voluntarily yield control to the scheduler (via `await` points). If a task never yields, it blocks all other tasks on the same thread. Both tokio (Rust) and asyncio (Python) use cooperative multitasking. Contrast with preemptive multitasking.
*First introduced: Lesson 04*

## D

**DashMap**
A concurrent hash map for Rust that uses sharded locking -- the map is divided into multiple shards, each with its own lock, so concurrent reads and writes to different shards do not block each other. Used throughout the registry for `SignalingState`, `RegistryState`, `HubState`, and `PairingState`.
*First introduced: Lesson 12*

**DataChannel**
A WebRTC primitive for arbitrary data transfer. DTLS-encrypted, peer-to-peer. Carries `MeshMessage` JSON in chatixia-mesh. DataChannels run over SCTP over DTLS over UDP.
*First introduced: Lesson 03*

**Data Plane**
The part of the system that carries application data between agents. In chatixia-mesh: WebRTC DataChannels (P2P). Separated from the control plane by design so the registry never sees agent-to-agent message content.
*First introduced: Lesson 01*

**Defense in Depth**
A security strategy that applies multiple layers of protection so that no single failure compromises the system. In chatixia-mesh: API key authentication, short-lived JWTs, DTLS encryption on DataChannels, sender verification, and graceful degradation through the task queue.
*First introduced: Lesson 14*

**Design Tokens**
Named constants that encode a design system's visual decisions -- colors, spacing, typography, radii, shadows. In chatixia-mesh, the hub dashboard centralizes all tokens in `hub/src/theme.ts` so that UI components reference semantic names rather than raw values.
*First introduced: Lesson 13*

**DTLS**
Datagram Transport Layer Security -- encryption layer for WebRTC DataChannels. Provides TLS-equivalent security over UDP datagrams. Automatic in WebRTC; no application-level configuration needed.
*First introduced: Lesson 03*

## E

**Event Loop**
The core of an async runtime. A single-threaded loop that polls for I/O readiness, timers, and task wakeups, then runs ready tasks until they yield. tokio's event loop is multi-threaded (work-stealing); Python's asyncio event loop is single-threaded.
*First introduced: Lesson 04*

**Eventually Consistent**
A consistency model where, after an update, all replicas will converge to the same state given enough time, but reads may return stale data in the interim. The chatixia-mesh registry is eventually consistent -- agent health is derived from heartbeat recency, and if the registry restarts, agents re-register on their next heartbeat cycle.
*First introduced: Lesson 12*

## F

**Full Mesh**
Network topology where every node connects to every other node. N nodes = N(N-1)/2 connections. Provides direct communication but scales quadratically. chatixia-mesh uses full mesh for small-to-medium agent counts.
*First introduced: Lesson 02*

## G

**Glassmorphism**
A UI design trend characterized by translucent surfaces with backdrop blur, creating a frosted-glass effect. The chatixia-mesh hub dashboard uses an "Atmospheric Luminescence" variant -- light-mode glassmorphic with tonal layering, ambient shadows, and no hard borders.
*First introduced: Lesson 13*

**Graceful Degradation**
The system's three-tier fallback strategy: P2P DataChannel (fastest) -> TURN relay (slower, still encrypted) -> HTTP task queue via registry (slowest, always works). The system never stops working; it only slows down.
*First introduced: Lesson 01*

## H

**Health**
Agent status derived from heartbeat recency: `active` (<90s since last heartbeat), `stale` (90-270s), `offline` (>270s). Computed by the registry's `health_check_loop` background task.
*First introduced: Lesson 09*

**Health Check**
A mechanism for verifying that a service is running and responsive. In chatixia-mesh, the registry runs a `health_check_loop` every 15 seconds that classifies agents by heartbeat recency. Docker Compose uses health checks on the registry to gate dependent service startup.
*First introduced: Lesson 15*

**Heartbeat**
Periodic HTTP POST from agent to registry (`/api/hub/heartbeat`). Updates agent metadata (hostname, IP, skills, uptime) and picks up pending tasks from the hub queue. Default interval: 30 seconds.
*First introduced: Lesson 09*

**HOL Blocking**
Head-of-line blocking -- when a lost packet at the front of a queue delays all subsequent packets. Occurs in TCP, HTTP/2, and SCTP reliable-ordered mode. WebRTC DataChannels can avoid HOL blocking by using unordered delivery.
*First introduced: Lesson 03*

**Hub**
The monitoring/control plane: task queue + dashboard. The hub API lives in the registry (Rust); the hub UI is a React app served as static assets. The dashboard polls the registry API for agents, tasks, and topology.
*First introduced: Lesson 13*

## I

**ICE**
Interactive Connectivity Establishment -- discovers the best network path between peers. ICE gathers candidates (host, server-reflexive via STUN, relay via TURN), then tests connectivity between all candidate pairs to find the optimal connection.
*First introduced: Lesson 02*

**ICE Candidate**
A potential network address (host, server-reflexive, or relay) that a peer can be reached at. Host candidates are local IPs; server-reflexive candidates are public IPs discovered via STUN; relay candidates route through TURN.
*First introduced: Lesson 02*

**IPC**
Inter-Process Communication -- the JSON-line protocol over Unix domain socket between a sidecar and its Python agent. Each message is a single JSON object terminated by a newline.
*First introduced: Lesson 06*

**IpcMessage**
JSON-line message between sidecar and Python agent. Structure: `type` (string identifying the message kind) and `payload` (arbitrary JSON). Agent-to-sidecar types: `send`, `broadcast`, `connect`, `list_peers`. Sidecar-to-agent types: `message`, `peer_connected`, `peer_disconnected`, `peer_list`.
*First introduced: Lesson 06*

## J

**JSON-lines**
A text format where each line is a valid JSON object, separated by newline characters. Used for the IPC protocol between sidecar and agent because it provides natural message framing -- the newline character delimits message boundaries without needing length prefixes or special escape sequences.
*First introduced: Lesson 06*

**JWT**
JSON Web Token -- short-lived (5 min) bearer token used for WebSocket authentication and sender verification. Issued by the registry in exchange for an API key. Contains claims: `sub` (peer_id), `role`, `exp`, `iat`.
*First introduced: Lesson 08*

## M

**MCP**
Model Context Protocol -- standard for connecting LLMs to external tools and data sources. chatixia-mesh agents can expose their skills as MCP servers, making them accessible to any MCP-compatible LLM client.
*First introduced: Lesson 09*

**Mesh**
The network of WebRTC DataChannel connections between sidecars. Full mesh = every sidecar connected to every other sidecar. The mesh carries all agent-to-agent application data.
*First introduced: Lesson 01*

**MeshMessage**
Application-level JSON message exchanged over DataChannels. Structure: `type`, `request_id`, `source_agent`, `target_agent`, `payload`. Message types include `task_request`, `task_response`, `agent_prompt`, `agent_response`, `skill_query`, `ping`, `pong`, and more.
*First introduced: Lesson 07*

**Multi-stage Docker Build**
A Dockerfile technique where earlier stages compile or build artifacts, and a final stage copies only the necessary outputs into a minimal base image. chatixia-mesh uses multi-stage builds for registry (Node + Rust -> debian-slim) and sidecar (Rust -> debian-slim) to produce small production images.
*First introduced: Lesson 15*

## N

**NAT**
Network Address Translation -- maps private IPs to public IPs. Prevents direct inbound connections to devices behind routers, which is why WebRTC needs ICE/STUN/TURN for connectivity.
*First introduced: Lesson 02*

## O

**OIDC Trusted Publisher**
An OpenID Connect-based mechanism where a package registry (like PyPI) trusts a specific CI/CD workflow to publish packages without static API tokens. chatixia-mesh uses OIDC trusted publisher for GitHub Actions to publish the `chatixia` Python package to PyPI on release.
*First introduced: Lesson 15*

## P

**Peer**
A sidecar identified by its `peer_id`. Peers communicate via WebRTC DataChannels. Each peer maintains connections to all other peers in the mesh.
*First introduced: Lesson 02*

**Peer ID**
Unique identifier assigned to a sidecar, derived from its API key entry (e.g., `agent-001`). The peer ID is the `sub` claim in the sidecar's JWT.
*First introduced: Lesson 08*

**Preemptive Multitasking**
A concurrency model where the scheduler can interrupt a running task at any time to give CPU time to another task. Used by operating systems for process scheduling and by multi-threaded runtimes. Contrast with cooperative multitasking, where tasks must explicitly yield.
*First introduced: Lesson 04*

**Process Isolation**
Running components in separate OS processes so that a crash or resource exhaustion in one does not bring down the others. The sidecar pattern uses process isolation -- the Rust sidecar and Python agent run as independent processes connected by IPC, so a Python crash does not take down the WebRTC connections.
*First introduced: Lesson 10*

**Protocol Layering**
The practice of organizing network communication into stacked layers, each building on the one below. In chatixia-mesh: `MeshMessage` (application) over DataChannel (WebRTC) over SCTP over DTLS over UDP. Each layer has a specific responsibility and can be understood independently.
*First introduced: Lesson 07*

**Protocol Stack**
The concrete set of protocols used at each layer of a system's communication. chatixia-mesh's protocol stack for P2P data: UDP -> DTLS -> SCTP -> DataChannel -> MeshMessage JSON. For signaling: TCP -> HTTP/WebSocket -> SignalingMessage JSON.
*First introduced: Lesson 03*

## Q

**QUIC**
A UDP-based transport protocol developed by Google/IETF. Provides multiplexed streams without HOL blocking, built-in TLS 1.3, and faster connection setup than TCP+TLS. Used by HTTP/3 but lacks the P2P NAT traversal that WebRTC provides.
*First introduced: Lesson 11*

## R

**Reactor Pattern**
A design pattern for handling concurrent I/O by demultiplexing incoming events from multiple sources into a single event loop, then dispatching each event to the appropriate handler. The foundation of async runtimes like tokio (Rust) and asyncio (Python). The reactor waits for I/O readiness rather than blocking on individual operations.
*First introduced: Lesson 04*

**Registry**
Central Rust server (port 8080) that provides signaling relay, agent discovery, task queue, and hub API. The registry is the control plane -- it coordinates connections but does not carry agent-to-agent data.
*First introduced: Lesson 01*

**Request/Response Correlation**
The technique of including a unique `request_id` in messages so that responses can be matched to their originating requests. Used by `MeshClient` in the Python agent, which stores a per-request `asyncio.Future` in a pending map and resolves it when a response with the matching `request_id` arrives.
*First introduced: Lesson 07*

## S

**SCTP**
Stream Control Transmission Protocol -- transport layer used by WebRTC DataChannels. Runs over DTLS over UDP. Supports reliable/unreliable and ordered/unordered delivery modes, giving applications control over latency vs. reliability trade-offs.
*First introduced: Lesson 03*

**SDP**
Session Description Protocol -- describes media/data capabilities. Exchanged as offers and answers during WebRTC negotiation. An SDP offer says "here is what I can do"; an SDP answer says "here is what we have in common."
*First introduced: Lesson 03*

**Sharded Locking**
A concurrency technique that divides a shared data structure into multiple independent shards, each protected by its own lock. Concurrent operations on different shards proceed without contention. DashMap uses sharded locking to provide high-throughput concurrent access.
*First introduced: Lesson 12*

**Sidecar**
A Rust process that handles WebRTC signaling and DataChannels on behalf of a Python agent. Communicates with the agent via IPC over a Unix socket. Each agent has exactly one sidecar.
*First introduced: Lesson 10*

**Sidecar Pattern**
An architectural pattern where a helper process runs alongside a primary application, handling cross-cutting concerns like networking, security, or observability. In chatixia-mesh, the Rust sidecar handles all WebRTC complexity so the Python agent only needs to speak JSON over a Unix socket.
*First introduced: Lesson 10*

**Signaling**
The process of exchanging SDP offers/answers and ICE candidates between peers via the registry WebSocket to establish WebRTC connections. Signaling is a control-plane operation -- once the connection is established, signaling is no longer needed for data exchange.
*First introduced: Lesson 05*

**Skill**
A named capability (Python function) that an agent can execute. Skills are registered with the registry and used for task routing. Built-in skills include `delegate`, `mesh_send`, `mesh_broadcast`, `list_agents`, `find_agent`, and `user_intervention`.
*First introduced: Lesson 09*

**Skill Handler**
A Python function (sync or async) that implements a specific skill. Skill handlers are registered in the `SKILL_HANDLERS` dictionary and invoked when the agent receives a matching task -- either from the hub queue via heartbeat or from a direct P2P `task_request`.
*First introduced: Lesson 09*

**State Machine**
A model where a system exists in one of a finite set of states and transitions between them based on events. In chatixia-mesh, tasks follow a state machine: `pending` -> `assigned` -> `completed` or `failed`. Agent health follows another: `active` -> `stale` -> `offline`.
*First introduced: Lesson 12*

**STRIDE**
A threat modeling framework that categorizes threats into six types: Spoofing, Tampering, Repudiation, Information disclosure, Denial of service, and Elevation of privilege. Useful as a checklist when analyzing the security of each component and trust boundary.
*First introduced: Lesson 14*

**STUN**
Session Traversal Utilities for NAT -- server that helps peers discover their public IP address and port mapping. Used for NAT traversal. STUN alone works when both peers are behind cone NATs; symmetric NATs require TURN.
*First introduced: Lesson 02*

**Symmetric NAT**
A NAT type that creates a unique mapping for each (internal IP, internal port, external IP, external port) tuple. Since the external port changes depending on the destination, STUN-discovered addresses are useless for other peers. Symmetric NATs require TURN relay for WebRTC connectivity.
*First introduced: Lesson 02*

## T

**Task**
A unit of work submitted to the hub task queue. Has a lifecycle: `pending` -> `assigned` -> `completed` or `failed`. Tasks have a TTL, a target skill, and optional target/source agent IDs.
*First introduced: Lesson 09*

**Tool-use Loop**
The iterative cycle where an LLM generates a tool call, the agent executes it, returns the result to the LLM, and the LLM decides whether to make another tool call or produce a final response. chatixia-mesh agents implement this loop with skills as tools.
*First introduced: Lesson 09*

**Topology**
The mesh network graph -- which agents are online and which DataChannel connections exist between them. Exposed via the `/api/hub/network/topology` endpoint and visualized in the hub dashboard.
*First introduced: Lesson 13*

**TTL**
Time To Live -- maximum seconds a task can remain pending/assigned before the registry's `expire_tasks_loop` marks it as failed. Default: 300 seconds.
*First introduced: Lesson 09*

**TURN**
Traversal Using Relays around NAT -- relay server used when direct and STUN connections fail (e.g., symmetric NATs, CGNAT). TURN relays all traffic through a server, adding latency but guaranteeing connectivity. chatixia-mesh uses coturn with ephemeral credentials.
*First introduced: Lesson 02*

## U

**UDP Hole-Punching**
A NAT traversal technique where two peers behind NAT simultaneously send UDP packets to each other's STUN-discovered addresses, creating NAT mapping entries that allow the return packets through. Works with cone NATs but fails with symmetric NATs, which is why TURN exists as a fallback.
*First introduced: Lesson 02*

## W

**WebTransport**
An API for bidirectional client-server communication over QUIC. Considered a potential successor to WebRTC DataChannels for client-server use cases, but lacks P2P support and NAT traversal capabilities.
*First introduced: Lesson 11*
