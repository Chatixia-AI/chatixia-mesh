# Reading List

Curated resources for deepening your understanding of the topics covered in this curriculum. Organized by topic area, with references to the relevant lessons.

---

## Distributed Systems

**Books:**

- **"Designing Data-Intensive Applications, 2nd Edition"** by Martin Kleppmann and Chris Riccomini ([O'Reilly](https://learning.oreilly.com/library/view/-/9781098119058/), 2026) -- The definitive guide to distributed systems fundamentals, fully updated. Chapters on replication, partitioning, consistency, and consensus are directly relevant. *(Lessons 01, 12)*
- **"Designing Distributed Systems, 2nd Edition"** by Brendan Burns ([O'Reilly](https://learning.oreilly.com/library/view/-/9781098156343/), 2024) -- Patterns for distributed systems including sidecar, ambassador, and adapter patterns. Directly maps to chatixia-mesh's architecture. *(Lessons 01, 10)*
- **"Understanding Distributed Systems"** by Roberto Vitillo -- A more accessible introduction covering networking, coordination, and resilience patterns. *(Lesson 01)*

**Online:**

- [The Fallacies of Distributed Computing](https://en.wikipedia.org/wiki/Fallacies_of_distributed_computing) -- Peter Deutsch's eight fallacies, referenced in Lesson 01. *(Lesson 01)*
- [CAP Theorem Explained](https://martin.kleppmann.com/2015/05/11/please-stop-calling-databases-cp-or-ap.html) -- Martin Kleppmann's critique of oversimplified CAP reasoning. *(Lesson 01)*

---

## WebRTC and P2P Networking

**Books:**

- **"Programming WebRTC"** by Karl Stolley ([Pragmatic Bookshelf](https://learning.oreilly.com/library/view/-/9798888651100/), 2024) -- Modern, hands-on guide to WebRTC including DataChannels and signaling. The most up-to-date WebRTC book available. *(Lessons 02, 03, 05)*
- **"Real-Time Communication with WebRTC"** by Salvatore Loreto and Simon P. Romano ([O'Reilly](https://learning.oreilly.com/library/view/-/9781449371869/), 2014) -- Covers the full WebRTC stack from signaling to DataChannels. *(Lessons 02, 03, 05)*
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

- **"Async Rust"** by Maxwell Flitton and Caroline Morton ([O'Reilly](https://learning.oreilly.com/library/view/-/9781098149086/), 2024) -- Covers tokio, channels, futures, and async patterns. Directly relevant to the registry and sidecar codebase. *(Lesson 04)*
- **"Asynchronous Programming in Rust"** by Carl Fredrik Samson ([Packt](https://learning.oreilly.com/library/view/-/9781805128137/), 2024) -- Deeper dive into executors, pinning, and wakers for understanding the async runtime. *(Lesson 04)*
- [Tokio Tutorial](https://tokio.rs/tokio/tutorial) -- Official tutorial for the Tokio async runtime used by the registry and sidecar. Covers spawning, channels, select, and I/O. *(Lesson 04)*
- [Async Rust Book](https://rust-lang.github.io/async-book/) -- The official Rust async/await reference. Covers futures, executors, and pinning. *(Lesson 04)*
- [DashMap documentation](https://docs.rs/dashmap/latest/dashmap/) -- API reference for the concurrent HashMap used throughout the registry. *(Lessons 04, 12)*

**Python:**

- **"Python Concurrency with asyncio"** by Matthew Fowler ([Manning](https://learning.oreilly.com/library/view/-/9781617298660/), 2022) -- Comprehensive guide to asyncio coroutines, tasks, and event loops. Directly relevant to the agent runner and mesh client. *(Lesson 04)*
- **"Using Asyncio in Python"** by Caleb Hattingh ([O'Reilly](https://learning.oreilly.com/library/view/-/9781492075325/), 2020) -- Concise, practical guide to asyncio with real-world patterns. *(Lesson 04)*
- [Python asyncio documentation](https://docs.python.org/3/library/asyncio.html) -- Official reference for Python's async I/O library used by the agent runner and mesh client. *(Lesson 04)*
- [Real Python: Async IO in Python](https://realpython.com/async-io-python/) -- Practical tutorial with examples of coroutines, tasks, and event loops. *(Lesson 04)*

---

## Protocol Design

- [JSON Lines](https://jsonlines.org/) -- Specification for the newline-delimited JSON format used by the sidecar IPC protocol. *(Lesson 06)*
- [RFC 7519](https://datatracker.ietf.org/doc/html/rfc7519) -- JWT: JSON Web Tokens. The token format used for WebSocket authentication. *(Lesson 08)*
- [Protocol Buffers](https://protobuf.dev/) -- Google's binary serialization format. Referenced in Lesson 11 as the gRPC alternative to JSON. *(Lesson 11)*

---

## Security and Threat Modeling

- **"Threat Modeling: Designing for Security"** by Adam Shostack ([Wiley](https://learning.oreilly.com/library/view/-/9781118810057/), 2014) -- The definitive guide to threat modeling, including STRIDE methodology used in Lesson 14. *(Lesson 14)*
- **"Threat Modeling"** by Izar Tarandach and Matthew J. Coles ([O'Reilly](https://learning.oreilly.com/library/view/-/9781492056546/), 2020) -- Modern, practical approach to threat modeling with updated methodologies. *(Lesson 14)*
- **"Threat Modeling Best Practices"** by Derek Fisher ([Packt](https://learning.oreilly.com/library/view/-/9781805128250/), 2025) -- Latest techniques for threat modeling distributed applications. *(Lesson 14)*
- [OWASP Threat Modeling](https://owasp.org/www-community/Threat_Modeling) -- OWASP's threat modeling resources and cheat sheets. *(Lesson 14)*
- [RFC 6347](https://datatracker.ietf.org/doc/html/rfc6347) -- DTLS 1.2 specification. The encryption layer for WebRTC DataChannels. *(Lessons 03, 08)*
- [coturn use-auth-secret](https://github.com/coturn/coturn/wiki/turnserver#turn-rest-api) -- Documentation for the ephemeral credential mechanism used by chatixia-mesh. *(Lesson 08)*

---

## AI Agent Frameworks

**Books & Courses:**

- **"Building Applications with AI Agents"** by Michael Albada ([O'Reilly](https://learning.oreilly.com/library/view/-/9781098176495/), 2025) -- Agent design patterns, tool use, and orchestration strategies. *(Lesson 09)*
- **"Agentic Architectural Patterns for Building Multi-Agent Systems"** by Ali Arsanjani and Juan Pablo Bustos ([Packt](https://learning.oreilly.com/library/view/-/9781806029570/), 2026) -- Architecture patterns for multi-agent systems. Closely aligned with chatixia-mesh's design. *(Lesson 09)*
- **"Modern AI Agents: Building Single- and Multi-Agent Systems with MCP and LLMs, 2nd Edition"** by Sinan Ozdemir ([Pearson video course](https://learning.oreilly.com/videos/-/9780135882634/), 2025) -- Covers MCP integration and multi-agent patterns. *(Lesson 09)*
- [AI Agents Skill Plan](https://learning.oreilly.com/skill/-/0642572295110/) -- O'Reilly curated learning path for AI agent development. *(Lesson 09)*

**Projects & Specifications:**

- [Google A2A Protocol](https://github.com/google/A2A) -- Agent-to-Agent protocol specification. chatixia-mesh implements A2A Agent Cards. *(Lesson 09)*
- [Anthropic MCP](https://modelcontextprotocol.io/) -- Model Context Protocol for connecting LLMs to external tools. chatixia-mesh agents support MCP integration. *(Lesson 09)*
- [CrewAI](https://github.com/crewAIInc/crewAI) -- Role-based multi-agent framework. Inspired chatixia-mesh's role templates (researcher, analyst, coordinator, worker). *(Lesson 09)*
- [AutoGen](https://github.com/microsoft/autogen) -- Microsoft's multi-agent conversation framework. *(Lesson 09)*
- [LangGraph](https://github.com/langchain-ai/langgraph) -- LangChain's framework for building agent workflows as graphs. *(Lesson 09)*

---

## Sidecar Pattern and Service Mesh

**Books:**

- **"Building Microservices, 2nd Edition"** by Sam Newman ([O'Reilly](https://learning.oreilly.com/library/view/-/9781492034018/), 2021) -- Covers sidecar proxies, service mesh, decomposition, and inter-service communication patterns. *(Lessons 10, 06)*
- **"Microservices Patterns"** by Chris Richardson ([Manning](https://learning.oreilly.com/library/view/-/9781617294549/), 2018) -- IPC patterns, service discovery, and observability for distributed services. *(Lessons 10, 06)*

**Projects:**

- [Envoy Proxy](https://www.envoyproxy.io/) -- The most widely deployed sidecar proxy. Used by Istio for service mesh. *(Lesson 10)*
- [Dapr](https://dapr.io/) -- Distributed Application Runtime. A sidecar that provides building blocks for microservices. *(Lesson 10)*
- [Linkerd](https://linkerd.io/) -- Lightweight service mesh with sidecar proxies for mTLS and observability. *(Lesson 10)*

---

## Deployment and DevOps

**Books:**

- **"Bootstrapping Microservices with Docker, Kubernetes, and Terraform"** by Ashley Davis ([Manning](https://learning.oreilly.com/library/view/-/9781617297212/), 2021) -- Practical guide to containerized deployments with multi-stage builds and compose orchestration. *(Lesson 15)*

**Online:**

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
