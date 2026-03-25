# Curriculum Changelog

All notable changes to the learning curriculum are documented here.

Format: `[YYYY-MM-DD] — Summary of changes`

---

## [2026-03-25] — Initial Release

### Added
- **00-curriculum-overview.md** — Table of contents, learning paths, dependency graph, time estimates
- **01-why-distributed-systems.md** — Topology models, eight fallacies, control/data plane separation
- **02-peer-to-peer-networking.md** — NAT problem, STUN/TURN/ICE, UDP hole-punching, signaling
- **03-webrtc-fundamentals.md** — Protocol stack, SDP, DTLS, SCTP, DataChannels, connection lifecycle
- **04-async-programming-patterns.md** — Tokio, asyncio, channels, DashMap, concurrent data structures
- **05-signaling-protocol-design.md** — JSON/WebSocket protocol, sequence diagrams, sender verification, peer list filtering
- **06-inter-process-communication.md** — Unix sockets, JSON-lines, request/response correlation, sidecar lifecycle
- **07-application-protocol-design.md** — MeshMessage, task state machine, dual execution paths, graceful degradation
- **08-authentication-and-security.md** — JWT, API keys, device pairing, ephemeral TURN credentials, DTLS encryption
- **09-ai-agent-architecture.md** — Skills, LLM tool-use loop, agent lifecycle, multi-agent collaboration, role templates
- **10-sidecar-pattern.md** — Process isolation, API boundaries, cross-language interop, binary distribution
- **11-transport-comparison.md** — WebRTC vs HTTP vs gRPC, devil's advocate analysis, rebuttals, decision matrix
- **12-state-management-without-a-database.md** — DashMap, heartbeat-based consistency, TTL patterns, health state machine
- **13-building-monitoring-dashboards.md** — Polling vs push, canvas-based topology, design systems, component architecture
- **14-threat-modeling.md** — STRIDE applied to chatixia-mesh, WebRTC attack surface, production security checklist
- **15-deployment-patterns.md** — Docker Compose, multi-stage builds, Cloudflare Tunnel, cross-network connectivity tiers
- **16-architecture-decision-records.md** — ADR methodology, task execution evolution case study, devil's advocate ADRs
- **17-testing-distributed-systems.md** — Testing pyramid, async testing, E2E gap case study (Session 4), CI for polyglot projects
- **glossary.md** — 60+ terms with lesson cross-references
- **reading-list.md** — 40+ curated books, RFCs, projects, and online resources
- **diagrams/** — 5 reusable ASCII diagrams (architecture overview, WebRTC stack, signaling sequence, task state machine, connectivity tiers)
- **CHANGELOG.md** — This file
