# Lesson 18: When Handshakes Fail -- Offer Glare, Early Candidates, and Re-dialing

**Prerequisites:** [Lesson 03: WebRTC Fundamentals](03-webrtc-fundamentals.md), [Lesson 05: Signaling Protocol Design](05-signaling-protocol-design.md), [Lesson 17: Testing Distributed Systems](17-testing-distributed-systems.md)

**Time estimate:** 75-90 minutes

**Key source files:**
- `sidecar/src/signaling.rs` -- `handle_signaling_message` (glare check, candidate buffering), `glare_keep_local_offer`
- `sidecar/src/mesh.rs` -- `MeshManager` (`pending_candidates`, `queue_candidate`, `take_candidates`, `flush_candidates`, `remove_peer_if_pc`, `remove_peer_if_channel`, `signaling_tx`)
- `sidecar/src/webrtc_peer.rs` -- `setup_ice_forwarding` (connection state handler and re-dial), `should_redial`, `REDIAL_DELAY`, `initiate_connection`, `handle_offer`
- `sidecar/src/main.rs` -- explicit rustls crypto provider
- `registry/src/main.rs` -- `handle_ws` (where a peer enters the signaling map)
- `registry/src/signaling.rs` -- `SignalingState::handle_message` (`register` produces `peer_list`)
- `docs/ADR.md` -- ADR-021 (glare and early candidates), ADR-022 (re-dial), ADR-025 (finish each offer before the next signaling message)
- `tests/integration/` -- the two-sidecar integration test (`harness.py`, `test_two_sidecars.py`)

---

## What You'll Learn

- Why two peers that dial each other at the same moment can deadlock ("offer glare"), and how a deterministic tie-break on `peer_id` resolves it
- Why trickled ICE candidates can arrive before their connection can accept them, and how a per-peer buffer fixes it
- Why a connection that has been replaced must never be allowed to remove its replacement, and how an identity check enforces that
- How ICE liveness checks detect a dead path while the signaling WebSocket stays up, and how the re-dial path heals it without a process restart
- Why unit tests missed all of this, how a two-sidecar integration test found the last glare race, and what is still open

---

## Introduction

Lessons 03 and 05 drew the WebRTC handshake as a clean sequence: one side offers, the other answers, candidates trickle across, a DataChannel opens. That diagram is correct, but it only shows the happy path. The handshake runs between two independent processes and a relay, each with its own scheduler, and nothing in the protocol stops them from acting at the same moment.

On 2026-09-22, chatixia-world's Phase 2 ran two real sidecars against one registry for the first time: two world instances on one laptop, started together by chatixia-world's `scripts/mesh-demo.sh`. The unit tests were green. The handshake failed in three different ways (ADR-021), and a fourth failure showed up in a longer session, recorded the same day (ADR-022):

| # | Symptom | Root cause | Decision |
|---|---------|------------|----------|
| 1 | Handshake stalls; log says `invalid proposed signaling state transition from stable applying remote answer` | Both sidecars offered at once and both backed off | ADR-021: glare tie-break by `peer_id` |
| 2 | ICE times out after 30 s on one machine | Early ICE candidates were silently dropped, including the only usable one | ADR-021: buffer early candidates |
| 3 | Panic on the first DTLS handshake, right after ICE connects | Two rustls crypto providers compiled in, none chosen | ADR-021: install `ring` in `main()` |
| 4 | Peer disappears after a laptop doze and never comes back | Connection failed while signaling stayed up; nothing retried | ADR-022: re-dial over the live signaling channel |

This lesson works through failures 1, 2 and 4, plus a bug that fixing 1 exposed: a replaced connection removing its replacement. For each one it covers the mechanism, the fix in the current code, and what the fix still leaves open. Failure 3 comes up in Section 6, because the reason it went unnoticed matters more than the fix.

---

## 1. The Assumption Hidden in the Happy Path

Look again at Lesson 05's sequence diagram. A registers, gets an empty `peer_list`, and waits. B registers later, gets `peer_list: ["A"]`, and offers. Exactly one side offers because registrations happen one after the other.

Nothing in the system enforces that order. Here is where a peer enters the registry's signaling map, in `registry/src/main.rs`:

```rust
async fn handle_ws(mut socket: WebSocket, peer_id: String, state: AppState) {
    // Create a channel for sending messages to this peer
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();

    // Register this peer's sender
    state.signaling.add_peer(&peer_id, tx);
    info!("[WS] peer connected: {}", peer_id);
```

The peer is added as soon as the WebSocket upgrade completes, before it sends `register`. When the `register` message arrives, `SignalingState::handle_message` in `registry/src/signaling.rs` answers with everyone currently in the map:

```rust
"register" => {
    info!("[SIG] register from peer_id={}", msg.peer_id);
    // Only authorized peers see other authorized peers
    let peers: Vec<String> = if is_authorized(&msg.peer_id) {
        self.connected_peers()
            .into_iter()
            .filter(|p| p != &msg.peer_id && is_authorized(p))
            .collect()
    } else {
```

Each WebSocket gets its own task, and the map is a `DashMap` with no ordering between tasks. If A and B connect within a few milliseconds of each other, both upgrades land before either `register` is processed:

```
  Sidecar A                      Registry                      Sidecar B
      |                              |                              |
      |== WebSocket upgrade ========>| add_peer(A)                  |
      |                              | add_peer(B) <== WS upgrade ==|
      |                              |                              |
      |-- register ----------------->|                              |
      |                              |<----------------- register --|
      |<-- peer_list ["B"] ----------|                              |
      |                              |---------- peer_list ["A"] -->|
      |                              |                              |
   "B is online, I'll offer"         |        "A is online, I'll offer"
```

Both sidecars now believe they are the caller. Moving `add_peer` to the `register` handler would make this rarer, not impossible, because two `register` messages on two tasks can still interleave. ADR-021 therefore leaves the registry alone: "glare is legal in WebRTC and must be handled by peers anyway."

Simultaneous startup is not the only source of offers. The sidecar starts an offer from three places:

| Trigger | Code path |
|---------|-----------|
| A `peer_list` names a peer we are not connected to | `handle_signaling_message`, `"peer_list"` arm, awaits `webrtc_peer::initiate_connection` (spawned it before ADR-025) |
| The Python agent sends an IPC `connect` command | `sidecar/src/ipc.rs`, spawns `webrtc_peer::initiate_connection` |
| A connection fails and the sidecar re-registers (Section 5) | `setup_ice_forwarding` sends `register`, which produces a new `peer_list` |

Any two of these can fire on opposite sides of the same pair. In the re-dial case, as you will see, they fire together on purpose. Whether the offer is awaited or spawned turns out to matter a lot (Section 2).

One registry detail has changed since these bugs were found. Since ADR-024, the registry relays `offer`, `answer` and `ice_candidate` only when both the sender and the target are approved or API-key peers, which is the same check `register` uses to build the `peer_list`. The two sidecars in this lesson are both authorized, so the relay behaves exactly as before. It just no longer carries handshakes to or from peers that are not approved.

---

## 2. Offer Glare

### The signaling state machine

Every `RTCPeerConnection` tracks where it is in the offer/answer exchange. The states come from JSEP (RFC 9429), and webrtc-rs exposes them as `RTCSignalingState`. The sidecar never uses provisional answers, so only three states matter here:

```
                   set_local_description(offer)
              +-------------------------------------> +-------------------+
              |                                       | have-local-offer  |
              |   +---------------------------------- | (we are caller)   |
              |   |  set_remote_description(answer)   +-------------------+
              |   v
         +----------+
         |  stable  |
         +----------+
              |   ^
              |   |  set_local_description(answer)    +-------------------+
              |   +---------------------------------- | have-remote-offer |
              |                                       | (we are callee)   |
              +-------------------------------------> +-------------------+
                   set_remote_description(offer)
```

Only a connection in `have-local-offer` can accept an answer. Give an answer to a connection in `stable` and webrtc-rs rejects it with `invalid proposed signaling state transition from stable applying remote answer`, which is the error in ADR-021's log.

**Offer glare** (also called an offer collision) is when both peers are in `have-local-offer` for the same pair and each receives the other's offer. Neither offer is wrong on its own. The two sides just have to agree which one survives.

### What the sidecar did before ADR-021

Before the fix, the `"offer"` arm of `handle_signaling_message` always spawned `handle_offer`, which builds a new answering connection and stores it with `mesh.add_peer`, overwriting whatever was there. Both sides did this at once:

```
  Sidecar A                      Registry                      Sidecar B
      |                              |                              |
  pcA1: have-local-offer             |             pcB1: have-local-offer
      |-- offer (pcA1) ------------->|----------------------------->|
      |<-----------------------------|<------------- offer (pcB1) --|
      |                              |                              |
  replace pcA1 with pcA2             |             replace pcB1 with pcB2
  pcA2 answers (-> stable)           |             pcB2 answers (-> stable)
      |-- answer (pcA2) ------------>|----------------------------->|
      |<-----------------------------|<------------ answer (pcB2) --|
      |                              |                              |
  get_pc(B) = pcA2, stable           |             get_pc(A) = pcB2, stable
  set_remote_description(answer)     |             set_remote_description(answer)
  ERROR: "from stable applying       |             ERROR: same
          remote answer"             |
      |                              |                              |
            Both sides answered. Nobody is offering. Deadlock.
```

Each side politely gave up its own offer, so both ended up waiting as callees.

### The tie-break

The fix is in the `"offer"` arm of `handle_signaling_message` in `sidecar/src/signaling.rs`. This is the current code, including ADR-025's changes, which are explained below:

```rust
"offer" => {
    // Incoming offer — create answer
    let from_peer = msg.peer_id.clone();

    // Offer glare: both sides offered at once (e.g. two sidecars
    // registering in the same instant both see each other in their
    // peer_list). Deterministic tie-break so exactly one offer wins:
    // the lower peer_id keeps its offer, the higher one yields.
    let mut yielded = None;
    if let Some(existing) = mesh.get_pc(&from_peer) {
        if existing.signaling_state() == RTCSignalingState::HaveLocalOffer {
            if glare_keep_local_offer(local_peer_id, &from_peer) {
                info!(
                    "[SIG] offer glare with {}: keeping our offer (lower peer_id wins)",
                    from_peer
                );
                return;
            }
            info!(
                "[SIG] offer glare with {}: yielding to their offer",
                from_peer
            );
            yielded = Some(existing);
        }
    }

    // Awaited for the same reason as `peer_list`: the answering
    // connection is on record before the next message is read.
    if let Some(sdp) = msg.payload.get("sdp").and_then(|s| s.as_str()) {
        if let Err(e) = webrtc_peer::handle_offer(
            local_peer_id, &from_peer, sdp,
            sig_tx.clone(), mesh.clone(), to_agent_tx.clone(),
        )
        .await
        { /* log */ }
    }

    // Close the abandoned offer only after its replacement is on
    // record, so its Closed callback sees a stale connection and
    // does not report the peer as disconnected.
    if let Some(existing) = yielded {
        tokio::spawn(async move {
            let _ = existing.close().await;
        });
    }
}
```

The rule itself is one line:

```rust
fn glare_keep_local_offer(local_peer_id: &str, remote_peer_id: &str) -> bool {
    local_peer_id < remote_peer_id
}
```

With chatixia-world's peer IDs (`world-the-alpha` and `world-the-beta`, set in the world's `api_keys.json`), the same collision now resolves like this:

```
  A = world-the-alpha (lower)        Registry        B = world-the-beta (higher)
      |                                 |                               |
  pcA1: have-local-offer                |              pcB1: have-local-offer
      |-- offer (pcA1) ---------------->|------------------------------>|
      |<--------------------------------|<-------------- offer (pcB1) --|
      |                                 |                               |
  "alpha" < "beta": keep ours           |          "beta" > "alpha": yield
  return (B's offer ignored)            |          handle_offer -> pcB2 on record
      |                                 |          pcB2 answers pcA1's offer
      |<--------------------------------|<-------------- answer (pcB2) --|
      |                                 |          then close pcB1 (stale)
  pcA1.set_remote_description(answer)   |                               |
  -> stable                             |                               |
  flush_candidates(B)                   |                               |
      |                                 |                               |
      |<====== ICE checks / DTLS / SCTP / DataChannel "mesh" =========>|
```

Exactly one offer (pcA1's) survives, and exactly one answer (pcB2's) comes back.

### Why determinism matters

For the tie-break to work, both sides must reach opposite conclusions about the same collision without talking to each other. Any rule has to meet three conditions:

1. **Both sides have the inputs.** Each sidecar knows its own `peer_id` and the sender's `peer_id` from the offer. No clock, sequence number, or registry state is involved.
2. **It is antisymmetric.** For any two distinct IDs, exactly one side keeps its offer. If both kept, you would get the deadlock in reverse (two offers, no answers). If both yielded, you would get the original bug.
3. **It is stable.** The same pair gets the same answer every time, so a collision during a re-dial (Section 5) resolves the same way as one at startup.

Compare the alternatives:

| Approach | Problem |
|----------|---------|
| Random backoff ("wait 0-500 ms and retry") | Both sides can pick similar delays; needs a retry loop; test runs are not reproducible |
| Earliest timestamp wins | Two machines' clocks disagree, and ties are possible |
| Registry assigns a caller | Adds per-pair state to a relay that is meant to stay stateless about WebRTC (Lesson 05) |
| Compare `peer_id`s | Needs nothing new; one string comparison |

Two unit tests pin the rule itself down. `glare_rule_is_antisymmetric_for_any_distinct_ids` checks property 2 over a small set of IDs, including mixed case (`"Z"` sorts before `"a"` in byte order, and the rule still works because both sides use the same ordering).

### Relationship to "perfect negotiation"

Browsers solve the same problem with the pattern MDN calls **perfect negotiation**. One peer is **polite**: when an offer collides with its own, it drops its offer and answers. The other is **impolite**: it ignores the colliding offer and keeps its own. In chatixia-mesh, the lower `peer_id` plays the impolite role and the higher one the polite role.

There are two differences worth knowing:

- **Rollback vs a fresh connection.** A polite browser peer calls `setRemoteDescription(offer)` on the same connection, which rolls its own offer back. The sidecar instead closes the pending connection and builds a new one in `handle_offer`. A sidecar connection carries no media tracks or other state worth keeping, so a fresh `RTCPeerConnection` is simpler to reason about than rollback. The cost is that the closed connection's callbacks can still fire later, which leads to Section 4.
- **How a collision is detected.** MDN's example keeps a `makingOffer` flag and deliberately does not rely on `signalingState` alone, because "the value of `signalingState` changes asynchronously". The sidecar checks `existing.signaling_state() == RTCSignalingState::HaveLocalOffer`. That check is only reliable if our offer is guaranteed to be in `have-local-offer` before the peer's offer is read. ADR-021 did not guarantee that, which is the next part of the story.

### The race the tie-break missed (ADR-025)

ADR-021's tie-break only runs if our own offering connection is already on record in `have-local-offer` when the peer's offer is read. With ADR-021 alone, the signaling loop did not wait for that. The `"peer_list"` arm spawned `initiate_connection` and the `"offer"` arm spawned `handle_offer`, and the loop went straight back to reading the WebSocket. Inside `initiate_connection`, the offering connection only reaches `have-local-offer` after `create_peer_connection`, `mesh.add_peer`, `create_offer` and `set_local_description`. Until then, a crossing offer finds either no connection or one still in `stable`, and the glare check does not run:

```
  Sidecar A, ADR-021 code (both handlers spawned)

  read peer_list ["B"]  --> spawn task 1: initiate_connection(B)
  read offer from B     --> get_pc(B): None, or pcA1 still `stable`
                            -> not glare; spawn task 2: handle_offer(B)

      task 1: create pcA1 ... add_peer(B, pcA1) ... set_local_description ... send offer
      task 2: create pcA2 ... add_peer(B, pcA2) ... answer B's offer ... send answer

  Both tasks write peers["B"]. Whichever runs add_peer last wins the map,
  and A has sent both an offer and an answer.
```

If A's map ends up holding its offerer, A has answered B's offer from a connection it no longer tracks. If both sidecars end up holding their answerers, each rejects the other's answer with `stable applying remote answer`: the same deadlock as before ADR-021, reached by a different route. Nothing connects until ICE gives up 30 s later.

ADR-021's five manual restarts all passed, so this went unnoticed. It was found by the two-sidecar integration test (Section 6), which forces both sidecars to see each other in the same `peer_list`. **4 of 12** forced-glare runs failed, and one of those was the full deadlock where both sides answered.

The fix, ADR-025, removes the concurrency instead of adding a flag. The signaling loop now awaits `initiate_connection` for each peer in a `peer_list` and awaits `handle_offer`:

```rust
"peer_list" => {
    // Registry tells us about other connected peers — initiate offers.
    //
    // Awaited here, not spawned: the offer must be on record (in
    // `have-local-offer`) before this loop reads the next signaling
    // message, or a crossing offer from the same peer is answered
    // without the glare check and both sides end up answering.
    // ...
            if let Err(e) = webrtc_peer::initiate_connection(
                local_peer_id, pid, sig_tx.clone(), mesh.clone(), to_agent_tx.clone(),
            )
            .await
            { /* log */ }
```

A single task reads the WebSocket, so it is now also the point where signaling is serialized. When the loop reads B's offer, any offer A started from a `peer_list` is already in `have-local-offer`, and the glare check sees it. For offers started from the signaling loop, this gives the same guarantee as MDN's `makingOffer` flag. ADR-025 also changed the polite side to close its abandoned offer only after the replacement is on record, which Section 4 comes back to.

After the change, **25 of 25** forced-glare runs passed. The new unit test `crossing_offers_resolve_to_exactly_one_negotiation` fails on the old code. It feeds crossing `peer_list` and `offer` messages to two `MeshManager`s through `handle_signaling_message`, and checks that exactly one answer comes back and that it applies to the lower peer's offer.

The fix has a cost and a limit:

- **Cost.** Offers to several peers in one `peer_list` are now created one after another. Each is local work (create offer, set local description, send), so ADR-025 judges the delay small.
- **Limit.** The IPC `connect` command in `sidecar/src/ipc.rs` still spawns `initiate_connection`. An offer started by the agent does not go through the signaling loop, so the old window still exists for that path. ADR-025 lists it as open.

### In chatixia-mesh

| Piece | Location |
|-------|----------|
| Collision detection | `sidecar/src/signaling.rs`, `"offer"` arm: `mesh.get_pc` plus `signaling_state() == HaveLocalOffer` |
| Tie-break rule | `sidecar/src/signaling.rs`, `glare_keep_local_offer` |
| Our offer on record before the next read | `"peer_list"` arm awaits `initiate_connection`; `"offer"` arm awaits `handle_offer` (ADR-025) |
| Yield (polite side) | Same arm: `webrtc_peer::handle_offer`, then `existing.close()` in a spawned task |
| Keep (impolite side) | Same arm: early `return`; the answer is later applied in the `"answer"` arm |
| Tests | `glare_exactly_one_side_keeps_its_offer`, `glare_rule_is_antisymmetric_for_any_distinct_ids`, `crossing_offers_resolve_to_exactly_one_negotiation`; integration: `test_simultaneous_dial_glare` |
| Log lines | `[SIG] offer glare with <peer>: keeping our offer (lower peer_id wins)` / `... yielding to their offer` |

---

## 3. Candidates That Arrive Too Early

### Trickle ICE recap

In Lesson 03, step 8 of the connection lifecycle said ICE candidates are "trickled in both directions". Trickle ICE (RFC 8838) sends each candidate as soon as the local ICE agent discovers it, instead of waiting for gathering to finish and embedding them all in the SDP. It saves seconds of setup, but it means candidates and descriptions travel as separate messages, and the receiver has to handle them in any order.

A remote candidate is only meaningful once the connection knows the remote side's ICE credentials, which arrive in the remote description. webrtc-rs enforces this directly: `RTCPeerConnection::add_ice_candidate` returns `Error::ErrNoRemoteDescription` if no remote description is set.

### Where the race came from

When ADR-021 was written, the arms of `handle_signaling_message` did not all run the same way:

- `"offer"` spawned `webrtc_peer::handle_offer` in a separate task. That task creates the connection, wires its callbacks, calls `set_remote_description`, and only then calls `mesh.add_peer`.
- `"ice_candidate"` ran inline in the WebSocket read loop.

So the read loop could process the candidates that follow an offer before the spawned task had stored an answering connection. Before ADR-021, the candidate arm was essentially "if there is a connection for this peer, add the candidate" (Lesson 05 showed this shape before it was updated). A candidate with no connection was skipped. A candidate for a connection with no remote description failed in `add_ice_candidate` and was only logged. Either way it was gone.

```
  Offerer (A)                   Registry                  Answerer (B), pre-ADR-021 code
      |-- offer ----------------->|--------------------------->| spawn handle_offer
      |                           |                            |   create_peer_connection ...
      |-- ice_candidate (host) -->|--------------------------->| read loop: get_pc(A) = None
      |                           |                            |   -> DROPPED
      |-- ice_candidate (srflx) ->|--------------------------->|   -> DROPPED
      |                           |                            |   set_remote_description
      |                           |                            |   add_peer(A)
      |-- ice_candidate (relay) ->|--------------------------->| applied
```

Since ADR-025, `handle_offer` is awaited, so the loop does not read the candidates that follow an offer until the answering connection is on record with its remote description. That removes this particular ordering, but not the need for a buffer. Candidates can still arrive early in three ways:

1. **Before the offer.** `initiate_connection` calls `set_local_description(offer)`, and in webrtc-rs that call starts ICE gathering. Only after it returns does the code send the offer. A candidate found in between can be put on the same outbound channel ahead of the offer, so the answerer sees it when it has no connection for that peer yet.
2. **Before the answer.** `handle_offer` has the same shape: `set_local_description(answer)` starts gathering before the answer is sent. So the offerer can receive a candidate while its connection is still in `have-local-offer` with no remote description.
3. **During glare.** The impolite side's offering connection has no remote description until the polite side's answer arrives. Every candidate that arrives before then has to wait.

### Why losing one candidate cost 30 seconds

On one machine the lost candidate was the host candidate, leaving only a server-reflexive (srflx) one: the router's public address for that socket, learned via STUN. For two sidecars on the same LAN, using the srflx candidate means sending a packet to your own router's public address and expecting it to come back inside. That is called **hairpinning** (NAT loopback), and many consumer routers do not support it. With the host candidate gone, no pair worked.

The ICE agent then gives up on its own schedule. In webrtc-ice 0.17.2, the version in `Cargo.lock`, an agent that stays in the checking state longer than the disconnected timeout plus the failed timeout (5 s + 25 s by default, in `agent_config.rs`) moves to `Failed`. That is the 30 seconds in ADR-021.

### The fix: hold, then flush

`MeshManager` in `sidecar/src/mesh.rs` gained a per-peer holding area:

```rust
/// Remote ICE candidates that arrived before their connection had a remote
/// description (trickle ICE races the offer/answer). Applied on flush.
pending_candidates: DashMap<String, Vec<RTCIceCandidateInit>>,
```

The candidate arm now applies a candidate only when the connection can accept it:

```rust
// Trickle ICE races the offer/answer: a candidate can arrive
// before its connection exists or has a remote description.
// Hold it and apply it once the description is set.
match mesh.get_pc(&from_peer) {
    Some(pc) if pc.remote_description().await.is_some() => {
        if let Err(e) = pc.add_ice_candidate(init).await {
            warn!("[ICE] failed to add candidate from {}: {}", from_peer, e);
        }
    }
    _ => {
        mesh.queue_candidate(&from_peer, init);
        // The description may have landed while we queued.
        mesh.flush_candidates(&from_peer).await;
    }
}
```

`flush_candidates` does nothing unless the connection now has a remote description. It then takes the whole queue and applies it:

```rust
pub async fn flush_candidates(&self, peer_id: &str) {
    let Some(pc) = self.get_pc(peer_id) else {
        return;
    };
    if pc.remote_description().await.is_none() {
        return;
    }
    let held = self.take_candidates(peer_id);
    // ... add_ice_candidate for each, logging "[ICE] applying N early candidate(s)"
}
```

`take_candidates` removes the entry with a single `DashMap::remove`, so two concurrent flushes cannot apply the same candidate twice. The comment calls this "atomic, so a double flush is harmless".

There are three flush points, one for each place a remote description can appear or a candidate can arrive:

| Flush point | Why it is needed |
|-------------|------------------|
| `webrtc_peer::handle_offer`, right after `mesh.add_peer` | The answerer's connection now exists and has the offer as its remote description |
| `"answer"` arm, after `set_remote_description` succeeds | The offerer's connection just left `have-local-offer` |
| `"ice_candidate"` arm, right after queuing | The description may have been set between the check and the queue |

The third one closes a gap that is easy to miss. The check (`remote_description().await.is_some()`) and the queue are two separate steps. Without a flush after queuing, a candidate that loses that race would wait in the queue until the next flush, which may never come.

The queue is cleared in `MeshManager::clear_all_peers`, which `signaling::run` calls before every signaling reconnect. That way candidates from an old session never leak into a new one.

With the current code, a candidate that overtakes its offer is handled like this:

```
  Offerer (A)                   Registry                  Answerer (B)
  set_local_description(offer)    |                            |
  (gathering starts)              |                            |
      |-- ice_candidate (host) -->|--------------------------->| get_pc(A) = None -> queue [host]
      |-- offer ----------------->|--------------------------->| await handle_offer:
      |                           |                            |   set_remote_description
      |                           |                            |   add_peer(A)
      |                           |                            |   flush -> apply host
      |<-- answer ----------------|<---------------------------|
      |-- ice_candidate (srflx) ->|--------------------------->| remote description set -> apply
```

### Trade-offs the buffer accepts

- **The queue is keyed by `peer_id`, not by connection.** The sidecar's `ice_candidate` payload carries `candidate`, `sdpMid` and `sdpMLineIndex` only (`username_fragment` is set to `None`), so it cannot tell which ICE session a candidate belongs to. During glare, the impolite side queues candidates from the polite side's abandoned offer (pcB1 in the Section 2 diagram) and later flushes them onto its surviving connection. They point at sockets that close with pcB1, so their checks should simply fail: wasted work, not a correctness bug. That is a hypothesis; no test checks it. MDN's pattern avoids the question by ignoring candidates while an offer is being ignored. ADR-025 lists per-peer keying as still open.
- **The queue is unbounded.** It is emptied on flush or on signaling reconnect. A peer that sends candidates but never completes a handshake leaves an entry behind until then. This is the same class as gap G3 ("unbounded in-memory growth") in `docs/THREAT_MODEL.md`, which currently covers only the registry.

### In chatixia-mesh

| Piece | Location |
|-------|----------|
| Holding area | `MeshManager::pending_candidates` in `sidecar/src/mesh.rs` |
| Queue / take / flush | `queue_candidate`, `take_candidates`, `flush_candidates` |
| Apply-or-queue decision | `"ice_candidate"` arm in `sidecar/src/signaling.rs` |
| Flush after description | `webrtc_peer::handle_offer` (after `add_peer`), `"answer"` arm |
| Reset | `MeshManager::clear_all_peers`, called from `signaling::run` |
| Log line | `[ICE] applying N early candidate(s) from <peer>` |

---

## 4. A Replaced Connection Must Not Remove Its Replacement

The glare fix introduced a new bug of its own, and ADR-021 fixed it in the same decision.

Every connection gets a state-change callback in `setup_ice_forwarding`. When the state reaches an end state, the callback removes the peer from `MeshManager` and tells the Python agent `peer_disconnected`. The callback identifies the peer by its `peer_id` string, captured when the callback was created.

Now follow the polite side of a glare collision as ADR-021 first wrote it. It called `existing.close()` on its offering connection pcB1 in a spawned task, and `handle_offer` stored the answering connection pcB2 under the same `peer_id`. Closing pcB1 fires its state callback with `Closed`. A callback that runs `remove_peer("world-the-alpha")` removes whatever is stored under that name, and by then that could be pcB2, the healthy replacement:

```
  Polite side (B), connections to peer A, name-based removal
      |
      |  peers["A"] = pcB1 (offering)
      |
      |  glare: close pcB1;  handle_offer
      |
      |  handle_offer: peers["A"] = pcB2 (answering)      <-- replacement stored
      |
      |  pcB1 finishes closing -> state callback(Closed)
      |     remove_peer("A")                             <-- removes pcB2!
      |     IPC peer_disconnected("A")                   <-- agent told a lie
      |
      |  pcB2's DataChannel opens, but the peer is no longer on record
```

This is the **stale callback** problem, and it appears whenever a long-lived callback refers to something by name while that name can be rebound. Late timers, retries and old socket handlers all fall into it. The general fix is "compare and delete": only remove the entry if it is still the object you think it is.

`MeshManager` does this with pointer identity:

```rust
/// Remove a peer only if `pc` is still its current connection.
///
/// A connection replaced during offer glare fires its own Closed/Failed
/// callback later; this keeps that stale callback from tearing down the
/// connection that replaced it. Returns true if the peer was removed.
pub fn remove_peer_if_pc(&self, peer_id: &str, pc: *const RTCPeerConnection) -> bool {
    let is_current = self
        .peers
        .get(peer_id)
        .is_some_and(|p| std::ptr::eq(Arc::as_ptr(&p.pc), pc));
    if is_current {
        self.remove_peer(peer_id);
    }
    is_current
}
```

The state callback in `sidecar/src/webrtc_peer.rs` passes its own connection's address:

```rust
// Weak, so the callback doesn't keep its own connection alive.
let pc_weak = Arc::downgrade(pc);
pc.on_peer_connection_state_change(Box::new(move |state: RTCPeerConnectionState| {
    // ...
    let ended = matches!(
        state,
        RTCPeerConnectionState::Failed
            | RTCPeerConnectionState::Disconnected
            | RTCPeerConnectionState::Closed
    );
    // Ignore connections already replaced (e.g. after offer glare).
    if ended && mesh.remove_peer_if_pc(&rpid, Weak::as_ptr(&pc_weak)) {
        let _ = to_agent_tx.send(IpcMessage { /* peer_disconnected */ });
        // ... re-dial (Section 5)
    }
    Box::pin(async {})
}));
```

Three details make this work:

1. **`Weak`, not `Arc`.** The callback is owned by the connection. A strong reference back to that connection would be a reference cycle, and the connection would never be freed.
2. **`Weak::as_ptr` is a safe identity.** A `Weak` keeps the allocation alive even after the connection itself is dropped, so its address cannot be reused by a newer connection while the callback exists. Two different connections can never compare equal by accident.
3. **The DataChannel has the same guard.** `on_close` in `setup_datachannel_handler` calls `MeshManager::remove_peer_if_channel` with the channel's own `Weak` pointer, comparing against the `channels` map.

The return value matters too. `peer_disconnected` and the re-dial only happen when the removal actually happened, so a stale callback does nothing that anyone can see.

Under ADR-021, whether pcB1's `Closed` fired before or after `handle_offer` stored pcB2 was a race between two spawned tasks. If it fired first, pcB1 was still on record, so the identity check passed. The peer was removed and the agent got a `peer_disconnected` for a peer it had never been told was connected. ADR-025 removed that ordering. The `"offer"` arm now awaits `handle_offer` and only then spawns `existing.close()`, so pcB1 is always stale by the time its callback runs. The identity check still matters, because it is what turns "always stale" into "does nothing". Exercise 3 asks you to trace both versions.

### In chatixia-mesh

| Piece | Location |
|-------|----------|
| Connection-identity removal | `MeshManager::remove_peer_if_pc` |
| Channel-identity removal | `MeshManager::remove_peer_if_channel` |
| Callers | `setup_ice_forwarding` (state change), `setup_datachannel_handler` (`on_close`) |
| Unconditional removal (still used) | `MeshManager::remove_peer`, called by the two guarded versions |
| Close after replacement | `"offer"` arm in `sidecar/src/signaling.rs`: `existing.close()` spawned after `handle_offer` returns (ADR-025) |

---

## 5. When the Path Dies but Signaling Lives

### How ICE notices a dead path

Once a connection is up, the registry is out of the data path (Lesson 01's control plane / data plane split). So how does a sidecar find out that the other end has gone quiet?

ICE keeps checking. RFC 7675, **ICE consent freshness**, requires an agent to keep sending STUN binding requests on the selected candidate pair. If responses stop for long enough, the agent must treat consent as lost and stop sending. The RFC's default is 30 seconds without a response. The goal is partly security (never keep blasting packets at a host that no longer agrees to receive them) and partly liveness detection.

webrtc-ice 0.17.2 implements this with its own timers (`src/agent/agent_config.rs`):

| Constant | Default | Effect |
|----------|---------|--------|
| `DEFAULT_KEEPALIVE_INTERVAL` | 2 s | Binding request sent on the selected pair |
| `DEFAULT_DISCONNECTED_TIMEOUT` | 5 s | Nothing received for 5 s: ICE state `Disconnected` |
| `DEFAULT_FAILED_TIMEOUT` | 25 s | Nothing received for 5 + 25 = 30 s: ICE state `Failed` |

`Disconnected` can recover. If traffic resumes, the agent returns to `Connected`. `Failed` is final for that ICE session. The peer connection state follows the ICE state (a DTLS failure also maps to `Failed`, and `Closed` is set only by a local `close()` call), so the sidecar's callback sees:

```
   new --> connecting --> connected <----------+
                |             |                |  traffic resumes
                |             | 5 s silent     |
                |             v                |
                |        disconnected ---------+
                |             |
                | 30 s in     | 30 s silent in total
                | checking    v
                +--------> failed

   any state --close()--> closed
```

The sidecar never changes these defaults. webrtc-rs exposes them through `SettingEngine::set_ice_timeouts`, but `create_peer_connection` in `webrtc_peer.rs` does not use a `SettingEngine`.

### The incident

ADR-022 records what happened during the first "keeper whisper" test. About five minutes after the last DataChannel message, both sidecars' connections went to `Failed` at the same instant. The laptop had most likely dozed, and an Ollama request in flight at the same moment also hung for 299 s. Both sidecars removed the peer and reported `peer_disconnected`, and then nothing happened for the rest of the session.

The sidecar had exactly one recovery path, in `signaling::run`: when the WebSocket drops, call `mesh.clear_all_peers()`, reconnect, send `register`, get a `peer_list`, offer again. But the WebSocket never dropped. The two sidecars and the registry were on the same laptop, so the control plane stayed healthy while the data plane had died. No code was waiting for that combination.

On one laptop, the cost is restarting the demo. On a Raspberry Pi over wifi, ICE consent timeouts are the normal failure, and chatixia-world's cross-NAT run has "no sidecar restart" as a pass condition.

### The re-dial path

The fix is in the same state callback from Section 4:

```rust
// The signaling socket is usually still up when a peer connection
// dies on its own (laptop sleep, wifi drop, ICE consent timeout),
// so nothing else would ever try again. Re-registering makes the
// registry send a fresh peer_list, which re-runs the normal offer
// path, glare tie-break included. Closed is skipped: that is us
// closing a connection on purpose (glare yield, shutdown).
if should_redial(state) {
    let mesh = mesh.clone();
    let local = local_for_redial.clone();
    let rpid = rpid.clone();
    tokio::spawn(async move {
        tokio::time::sleep(REDIAL_DELAY).await;
        if mesh.is_connected(&rpid) {
            return;
        }
        match mesh.signaling_tx() {
            Some(tx) => {
                info!("[SIG] re-dialing {} after connection failure", rpid);
                let register = SignalingMessage {
                    msg_type: "register".into(),
                    peer_id: local,
                    target_id: None,
                    payload: serde_json::Value::Null,
                };
                let _ = tx.send(serde_json::to_string(&register).unwrap());
            }
            None => warn!("[SIG] cannot re-dial {}: signaling is down", rpid),
        }
    });
}
```

With its supporting pieces:

```rust
/// How long to wait after a peer connection fails before asking the registry
/// for peers again. Long enough for the other side to notice too, short enough
/// that a wifi blip costs seconds, not a restart.
const REDIAL_DELAY: std::time::Duration = std::time::Duration::from_secs(3);

/// A peer connection that failed or dropped on its own is worth re-dialing;
/// one we closed deliberately is not.
fn should_redial(state: RTCPeerConnectionState) -> bool {
    matches!(
        state,
        RTCPeerConnectionState::Failed | RTCPeerConnectionState::Disconnected
    )
}
```

The callback needs a way to send on the current signaling session, and before ADR-022 it had none. `MeshManager` now holds `signaling_tx: std::sync::Mutex<Option<mpsc::UnboundedSender<String>>>`. `connect_once` in `sidecar/src/signaling.rs` sets it with `set_signaling_tx` when a WebSocket session starts, and `run` clears it with `clear_signaling_tx` when the session ends. If signaling is down, the re-dial just logs a warning, since the signaling reconnect path will redo everything anyway.

A full recovery with both sides detecting the failure together, as in the doze incident:

```
  A = world-the-alpha              Registry              B = world-the-beta
      |<========== DataChannel open; both WebSockets open ==========>|
      |                               |                               |
      |    ... path dies: consent checks go unanswered ...            |
      |                               |                               |
  state -> Failed/Disconnected        |       state -> Failed/Disconnected
  remove_peer_if_pc -> true           |       remove_peer_if_pc -> true
  IPC peer_disconnected               |       IPC peer_disconnected
  sleep(REDIAL_DELAY = 3 s)           |       sleep(REDIAL_DELAY = 3 s)
      |                               |                               |
  !is_connected(B)                    |       !is_connected(A)
      |-- register ------------------>|<------------------ register --|
      |<-- peer_list ["B"] -----------|----------- peer_list ["A"] -->|
      |                               |                               |
  initiate_connection -> pcA          |       initiate_connection -> pcB
      |-- offer --------------------->|------------------------------>|
      |<------------------------------|<------------------------ offer|
  glare: "alpha" < "beta", keep       |       glare: yield, close pcB,
      |                               |       answer with pcB'
      |<------------------------------|<------------------- answer ---|
      |<========== new DataChannel "mesh" open; IPC peer_connected ====>|
```

### Why this design

- **Reuse the normal path.** Re-dialing sends `register`, not a direct offer. The registry replies with an up-to-date `peer_list`, so if the other sidecar has really gone, its WebSocket is gone too, it is not listed, and no offer is made. The re-dial needs no new message type and no registry change.
- **Glare on purpose.** Both sides re-dial after the same delay, so they usually offer at the same time. That is fine, because ADR-021's tie-break already decides who offers. ADR-022 depends on ADR-021.
- **Why wait 3 seconds.** The code comment gives the reason: it is long enough for the other side to notice too, and short enough that a wifi blip costs seconds rather than a restart. The `is_connected` check after the sleep skips the re-dial if the other side's offer already produced an open channel in the meantime.
- **Why skip `Closed`.** `Closed` only happens when this sidecar calls `close()`: on a glare yield, or when `clear_all_peers` runs before a signaling reconnect. Re-dialing there would fight the replacement connection.

### Trade-offs

- **`Disconnected` is treated as final.** ICE can recover from `Disconnected`, but the sidecar removes the peer as soon as it gets there, which is after 5 s of silence. A short blip that ICE would have ridden out now costs a full new handshake. The sidecar also does not call `close()` on the connection it stops tracking (`remove_peer` only drops map entries), so whatever happens to that connection afterwards is up to webrtc-rs. ADR-025 lists both halves of this as still open.
- **One `register` per failure.** Each failure of the connection on record triggers exactly one re-register. The re-dial offers to every peer in the new `peer_list` that is not connected, not only the one that failed. If the new attempt also fails, its own callback triggers the next re-dial, so retries go on only while attempts are being made and failing. If the remote sidecar is gone for good, the registry stops listing it and the retries stop. (ADR-022 says a returning peer shows up "via the registry's `peer_joined` path", but the registry has no `peer_joined` message. What actually happens is that the returning sidecar sends its own `register`, finds us in its `peer_list`, and offers.)
- **Recovery time.** ADR-022 reports about 3 s plus a handshake, measured from the moment failure is detected. On top of that comes the detection time itself: 5 s for `Disconnected`, or 30 s if the connection jumps straight to `Failed`.

### In chatixia-mesh

| Piece | Location |
|-------|----------|
| State handler and re-dial | `setup_ice_forwarding` in `sidecar/src/webrtc_peer.rs` |
| Which states re-dial | `should_redial` (tested by `test_should_redial_only_on_involuntary_end`) |
| Delay | `REDIAL_DELAY` (3 s) |
| Live signaling sender | `MeshManager::signaling_tx`, set in `connect_once`, cleared in `run` |
| Registry side | Unchanged: `register` handled by `SignalingState::handle_message` |
| Log lines | `[SIG] re-dialing <peer> after connection failure`, `[SIG] cannot re-dial <peer>: signaling is down` |
| Integration test | `test_redial_after_ice_consent_timeout` in `tests/integration/test_two_sidecars.py` |

---

## 6. How the Bugs Were Found, and Why Unit Tests Missed Them

Every bug in this lesson was found by running the real thing: two sidecars, one registry, real WebSockets, real ICE. The first four came from running chatixia-world by hand: ADR-021's `scripts/mesh-demo.sh` run and ADR-022's keeper-whisper session. The last one, the glare race in Section 2, came from an automated version of the same setup. The third failure from ADR-021 is the clearest example of why running the real thing matters.

**The rustls provider panic.** `reqwest` 0.13 turns on rustls with the `aws-lc-rs` crypto backend. webrtc's DTLS stack turns it on with `ring`. With both compiled in, rustls cannot pick a process-wide default and panics on first use, which is the first DTLS handshake, right after ICE connects. The fix is a single line at the top of `main()` in `sidecar/src/main.rs`:

```rust
let _ = rustls::crypto::ring::default_provider().install_default();
```

ADR-021 notes that the lockfile has had this combination since the reqwest 0.13 bump in March 2026, so the sidecar most likely could not open a DataChannel for about six months. All that time the test suite passed.

Here is why each bug was out of reach for the unit tests that existed when it was found:

| Bug | What it takes to trigger | Why unit tests could not see it |
|-----|--------------------------|----------------------------------|
| Offer glare | Two processes registering within milliseconds of each other | Tests used one `MeshManager` and never ran an offer/answer between two sidecars |
| Early candidates | A spawned task losing a race to the WebSocket read loop | Needs a real relay; no test called `handle_signaling_message` |
| Stale removal | A glare yield followed by a late `Closed` callback | Only happens after glare, which never happened in tests |
| rustls panic | A DTLS handshake | `mesh.rs` tests create `RTCPeerConnection`s but never connect two of them |
| No re-dial | 5-30 s of ICE silence while the WebSocket stays up | Needs a live connection and a way to kill its path, not its process |
| Glare race (ADR-025) | An offer read before our own spawned offer reached `have-local-offer` | The ADR-021 unit tests checked the rule, never the moment it runs; five manual runs happened to miss it |

This is Lesson 17's point about seams. The heartbeat bug there sat between a registry response and a runner that ignored it. These bugs sit between two copies of the same program, where each copy is correct on its own and the failure comes from how their timing combines.

### Decisions vs wiring

ADR-021 and ADR-022 came with three unit tests:

- `glare_exactly_one_side_keeps_its_offer` and `glare_rule_is_antisymmetric_for_any_distinct_ids` in `sidecar/src/signaling.rs`
- `test_should_redial_only_on_involuntary_end` in `sidecar/src/webrtc_peer.rs`

Look at what they test. Each one checks a **decision**, pulled out into a small pure function: who keeps the offer, which states re-dial. None of them checks the **wiring**: that the decision is consulted at the right moment, that the flush happens after `add_peer`, or that a stale callback is really ignored. Pure decisions are cheap to test exhaustively. The ADR-025 bug shows what that leaves out. The tie-break rule was correct, and was sometimes never consulted.

ADR-025's unit test, `crossing_offers_resolve_to_exactly_one_negotiation`, sits in between. It runs the real `handle_signaling_message` against two `MeshManager`s with real `RTCPeerConnection`s, but passes messages by hand with no WebSocket, registry or network. That makes it deterministic enough for a unit test while still exercising the wiring: that our offer is on record when the handler returns, and that exactly one answer comes back. It could only be written once the integration test had shown which ordering to reproduce.

### From manual runs to an integration test

The evidence behind ADR-021 and ADR-022 was manual:

- **ADR-021:** five simultaneous restarts of both sidecars on one machine. Each time the DataChannel came back in 4-5 s, mostly the 3 s respawn delay. Glare and early candidates each happened during that run and were resolved. It was also the first end-to-end LLM dialogue across two sidecars, with 1-3 ms one-hop DataChannel latency on localhost.
- **ADR-022:** one sidecar paused with `SIGSTOP` past the ICE consent timeout, then resumed with `SIGCONT`. Pausing the process freezes it without closing its sockets, so the registry still sees a live WebSocket, which is the exact shape of the doze incident.

The two-sidecar integration test in `tests/integration/` turned both into repeatable runs, and in doing so found the glare race described in Section 2. Five manual passes had looked convincing. Under forced conditions the old code failed 4 runs out of 12. With a failure that intermittent, five passes by hand is weak evidence that nothing is wrong. A test that forces the dangerous ordering and runs it many times is what exposed it.

When you run the mesh yourself, these are the log lines that show which path ran:

```
[SIG] offer glare with world-the-beta: keeping our offer (lower peer_id wins)
[SIG] offer glare with world-the-alpha: yielding to their offer
[ICE] applying 2 early candidate(s) from world-the-alpha
[SIG] re-dialing world-the-beta after connection failure
[ICE] world-the-beta selected pair: local=host 192.168.1.20:50412 remote=host 192.168.1.20:61203
```

(The formats come from the code. The peer IDs, counts and addresses are illustrative.)

---

## 7. What the Integration Test Covers, and What Is Still Open

### The two-sidecar integration test

`tests/integration/` runs a real `chatixia-registry` and two real `chatixia-sidecar` processes on localhost:

| File | Role |
|------|------|
| `harness.py` | Starts the processes, plays a minimal agent on each sidecar's IPC socket (`FakeAgent`), and provides `WsGate`, a TCP proxy that holds both sidecars' WebSocket upgrades and releases them together |
| `test_two_sidecars.py` | The three scenarios below |
| `conftest.py` | Locates the binaries and kills any process a failed test leaves behind |

The sidecars use peer IDs `itest-a` and `itest-b` with API keys in a generated `api_keys.json`, so they pass ADR-024's approval checks as legacy peers. ICE runs natively with host candidates, and there is no Docker or TURN. The harness uses only the Python standard library.

| Scenario | What it does | What it asserts |
|----------|--------------|-----------------|
| `test_connect_and_exchange_messages` | Starts A, waits for its `register`, then starts B | Both agents get `peer_connected`; an `agent_prompt` arrives each way over the DataChannel; the `selected pair` log line shows a direct pair (`host`, `srflx` or `prflx`), never a relay |
| `test_redial_after_ice_consent_timeout` (ADR-022) | Connects, then `SIGSTOP`s B for 36 s (the 30 s consent failure plus a margin) and resumes it | A reports `peer_disconnected`, logs `re-dialing itest-b after connection failure` while B is frozen, and its old connection reaches `failed`. After `SIGCONT` the link heals within 45 s, with messages flowing both ways, no stale `peer_disconnected` after the last `peer_connected`, and no signaling reconnect in either sidecar's log |
| `test_simultaneous_dial_glare` (ADR-021, ADR-025) | Starts both sidecars at once behind `WsGate`, so each `register` gets a `peer_list` naming the other | Both sidecars log `initiating connection`, A logs `keeping our offer`, B logs `yielding to their offer`, and the link is healthy afterwards |

To run it from the repository root:

```bash
uvx pytest tests/integration -v
```

Without `CHATIXIA_BIN_DIR`, the harness first runs `cargo build -p chatixia-registry -p chatixia-sidecar` (debug), so it never tests stale binaries. In CI, the `integration` job in `.github/workflows/ci.yml` builds the binaries in a separate step, runs the test with `CHATIXIA_BIN_DIR=target/debug`, and uploads the process logs as an artifact if anything fails (`CHATIXIA_ITEST_LOG_DIR`). The re-dial scenario takes at least 36 s by design, because it waits out the real ICE timeouts.

### What is still open

The test covers the paths ADR-021, ADR-022 and ADR-025 fixed. ADR-025 lists what it does not cover and what is known to be open. Each item below is a real gap in the code. None has yet been shown to cause a failure:

| Open item (ADR-025) | Where | What could happen |
|---------------------|-------|-------------------|
| IPC `connect` still spawns its offer | `sidecar/src/ipc.rs` | An agent-started offer is not on record before the signaling loop reads a crossing offer, so the pre-ADR-025 glare window still exists on this path (Section 2) |
| A later `peer_list` can start a second offer mid-handshake | `"peer_list"` arm checks `is_connected`, which is only true once a DataChannel opens | A re-register during a handshake starts a second offer to the same peer and replaces the connection in progress |
| Candidates are keyed by peer, not connection | `MeshManager::pending_candidates` | Candidates from an abandoned glare offer are flushed onto the surviving connection (Section 3) |
| `Disconnected` connections are removed but never closed | State callback and `MeshManager::remove_peer` | Needless handshakes on short blips, and untracked connections left to webrtc-rs (Section 5) |

Some consequences in that table are still hypotheses: that stale candidates only waste checks, and that a second mid-handshake offer actually breaks a link rather than just replacing it. So is one more item that ADR-025 does not mention. The registry's `handle_ws` still removes a peer from the signaling map by name when its socket closes (`state.signaling.remove_peer(&peer_id)`). That has the same shape as Section 4's stale-removal bug, on the registry side (Exercise 4). ADR-024 changed who may relay to whom, but not this cleanup.

Scenarios the integration test does not run yet:

- **Early candidates on purpose.** Candidates overtaking their description do happen during the scenarios, but no scenario forces that ordering or asserts on `applying N early candidate(s)`.
- **Signaling loss.** Nothing restarts the registry or drops a WebSocket mid-session, so the `signaling::run` reconnect path (`clear_all_peers`, then register again) is still verified only by hand.
- **Real NATs.** Everything runs on localhost. ADR-021 lists the two-machine cross-NAT run as outstanding, and no later ADR records it as done. `docs/DEPLOYMENT_GUIDE.md` ("chatixia-world across two NATs") points to the world's runbook and to the evidence to look for: the `[ICE] <peer> selected pair` line, where `srflx`, `prflx` or `relay` means a real NAT crossing and `host` means same LAN. Across NATs, relay latency and wifi make every race in this lesson more likely, not less.

---

## Exercises

### Exercise 1: The window before `have-local-offer`

Section 2 showed the race ADR-025 fixed for offers started from `peer_list`. The IPC `connect` path in `sidecar/src/ipc.rs` still spawns `initiate_connection`.

1. Suppose A's agent sends `connect` for B at the same moment B's sidecar reads a `peer_list` naming A. List the points inside `webrtc_peer::initiate_connection` (`create_peer_connection`, `setup_ice_forwarding`, `create_data_channel`, `mesh.add_peer`, `create_offer`, `set_local_description`, send) where B's offer can reach A's signaling loop. For each one, what does the `"offer"` arm on A see from `mesh.get_pc` and `signaling_state()`, and which branch runs?
2. Pick the worst case and follow both sidecars until the handshake either succeeds or both connections reach a final state. Which log lines appear? Does the re-dial path from Section 5 eventually recover?
3. Propose a fix for the IPC path. Compare at least two options: route `connect` through the signaling loop (for example, by sending it a command on a channel) so ADR-025's serialization covers it, or keep a `making_offer` set in `MeshManager` as MDN does. For each, explain what new ordering problem it could create. (Hint: `set_local_description` starts candidate gathering, and `on_ice_candidate` sends right away.)
4. Extend `test_simultaneous_dial_glare`, or write a new scenario, that would fail on the current IPC path. What would you use to make the timing reliable?

### Exercise 2: Why not let only the lower ID offer?

A colleague proposes removing glare completely: on `peer_list`, a sidecar offers only to peers whose `peer_id` is higher than its own, and the others wait for an offer.

1. Which of the three offer triggers from Section 1 does this break or complicate? Think about IPC `connect` from the agent with the higher ID, and about re-dial when only the higher-ID side notices the failure.
2. Would the glare check in the `"offer"` arm still be needed? Why or why not?
3. Write a short ADR-style Decision and Consequences section for either keeping the current design or switching. Include at least one (+) and one (-) for each.

### Exercise 3: Trace the stale `Closed` callback

Under ADR-021, the polite side ran `existing.close()` and `handle_offer` in two separately spawned tasks. Under ADR-025, it awaits `handle_offer` and only then spawns `existing.close()`.

1. ADR-021, order 1: pcB1's `Closed` callback runs **before** `handle_offer` calls `mesh.add_peer`. What does `remove_peer_if_pc` return? Does the agent get `peer_disconnected`? Does a re-dial happen? What does `handle_offer`'s `add_peer` then do?
2. ADR-021, order 2: the callback runs **after** `add_peer`. Answer the same questions.
3. Show from the current `"offer"` arm in `sidecar/src/signaling.rs` that only order 2 can happen now. Which assertion in `test_simultaneous_dial_glare` would fail if order 1 came back? (Hint: look at `assert_healthy_link` and `last_link_event`.)
4. Given that the order is now fixed, is `remove_peer_if_pc` still needed on this path? Name another path in this lesson where a replaced connection's callback can still fire late.
5. Explain in two sentences why comparing `Weak::as_ptr` values cannot give a false match, even if the old connection has already been dropped.

### Exercise 4: The same bug on the registry side

`handle_ws` in `registry/src/main.rs` calls `state.signaling.add_peer(&peer_id, tx)` on connect and `state.signaling.remove_peer(&peer_id)` when its loop ends.

1. Describe a sequence where a sidecar's new WebSocket is registered before the registry notices its old one has closed. (Hint: think about a half-open TCP connection after a wifi drop, and how quickly `signaling::run` reconnects after a session ends.) What does the old socket's cleanup do to the new session?
2. What does the sidecar observe afterwards? Would the re-dial path from Section 5 help?
3. Write `SignalingState::remove_peer_if_sender(&self, peer_id: &str, tx: &mpsc::UnboundedSender<String>) -> bool` using `UnboundedSender::same_channel`, and show the change to `handle_ws`. Write a unit test in the style of the existing ones in `registry/src/signaling.rs` that fails without your fix.

### Exercise 5: Extend the two-sidecar integration test

`tests/integration/test_two_sidecars.py` covers connect, re-dial and forced glare. Design (and, if you like, implement) the scenarios it is missing.

1. **Early candidates.** Add a mode to `WsGate` (or a second proxy) that delays one sidecar's `offer` by a few hundred milliseconds while passing its `ice_candidate` messages straight through. What should the test assert: the `applying N early candidate(s)` log line, a working DataChannel, or both? Why is a working DataChannel alone not enough?
2. **Signaling loss.** Restart the registry mid-session. What should happen to the DataChannel (Lesson 05 says established channels survive registry downtime), and what does `signaling::run` do on reconnect? Write the assertions.
3. **Faster consent tests.** The re-dial scenario waits at least 36 s because the ICE defaults are real. `SettingEngine::set_ice_timeouts` exists in webrtc-rs, but `create_peer_connection` does not use a `SettingEngine`. Sketch a test-only environment variable that shortens the timeouts, and argue whether it belongs in production code.
4. Which of your scenarios can run in the CI `integration` job on one machine, and which need two networks?

### Exercise 6 (open-ended): Should `Disconnected` trigger a re-dial?

The sidecar acts on `Disconnected` (5 s of silence). It could instead wait for `Failed` (30 s) and let ICE recover from short blips by itself. Argue both sides for chatixia-world's target deployment: a Raspberry Pi on home wifi talking to a laptop across NATs, with "no sidecar restart" as the pass condition and a thought log that users watch in real time. Consider recovery time, the cost of a needless handshake, the untracked connection left behind by `remove_peer`, and what the agent sees on the IPC channel in each case. State which you would choose, and what measurement would change your mind.

---

## Summary

The handshake from Lessons 03 and 05 is correct, but it only describes one ordering of events. The first real two-sidecar run found the others:

- **Offer glare.** When both peers offer at once, they have to agree on which offer survives without asking anyone. The sidecar compares `peer_id`s: the lower one keeps its offer (impolite), the higher one closes its connection and answers (polite). The rule works because both sides have the inputs, and it is antisymmetric and stable.
- **Early candidates.** Trickle ICE sends candidates independently of the offer and answer, and `add_ice_candidate` needs a remote description. `MeshManager::pending_candidates` holds early candidates and flushes them at every point where a description can appear.
- **Stale callbacks.** A connection that has been replaced must not remove its replacement. `remove_peer_if_pc` and `remove_peer_if_channel` compare identity, not names.
- **Dead path, live signaling.** ICE liveness checks (RFC 7675 consent freshness; 5 s to `Disconnected` and 30 s to `Failed` in webrtc-ice) can fail a connection while the WebSocket is fine. The sidecar now re-registers after `REDIAL_DELAY`, reusing the normal offer path and ADR-021's tie-break.
- **Finish the offer before reading on.** A correct tie-break is useless if it is never consulted. ADR-025 makes the signaling loop await `initiate_connection` and `handle_offer`, so our offer is always on record before a crossing offer is read.

None of this was visible to unit tests, because each bug is about timing between two copies of the same program. The rule-level unit tests check the decisions. The two-sidecar integration test checks the wiring, and it found the glare race that five manual runs had missed. What remains open is listed in ADR-025: the IPC `connect` path, a second offer during a handshake, candidates keyed by peer, and `Disconnected` connections that are never closed. A real cross-NAT run is also still outstanding.

---

## Related Lessons

- [Lesson 01: Why Distributed Systems](01-why-distributed-systems.md) -- control plane vs data plane; Section 5's incident is the two failing separately
- [Lesson 02: Peer-to-Peer Networking](02-peer-to-peer-networking.md) -- host, srflx and relay candidates; NAT behavior behind the hairpinning failure
- [Lesson 03: WebRTC Fundamentals](03-webrtc-fundamentals.md) -- the connection lifecycle this lesson breaks apart
- [Lesson 05: Signaling Protocol Design](05-signaling-protocol-design.md) -- `register` / `peer_list` / `offer` / `answer` / `ice_candidate`; the sequence this lesson breaks apart, with excerpts updated to the current handlers
- [Lesson 10: The Sidecar Pattern](10-sidecar-pattern.md) -- `MeshManager` and the sidecar's internal structure
- [Lesson 15: Deployment Patterns](15-deployment-patterns.md) -- connectivity tiers and cross-network setup, where these races become more likely
- [Lesson 16: Architecture Decision Records](16-architecture-decision-records.md) -- ADR-021, ADR-022 and ADR-025 are incident-driven ADRs with measured consequences, and ADR-025 corrects ADR-021
- [Lesson 17: Testing Distributed Systems](17-testing-distributed-systems.md) -- seams and the E2E gap; `tests/integration/` is a multi-process take on its Exercise 4 (signaling integration test), and Exercise 5 here extends it

---

## Further Reading

- [RFC 8445](https://datatracker.ietf.org/doc/html/rfc8445) -- ICE. Candidate pairs, connectivity checks, nomination.
- [RFC 8838](https://datatracker.ietf.org/doc/html/rfc8838) -- Trickle ICE: incremental provisioning of candidates. Why candidates and descriptions travel separately.
- [RFC 7675](https://datatracker.ietf.org/doc/html/rfc7675) -- STUN usage for consent freshness. The liveness rule behind `Disconnected` and `Failed`.
- [RFC 9429](https://datatracker.ietf.org/doc/html/rfc9429) -- JSEP. The signaling state machine, offer/answer rules, and rollback.
- [MDN: Establishing a connection -- the WebRTC perfect negotiation pattern](https://developer.mozilla.org/en-US/docs/Web/API/WebRTC_API/Perfect_negotiation) -- polite and impolite peers, and why `makingOffer` is tracked separately from `signalingState`.
- [Perfect negotiation in WebRTC](https://blog.mozilla.org/webrtc/perfect-negotiation-in-webrtc/) -- Jan-Ivar Bruaroey's Mozilla blog post introducing the pattern.
- `docs/ADR.md` -- ADR-021, ADR-022 and ADR-025 in full, including the measurements quoted in this lesson; ADR-024 for the registry's approval-gated relay.
- `tests/integration/harness.py` -- how the integration test forces glare (`WsGate`) and freezes a sidecar past the consent timeout.
- `docs/DEPLOYMENT_GUIDE.md` -- "chatixia-world across two NATs" and the `selected pair` evidence line.
- webrtc-ice 0.17.2 source, `src/agent/agent_config.rs` and `src/agent/agent_internal.rs` -- the keepalive, disconnected and failed timeouts, and the consent binding requests.
