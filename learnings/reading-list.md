# Reading List

Curated resources for deepening your understanding of the topics covered in this curriculum. Organized by topic area, with references to the relevant lessons.

---

## Distributed Systems

**Books:**

- **"Designing Data-Intensive Applications"** by Martin Kleppmann -- The definitive guide to distributed systems fundamentals. Chapters on replication, partitioning, consistency, and consensus are directly relevant. *(Lessons 01, 12)*
- **"Understanding Distributed Systems"** by Roberto Vitillo -- A more accessible introduction covering networking, coordination, and resilience patterns. *(Lesson 01)*

**Online:**

- [The Fallacies of Distributed Computing](https://en.wikipedia.org/wiki/Fallacies_of_distributed_computing) -- Peter Deutsch's eight fallacies, referenced in Lesson 01. *(Lesson 01)*
- [CAP Theorem Explained](https://martin.kleppmann.com/2015/05/11/please-stop-calling-databases-cp-or-ap.html) -- Martin Kleppmann's critique of oversimplified CAP reasoning. *(Lesson 01)*

---

## WebRTC and P2P Networking

**Books:**

- **"Real-Time Communication with WebRTC"** by Salvatore Loreto and Simon P. Romano (O'Reilly) -- Covers the full WebRTC stack from signaling to DataChannels. *(Lessons 02, 03, 05)*
- **"High Performance Browser Networking"** by Ilya Grigorik -- Chapter 18 covers WebRTC in depth. Free online at [hpbn.co](https://hpbn.co/webrtc/). *(Lessons 02, 03)*

**RFCs:**

- [RFC 8445](https://datatracker.ietf.org/doc/html/rfc8445) -- ICE: Interactive Connectivity Establishment. The full specification for how peers discover connectivity paths. *(Lesson 02)*
- [RFC 5389](https://datatracker.ietf.org/doc/html/rfc5389) -- STUN: Session Traversal Utilities for NAT. How peers discover their public IP addresses. *(Lesson 02)*
- [RFC 5766](https://datatracker.ietf.org/doc/html/rfc5766) -- TURN: Traversal Using Relays around NAT. The relay fallback when direct connectivity fails. *(Lesson 02)*
- [RFC 8261](https://datatracker.ietf.org/doc/html/rfc8261) -- SCTP over DTLS over UDP. The transport layer beneath DataChannels. *(Lesson 03)*
- [RFC 4566](https://datatracker.ietf.org/doc/html/rfc4566) -- SDP: Session Description Protocol. The format for WebRTC offer/answer exchange. *(Lesson 03)*

**Projects:**

- [Pion](https://github.com/pion/webrtc) -- Pure Go WebRTC implementation (~14k stars). The reference implementation that webrtc-rs was ported from. Well-documented with many examples. *(Lessons 03, 10)*
- [webrtc-rs](https://github.com/webrtc-rs/webrtc) -- Rust WebRTC implementation used by chatixia-mesh sidecars. Port of Pion. *(Lessons 03, 10)*
- [coturn](https://github.com/coturn/coturn) -- Open-source TURN/STUN server. Used by chatixia-mesh for NAT traversal relay. *(Lessons 02, 15)*

**Online:**

- [webrtc.org](https://webrtc.org/) -- Official WebRTC project site with guides and API documentation. *(Lesson 03)*
- [MDN WebRTC API](https://developer.mozilla.org/en-US/docs/Web/API/WebRTC_API) -- Mozilla's reference documentation for the browser WebRTC API. *(Lesson 03)*
- [WebRTC for the Curious](https://webrtcforthecurious.com/) -- Free online book explaining WebRTC protocols from the ground up. Written by Pion contributors. *(Lessons 02, 03)*

---

## Async Programming

**Rust:**

- [Tokio Tutorial](https://tokio.rs/tokio/tutorial) -- Official tutorial for the Tokio async runtime used by the registry and sidecar. Covers spawning, channels, select, and I/O. *(Lesson 04)*
- [Async Rust Book](https://rust-lang.github.io/async-book/) -- The official Rust async/await reference. Covers futures, executors, and pinning. *(Lesson 04)*
- [DashMap documentation](https://docs.rs/dashmap/latest/dashmap/) -- API reference for the concurrent HashMap used throughout the registry. *(Lessons 04, 12)*

**Python:**

- [Python asyncio documentation](https://docs.python.org/3/library/asyncio.html) -- Official reference for Python's async I/O library used by the agent runner and mesh client. *(Lesson 04)*
- [Real Python: Async IO in Python](https://realpython.com/async-io-python/) -- Practical tutorial with examples of coroutines, tasks, and event loops. *(Lesson 04)*

---

## Protocol Design

- [JSON Lines](https://jsonlines.org/) -- Specification for the newline-delimited JSON format used by the sidecar IPC protocol. *(Lesson 06)*
- [RFC 7519](https://datatracker.ietf.org/doc/html/rfc7519) -- JWT: JSON Web Tokens. The token format used for WebSocket authentication. *(Lesson 08)*
- [Protocol Buffers](https://protobuf.dev/) -- Google's binary serialization format. Referenced in Lesson 11 as the gRPC alternative to JSON. *(Lesson 11)*

---

## Security and Threat Modeling

- **"Threat Modeling: Designing for Security"** by Adam Shostack -- The definitive guide to threat modeling, including STRIDE methodology used in Lesson 14. *(Lesson 14)*
- [OWASP Threat Modeling](https://owasp.org/www-community/Threat_Modeling) -- OWASP's threat modeling resources and cheat sheets. *(Lesson 14)*
- [RFC 6347](https://datatracker.ietf.org/doc/html/rfc6347) -- DTLS 1.2 specification. The encryption layer for WebRTC DataChannels. *(Lessons 03, 08)*
- [coturn use-auth-secret](https://github.com/coturn/coturn/wiki/turnserver#turn-rest-api) -- Documentation for the ephemeral credential mechanism used by chatixia-mesh. *(Lesson 08)*

---

## AI Agent Frameworks

- [Google A2A Protocol](https://github.com/google/A2A) -- Agent-to-Agent protocol specification. chatixia-mesh implements A2A Agent Cards. *(Lesson 09)*
- [Anthropic MCP](https://modelcontextprotocol.io/) -- Model Context Protocol for connecting LLMs to external tools. chatixia-mesh agents support MCP integration. *(Lesson 09)*
- [CrewAI](https://github.com/crewAIInc/crewAI) -- Role-based multi-agent framework. Inspired chatixia-mesh's role templates (researcher, analyst, coordinator, worker). *(Lesson 09)*
- [AutoGen](https://github.com/microsoft/autogen) -- Microsoft's multi-agent conversation framework. *(Lesson 09)*
- [LangGraph](https://github.com/langchain-ai/langgraph) -- LangChain's framework for building agent workflows as graphs. *(Lesson 09)*

---

## Sidecar Pattern and Service Mesh

- [Envoy Proxy](https://www.envoyproxy.io/) -- The most widely deployed sidecar proxy. Used by Istio for service mesh. *(Lesson 10)*
- [Dapr](https://dapr.io/) -- Distributed Application Runtime. A sidecar that provides building blocks for microservices. *(Lesson 10)*
- [Linkerd](https://linkerd.io/) -- Lightweight service mesh with sidecar proxies for mTLS and observability. *(Lesson 10)*

---

## Deployment and DevOps

- [Docker Compose documentation](https://docs.docker.com/compose/) -- Reference for the multi-service orchestration used by chatixia-mesh. *(Lesson 15)*
- [Multi-stage Docker builds](https://docs.docker.com/build/building/multi-stage/) -- Official guide to the build pattern used by the registry and sidecar Dockerfiles. *(Lesson 15)*
- [Cloudflare Tunnel](https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/) -- Zero-trust tunnel documentation. Used to expose the registry across networks. *(Lesson 15)*

---

## Architecture Decision Records

- [Michael Nygard's original ADR blog post](https://cognitect.com/blog/2011/11/15/documenting-architecture-decisions) -- The origin of the ADR format used throughout chatixia-mesh. *(Lesson 16)*
- [ADR GitHub organization](https://adr.github.io/) -- Collection of ADR tools, templates, and examples. *(Lesson 16)*
- [Lightweight Architecture Decision Records](https://www.thoughtworks.com/radar/techniques/lightweight-architecture-decision-records) -- ThoughtWorks Technology Radar entry on ADRs. *(Lesson 16)*

---

## Testing

- **"Release It!"** by Michael Nygard -- Patterns for building resilient distributed systems, including stability patterns and testing strategies. *(Lesson 17)*
- [pytest-asyncio](https://pytest-asyncio.readthedocs.io/) -- Plugin for testing async Python code, used by the chatixia-mesh agent test suite. *(Lesson 17)*
- [Tokio testing guide](https://tokio.rs/tokio/topics/testing) -- How to write async tests in Rust with `#[tokio::test]`. *(Lesson 17)*

---

## Dashboard and Frontend

- [HTML5 Canvas API](https://developer.mozilla.org/en-US/docs/Web/API/Canvas_API) -- Reference for the canvas rendering used by the NetworkTopology component. *(Lesson 13)*
- [React 19 documentation](https://react.dev/) -- Official React docs. The hub dashboard uses React 19 with hooks. *(Lesson 13)*
