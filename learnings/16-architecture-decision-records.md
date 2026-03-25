# Lesson 16: Architecture Decision Records -- Making Decisions Visible and Reversible

> Prerequisites: Lesson 11 (Transport Comparison). You should understand the WebRTC vs HTTP vs gRPC trade-offs and be familiar with how chatixia-mesh uses WebRTC DataChannels for agent-to-agent communication.

---

## Introduction

Six months from now, someone will look at the chatixia-mesh codebase and ask: "Why does every Python agent spawn a separate Rust process just to send a JSON message?" The code will show *what* happens -- a Unix socket connection, JSON-line serialization, a sidecar binary. The git log will show *when* it was added. But neither will explain *why* this architecture was chosen over simpler alternatives.

The answer lives in ADR-001:

> **Context:** Python agents need to communicate over WebRTC DataChannels, but the Python WebRTC ecosystem (aiortc) is fragile, hard to debug, and lacks production-grade DTLS support.
>
> **Decision:** Each Python agent spawns a Rust sidecar process that handles all WebRTC/signaling complexity. The agent communicates with its sidecar via a Unix domain socket using a simple JSON-line protocol.

That is the entire justification, in two paragraphs. Without it, the next engineer might reasonably conclude the sidecar is unnecessary complexity and attempt to rip it out -- recreating a decision that was already made, evaluated, and accepted with full knowledge of its trade-offs.

Architecture Decision Records (ADRs) are short documents that capture the context, decision, and consequences of significant technical choices. They are the most valuable engineering artifact a team produces, because they preserve the *reasoning* that every other artifact -- code, tests, configuration -- only implies.

This lesson teaches the ADR format, walks through a three-ADR evolution chain from the chatixia-mesh project, examines what makes an exceptionally honest ADR, and gives you criteria for when to write one.

---

## 1. Why Document Decisions?

### The three artifacts of engineering work

Every line of code in a system is the result of a decision, but the code itself captures only a fraction of the information that led to it.

| Artifact | What it tells you | What it does not tell you |
|----------|-------------------|--------------------------|
| **Code** | What the system does right now | Why it does it this way instead of another way |
| **Git history** | When each change was made and by whom | Why those changes were chosen over alternatives |
| **ADRs** | Why a decision was made, what alternatives were considered, what trade-offs were accepted | -- |

Code is the *what*. Commits are the *when*. ADRs are the *why*.

### The cost of missing context

Without ADRs, teams experience a recurring pattern:

1. **Engineer A** evaluates three approaches, chooses one, implements it.
2. **Engineer B** joins six months later, reads the code, finds it confusing.
3. **Engineer B** proposes rewriting it using one of the approaches Engineer A already rejected.
4. **The team** spends a week debating the same trade-offs that were already evaluated.
5. Either the rewrite happens (redoing work) or it does not (wasting a week of debate).

Both outcomes are expensive. ADRs prevent this by making the original evaluation accessible to future readers. They do not prevent revisiting decisions -- they make revisiting efficient. When Engineer B reads the ADR, they can either accept the reasoning or challenge it with new information, skipping the "rediscovery of constraints" phase entirely.

### Decisions are the most perishable knowledge

Code persists in the repository. Tests persist in CI. Documentation persists in the docs folder. But the reasoning behind a decision exists only in the heads of the people who were in the room (or the Slack thread, or the design review). When those people leave, the reasoning leaves with them.

ADRs convert ephemeral knowledge into a durable artifact.

---

## 2. The ADR Format

### The Michael Nygard template

The standard ADR format was proposed by Michael Nygard in a 2011 blog post. It has five sections:

| Section | Purpose | Length |
|---------|---------|--------|
| **Date** | When the decision was made | One line |
| **Status** | Current state: proposed, accepted, deprecated, superseded | One line |
| **Context** | The situation that requires a decision -- forces, constraints, requirements | 1-3 paragraphs |
| **Decision** | What we decided to do | 1-3 paragraphs |
| **Consequences** | What follows from the decision -- both positive and negative | Bulleted list |

The format is deliberately short. An ADR should take 10-30 minutes to write and 5 minutes to read. If it takes longer, you are writing a design document, not an ADR.

### The chatixia-mesh extension: Migration Path

chatixia-mesh adds a sixth section to several ADRs:

> **Migration path:** Add PostgreSQL for task queue and agent registry when persistence or multi-instance is needed.

-- ADR-004: In-Memory State (No Database)

The Migration Path answers the question: "If this decision turns out to be wrong, how do we reverse it?" This is valuable because it forces the author to think about reversibility at decision time, not when things are already broken. It also reassures future readers that the team considered the exit strategy.

Not every ADR needs a migration path. Decisions that are cheap to reverse (changing a log format, choosing a linting tool) do not need one. Decisions that are expensive to reverse (choosing a transport protocol, choosing a database, choosing an IPC format) do.

### A complete example

Here is ADR-004 from chatixia-mesh, demonstrating the full format:

```markdown
## ADR-004: In-Memory State (No Database)

**Date:** 2026-03-21
**Status:** Accepted

**Context:** Registry needs to track agents, tasks, and signaling peers.
Options: database (PostgreSQL, Redis) or in-memory (DashMap).

**Decision:** All state is in-memory using `DashMap` (concurrent hash maps).
No database dependency.

**Consequences:**

- (+) Zero deployment complexity -- single binary, no external services
- (+) Very fast reads/writes
- (-) No durability -- restart loses all state
- (-) Single-instance only (no horizontal scaling)

**Migration path:** Add PostgreSQL for task queue and agent registry
when persistence or multi-instance is needed.
```

Notice the structure:

- **Context** explains the constraint (registry needs state storage) and names the alternatives (PostgreSQL, Redis, DashMap).
- **Decision** is a single sentence. No justification here -- that is implicit in the consequences.
- **Consequences** uses `(+)` and `(-)` markers to distinguish benefits from costs. Honest ADRs always have both.
- **Migration path** names the specific technology (PostgreSQL) and the specific trigger (persistence or multi-instance).

### Status transitions

ADR statuses follow a lifecycle:

```
proposed --> accepted --> deprecated
                     --> superseded by ADR-XXX
```

- **Proposed**: under discussion, not yet committed to.
- **Accepted**: the team has committed to this decision.
- **Deprecated**: the decision is no longer followed, but no replacement was defined.
- **Superseded**: a newer ADR replaces this one. The old ADR links to the new one.

In chatixia-mesh, all 18 ADRs have `Accepted` status. This is typical of a project in active development -- decisions are made, implemented, and move forward. Superseded and deprecated statuses appear as the project matures and earlier decisions are revisited.

---

## 3. Case Study: The Evolution of Task Execution

The most instructive ADRs are not individual records but *chains* -- sequences of decisions where each one builds on or corrects the one before. chatixia-mesh has a three-ADR chain that traces the evolution of task execution from a simple workaround to a mature P2P architecture.

### ADR-005: The simple first approach

**Problem:** Python skill handlers are synchronous (they run inside the LLM tool-use loop). The async `MeshClient` IPC bridge cannot be called from sync code. Agents need a way to delegate tasks to other agents.

**Decision:** Use the registry's REST API as a task queue. Sync handlers submit tasks via HTTP. The target agent picks up tasks on its next heartbeat.

```markdown
**Consequences:**

- (+) Works from synchronous Python code
- (+) Centralized task queue with status tracking
- (-) Higher latency than direct DataChannel (poll-based, ~3s intervals)
- (-) Bypasses P2P for these operations (goes through registry)

**Migration path:** Once the agent framework supports async skill handlers,
route through the sidecar DataChannel directly.
```

This ADR is honest about its limitations. It acknowledges that the HTTP task queue *contradicts* the system's P2P architecture. It routes all task data through the registry -- the exact thing the WebRTC DataChannel architecture was designed to avoid.

But it also explains *why* this compromise is acceptable: synchronous Python code cannot use the async IPC bridge. The constraint is real, and the workaround is pragmatic.

The migration path is specific: "Once the agent framework supports async skill handlers, route through the sidecar DataChannel directly." This is not a vague aspiration. It names the precondition (async handlers) and the target (DataChannel routing).

### ADR-013: Discovering that agents ignore their work

**Problem:** The registry assigns pending tasks to agents via the heartbeat response. But the Python runner's heartbeat loop discards the response -- it fires `requests.post()` and ignores the result. Tasks transition from `pending` to `assigned` on the server but are never executed.

This was discovered during end-to-end testing in Session 4. Task completion had to be simulated via direct API calls because no agent actually processed tasks.

**Decision:** Modify the runner's heartbeat loop to parse `pending_tasks` from the heartbeat response, look up the matching skill handler, execute it, and POST the result back.

```markdown
**Consequences:**

- (+) Agents actually execute delegated tasks -- closes the last gap
      in the task lifecycle
- (+) No new infrastructure -- reuses existing heartbeat polling
      and hub task API
- (+) Simple implementation -- skill handlers are already synchronous
      functions
- (-) Heartbeat interval (~15s) bounds task pickup latency
- (-) Inline execution blocks the heartbeat loop during skill execution
      -- acceptable for fast skills, needs async dispatch for slow ones
```

ADR-013 is notable for what it reveals about the development process. The original task execution path (ADR-005) had a bug that went undetected until E2E testing -- agents received task assignments but never acted on them. This is common in distributed systems where the happy path works but the full lifecycle has gaps that only integration testing exposes.

The ADR does not hide this. Its Context section says explicitly: "During E2E testing (Session 4), task completion had to be simulated via direct API calls." This kind of transparency is valuable for future engineers who might wonder why the heartbeat response parsing was added.

The consequence list identifies a new limitation: heartbeat-bounded latency (~15s). This sets the stage for ADR-016.

### ADR-016: The mature approach

**Problem:** Despite the system's P2P architecture, all task execution routes through the registry's REST API. The `delegate`, `mesh_send`, and `mesh_broadcast` skill handlers use synchronous HTTP calls. The target agent picks up tasks on heartbeat poll (~15s). This contradicts the core positioning: "registry is control plane only, agents talk directly."

**Decision:** Route task delegation through WebRTC DataChannels with automatic fallback to the registry task queue.

The ADR details six specific changes:

1. Sidecar emits peer lifecycle events (`peer_connected`, `peer_disconnected`) to the Python agent
2. MeshClient tracks connected peers from sidecar events
3. Skill handlers become async with P2P-first path
4. Runner registers a P2P task handler for incoming DataChannel requests
5. Non-blocking task execution via `asyncio.create_task()`
6. Registry fallback preserved for when P2P path is unavailable

```markdown
**Consequences:**

- (+) Agent-to-agent data flows directly over DTLS-encrypted
      DataChannels -- registry is truly out of the data path
- (+) Sub-second task delegation latency
      (vs. 3-15s with heartbeat polling)
- (+) Connected agents keep working if the registry goes down
      (P2P resilience)
- (+) Async handlers no longer block the heartbeat loop
- (+) Backward compatible -- HTTP fallback preserves behavior when
      P2P path is unavailable
- (-) Discovery still requires the registry -- agents can't find
      new peers without it
- (-) HTTP fallback path still uses synchronous urllib
      (acceptable since it's the backup path)
```

### Reading the chain

The three ADRs tell a story of iterative refinement:

```
ADR-005 (Sync HTTP task queue)
  Problem: sync Python can't use async IPC
  Solution: route tasks through registry REST API
  Latency: 3-15s (poll-based)
  Migration path: "once async handlers are supported"
      |
      v
ADR-013 (Heartbeat-driven execution)
  Problem: agents ignore heartbeat responses; tasks never execute
  Solution: parse and execute tasks from heartbeat response
  Latency: ~15s (heartbeat interval)
  New limitation: inline execution blocks heartbeat
      |
      v
ADR-016 (P2P DataChannel execution + HTTP fallback)
  Problem: all data still routes through registry
  Solution: async P2P-first path with HTTP fallback
  Latency: sub-second (P2P), 3-15s (fallback)
  Fulfills ADR-005's migration path
```

ADR-005's migration path -- "once the agent framework supports async skill handlers, route through the sidecar DataChannel directly" -- predicted ADR-016 before it existed. This is the value of migration paths: they create a traceable thread from current compromises to future improvements.

Each ADR in the chain is complete on its own. You can read ADR-016 without reading ADR-005 and understand the decision. But reading the chain reveals the engineering process: a pragmatic workaround, a testing-driven bug fix, and a principled refactoring. This is how real systems evolve.

---

## 4. The Devil's Advocate ADR

Most ADRs present the decision favorably -- the context explains why the choice is reasonable, and the consequences list more positives than negatives. This is natural; you document decisions you believe in.

ADR-018 takes a different approach. It presents the case for WebRTC DataChannels, then systematically argues against its own conclusion.

### Structure

ADR-018 has three sections that go beyond the standard format:

1. **The case for** -- two comparison tables (WebRTC vs HTTP, WebRTC vs gRPC) and a summary of nine advantages.
2. **The case against** -- eleven specific criticisms, each with data and examples.
3. **Trade-offs accepted** -- an explicit list of costs the team knowingly incurs.

The devil's advocate section does not pull punches. Here are excerpts from the criticisms:

On connection setup latency:

> ICE gathering + DTLS handshake takes 5-10s per peer (vs ~50-100ms for TCP+TLS). Full mesh formation with 10 agents can take minutes.

On NAT traversal solving a non-problem:

> The system already requires a central registry. Agents authenticate to it, heartbeat to it, and fall back to it for task routing. The "registry is not in the data path" principle is aspirational -- in practice, the registry is still a single point of failure for discovery, signaling, and task assignment.

On TURN negating P2P benefits:

> TURN relays all traffic through a server -- eliminating the latency and bandwidth advantages of P2P entirely. You are back to the star topology, but with more protocol overhead than HTTP.

On the sidecar tax:

> Every agent deployment requires four moving parts where HTTP/gRPC would need one.

These are not strawman arguments. They are genuine weaknesses that the team acknowledges, quantifies, and accepts.

### Why this works

The devil's advocate ADR builds trust in three ways:

**1. It demonstrates thorough evaluation.** When a reader sees eleven specific criticisms addressed, they know the decision was not made casually. The team considered connection latency numbers, TURN relay costs, UDP blocking rates, SCTP head-of-line blocking, missing infrastructure (load balancing, circuit breaking, observability), sidecar complexity, per-connection memory overhead, library maturity, security audit surface, and the WebTransport successor. No one can say "they didn't think about X."

**2. It enables future re-evaluation.** ADR-018 includes explicit conditions for reconsideration:

> WebRTC should be replaced if: (1) all agents run in the same network (NAT traversal unnecessary -- switch to gRPC), (2) agent count exceeds ~30 (O(N^2) unsustainable), (3) webrtc-rs stalls, or (4) WebTransport over QUIC matures as a simpler alternative.

These are testable conditions. In six months, the team can check each one and know whether the decision still holds. This converts "should we reconsider WebRTC?" from an open-ended debate into a checklist.

**3. It ends the decision with honesty, not certainty.** The final consequence is:

> (-) The honest question remains: do enough real deployments span NAT boundaries to justify the cost?

This is not a weakness -- it is intellectual honesty. The team does not know the answer. Documenting the uncertainty is more valuable than pretending certainty.

### How to write a devil's advocate section

The structure is straightforward:

1. Present your decision with its benefits.
2. For each benefit, ask: "Under what conditions is this benefit irrelevant?"
3. For each complexity cost, ask: "What does the simpler alternative give you for free?"
4. Quantify where possible (latency numbers, memory usage, cost estimates).
5. State explicit conditions under which you would reverse the decision.
6. Do not resolve the tension. Let both sides stand.

The goal is not to undermine the decision but to make it robust. A decision that survives its own devil's advocate is stronger than one that was never challenged.

---

## 5. When to Write an ADR

Not every decision needs an ADR. Writing one for every choice would drown the signal in noise. Here are criteria for when an ADR adds value.

### Write an ADR when the decision is:

**Irreversible or expensive to reverse.** Choosing a transport protocol (WebRTC vs gRPC) affects every component in the system. Changing it later requires rewriting the sidecar, the IPC protocol, the skill handlers, and the deployment model. ADR-018 documents this.

**Cross-cutting -- affects multiple components.** The sidecar pattern (ADR-001) affects the sidecar, the agent framework, the CLI, and deployment. In-memory state (ADR-004) affects the registry, the hub, and operational procedures. Decisions that touch one file rarely need ADRs.

**Likely to be questioned later.** If you find yourself thinking "someone will ask why we did this," write an ADR now while the context is fresh. ADR-007 (Atmospheric Luminescence UI) explains a design system choice that would otherwise seem arbitrary.

**Involves significant trade-offs.** If there are strong arguments on both sides, an ADR captures those arguments for future reference. ADR-002 (Full Mesh) accepts O(N^2) connections for simplicity -- a trade-off that is reasonable at 10 agents but problematic at 50.

### Skip the ADR when the decision is:

**Cheap to reverse.** Choosing structlog over the standard logging library is a half-day refactor. No ADR needed.

**Local to one component.** Using `DashMap` instead of `RwLock<HashMap>` inside the registry is an implementation detail that affects one crate. It might warrant a code comment, not an ADR.

**Industry standard.** Using JWT for stateless authentication, JSON for configuration, or HTTPS for API calls are not decisions that need justification. They are defaults.

**Already documented elsewhere.** If a library's README explains why you should use it, you do not need to repeat that in an ADR.

### The chatixia-mesh filter

Here is how the filter applies to actual decisions in the project:

| Decision | ADR? | Why |
|----------|------|-----|
| Use WebRTC for agent-to-agent communication | Yes (ADR-018) | Irreversible, cross-cutting, significant trade-offs |
| Use in-memory state, no database | Yes (ADR-004) | Affects registry, hub, operations; will be questioned |
| Full mesh topology | Yes (ADR-002) | Cross-cutting, O(N^2) scaling concern, migration path needed |
| Use structlog for Python logging | No | Cheap to reverse, local to agent package |
| Use Vite for hub build tool | No | Industry standard choice, local to hub |
| Name the CLI `chatixia` | No | Naming is a preference, not an architecture decision |
| Use HMAC-SHA1 for TURN credentials | Yes (ADR-006) | Security-relevant, specific to coturn, will be questioned |

### A practical heuristic

If you are unsure whether a decision needs an ADR, ask: "Would I spend more than 30 minutes explaining this decision to a new team member, including the alternatives I considered?" If yes, write the ADR. It will take less than 30 minutes to write and will save that 30-minute explanation for every future reader.

---

## 6. Living Documentation

An ADR that describes a decision the codebase no longer follows is worse than no ADR at all. It actively misleads. Keeping documentation in sync with code requires a system, not good intentions.

### The chatixia-mesh documentation matrix

chatixia-mesh organizes its documentation into purpose-specific files with clear triggers for reading and updating:

| Document | What it contains | When to read | When to update |
|----------|-----------------|--------------|----------------|
| `docs/COMPONENTS.md` | Every file, struct, route, env var | Start of every session | When adding/removing files, modules, routes, env vars |
| `docs/SYSTEM_DESIGN.md` | Architecture, protocols, auth flows | When understanding architecture | When changing architecture or protocols |
| `docs/ADR.md` | All architecture decisions | When needing context on past decisions | When making a new architectural decision |
| `docs/GLOSSARY.md` | Domain-specific terms | When encountering unfamiliar terms | When introducing new terms |
| `docs/THREAT_MODEL.md` | Attack surfaces, mitigations | When working on security-relevant code | When adding new attack surfaces or mitigations |
| `docs/WEBRTC_VS_ALTERNATIVES.md` | Transport comparison | When discussing transport choices | When running experiments or updating data |
| `docs/DEPLOYMENT_GUIDE.md` | Cross-network deployment | When deploying agents | When changing deployment steps or tunnel config |

This matrix is not decorative. It is embedded in the project's `CLAUDE.md` file, where it serves as an operating instruction for anyone (human or AI) working on the codebase. The "when to update" column is the key -- it converts documentation maintenance from "remember to update docs" into "this code change triggers an update to this specific file."

### The principle: documentation is a side effect of code changes

Documentation falls out of date because updating it is treated as a separate task from writing code. The chatixia-mesh approach makes documentation updates a *side effect* of code changes:

- Add a new route to the registry? Update `COMPONENTS.md` with the route.
- Change the authentication flow? Update `SYSTEM_DESIGN.md` with the new flow.
- Make an architectural decision? Add an ADR to `ADR.md`.
- Introduce a new term? Append a row to `GLOSSARY.md`.

This works because each trigger is specific and each target is singular. "Update the docs" is vague and ignorable. "Add the new route to the routes table in `COMPONENTS.md`" is concrete and takes two minutes.

### Single-file ADR log

chatixia-mesh keeps all ADRs in a single file (`docs/ADR.md`) rather than one file per ADR. This is a deliberate choice for a project of this scale:

- **Searchable.** One file means one `Ctrl+F` to find any decision.
- **Readable in sequence.** ADRs often reference each other (ADR-016 references ADR-005 and ADR-013). A single file lets you scroll between them.
- **Low overhead.** Adding an ADR means appending to one file, not creating a new file, updating an index, and maintaining a naming convention.

For larger organizations with hundreds of ADRs, one file per ADR with an index is more practical. For a project with 18 ADRs, a single file is simpler.

### The documentation review trigger

How do you know when documentation is out of date? You do not -- unless you have triggers. Here are three that work:

1. **Code review.** When reviewing a PR that changes a route, a protocol, or an architecture component, ask: "Does the relevant doc still match?" This catches drift at the point where it is introduced.

2. **Session boundary.** chatixia-mesh creates meeting notes at the end of every work session that summarize decisions made. These notes are local-only (not committed), but they serve as a prompt to update the committed docs before the context fades.

3. **New contributor onboarding.** When a new person reads the docs and finds something that does not match the code, that is a documentation bug. Treat it like a code bug -- fix it immediately.

---

## Summary

Architecture Decision Records preserve the *why* behind your system's architecture. The code shows what you built. The git log shows when you built it. ADRs show why you built it that way.

The format is simple: Date, Status, Context, Decision, Consequences, and optionally a Migration Path. Each ADR should take 10-30 minutes to write and 5 minutes to read.

The best ADRs are not sales pitches for the chosen approach. They are honest evaluations that include the arguments against the decision, the conditions under which the decision should be revisited, and the known costs the team is accepting. ADR-018 demonstrates this with its eleven-point devil's advocate section.

Not every decision needs an ADR. The filter: irreversible or expensive to reverse, affects multiple components, likely to be questioned, involves significant trade-offs. If you would spend more than 30 minutes explaining a decision to a new team member, write the ADR instead.

Documentation stays alive when updates are triggered by code changes, not by willpower. A documentation matrix that maps "type of code change" to "specific document to update" converts maintenance from a chore into a habit.

---

## Exercises

### Exercise 1: Write your own ADR

Write an ADR for a decision you made recently at work or on a personal project. Use the chatixia-mesh format:

```markdown
## ADR-XXX: [Title]

**Date:** YYYY-MM-DD
**Status:** Accepted

**Context:** [The situation and constraints]

**Decision:** [What you decided]

**Consequences:**

- (+) [Positive consequence]
- (-) [Negative consequence]

**Migration path:** [How to reverse if wrong]
```

Requirements:
- The Context section must name at least two alternatives you considered.
- The Consequences section must include at least one negative consequence.
- Include a Migration Path if the decision would take more than a day to reverse.

### Exercise 2: Analyze ADR-018

Read ADR-018 (WebRTC DataChannels over HTTP/gRPC) in `docs/ADR.md` and the extended analysis in `docs/WEBRTC_VS_ALTERNATIVES.md`. Then answer:

**(a)** Identify the three strongest arguments FOR WebRTC DataChannels in this project. For each, explain in one sentence why it matters specifically for chatixia-mesh (not for WebRTC in general).

**(b)** Identify the three strongest arguments AGAINST WebRTC DataChannels. For each, explain in one sentence the condition under which this argument would become decisive.

**(c)** List the four conditions for reconsideration stated in the ADR. For each, assess whether that condition is currently true, and what evidence you would need to check.

### Exercise 3: Write the PostgreSQL migration ADR

The team decides to replace DashMap with PostgreSQL for the task queue (`HubState`) and agent registry (`RegistryState`). Write this ADR.

Consider:
- What is the Context? (Hint: reread ADR-004's consequences and migration path.)
- What specific triggers made this decision necessary? (Invent plausible triggers: a production outage, a scaling requirement, a compliance audit.)
- What are the consequences? Think about: deployment complexity, latency, durability, horizontal scaling, migration effort, backward compatibility.
- What is the migration path back to in-memory if PostgreSQL becomes a bottleneck?

Your ADR should supersede ADR-004. Update ADR-004's status line as part of your answer.

### Exercise 4: Propose a documentation review process

chatixia-mesh has seven documentation files that must stay in sync with the codebase. Currently, updates are triggered by the documentation matrix in `CLAUDE.md`, but there is no formal review process.

Design a documentation review process that answers:

1. **How often** should each document be reviewed for accuracy? (Consider: some docs change frequently, others rarely.)
2. **By whom?** (Consider: the person who changed the code? A dedicated documentation owner? The whole team?)
3. **What triggers an update** beyond the matrix? (Consider: new releases, new contributors, customer-reported confusion.)
4. **How do you detect drift** between docs and code? (Consider: automated checks, PR templates, onboarding friction.)

Write your answer as a one-page proposal with concrete, implementable steps -- not abstract principles.

---

## Further Reading

- Michael Nygard, "Documenting Architecture Decisions" (2011) -- the original blog post defining the ADR format
- `docs/ADR.md` -- all 18 chatixia-mesh ADRs, from sidecar pattern to PyPI publishing
- `docs/WEBRTC_VS_ALTERNATIVES.md` -- the extended devil's advocate analysis referenced by ADR-018
- `docs/COMPONENTS.md` -- the codebase map, an example of living documentation at the file/struct/route level
- Joel Parker Henderson, "Architecture Decision Record" (GitHub collection) -- templates, examples, and tooling from many organizations
