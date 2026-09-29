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
- `docs/ADR.md` -- ADR-021 (glare and early candidates), ADR-022 (re-dial)

---

## What You'll Learn

- Why two peers that dial each other at the same moment can deadlock ("offer glare"), and how a deterministic tie-break on `peer_id` resolves it
- Why trickled ICE candidates can arrive before their connection can accept them, and how a per-peer buffer fixes it
- Why a connection that has been replaced must never be allowed to remove its replacement, and how an identity check enforces that
- How ICE liveness checks detect a dead path while the signaling WebSocket stays up, and how the re-dial path heals it without a process restart
- Why unit tests missed all of this, and what is still untested

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
| A `peer_list` names a peer we are not connected to | `handle_signaling_message`, `"peer_list"` arm, spawns `webrtc_peer::initiate_connection` |
| The Python agent sends an IPC `connect` command | `sidecar/src/ipc.rs`, spawns `webrtc_peer::initiate_connection` |
| A connection fails and the sidecar re-registers (Section 5) | `setup_ice_forwarding` sends `register`, which produces a new `peer_list` |

Any two of these can fire on opposite sides of the same pair. In the re-dial case, as you will see, they fire together on purpose.

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

The fix is in the `"offer"` arm of `handle_signaling_message` in `sidecar/src/signaling.rs`:

```rust
"offer" => {
    // Incoming offer — create answer
    let from_peer = msg.peer_id.clone();

    // Offer glare: both sides offered at once (e.g. two sidecars
    // registering in the same instant both see each other in their
    // peer_list). Deterministic tie-break so exactly one offer wins:
    // the lower peer_id keeps its offer, the higher one yields.
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
            tokio::spawn(async move {
                let _ = existing.close().await;
            });
        }
    }
    // ... then spawn handle_offer as before
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
  return (B's offer ignored)            |          spawn pcB1.close()
      |                                 |          handle_offer -> pcB2
      |                                 |          pcB2 answers pcA1's offer
      |<--------------------------------|<-------------- answer (pcB2) --|
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

Two unit tests pin the rule down. `glare_rule_is_antisymmetric_for_any_distinct_ids` checks property 2 over a small set of IDs, including mixed case (`"Z"` sorts before `"a"` in byte order, and the rule still works because both sides use the same ordering).

### Relationship to "perfect negotiation"

Browsers solve the same problem with the pattern MDN calls **perfect negotiation**. One peer is **polite**: when an offer collides with its own, it drops its offer and answers. The other is **impolite**: it ignores the colliding offer and keeps its own. In chatixia-mesh, the lower `peer_id` plays the impolite role and the higher one the polite role.

There are two differences worth knowing:

- **Rollback vs a fresh connection.** A polite browser peer calls `setRemoteDescription(offer)` on the same connection, which rolls its own offer back. The sidecar instead closes the pending connection and builds a new one in `handle_offer`. A sidecar connection carries no media tracks or other state worth keeping, so a fresh `RTCPeerConnection` is simpler to reason about than rollback. The cost is that the closed connection's callbacks can still fire later, which leads to Section 4.
- **How a collision is detected.** MDN's example keeps a `makingOffer` flag and deliberately does not rely on `signalingState` alone, because "the value of `signalingState` changes asynchronously". The sidecar checks `existing.signaling_state() == RTCSignalingState::HaveLocalOffer`. In `initiate_connection`, `mesh.add_peer` runs before `pc.create_offer` and `pc.set_local_description`, so for a short window our offering connection is on record but still `stable`. An offer that arrives in that window is not seen as glare. ADR-021's five simultaneous restarts did not trigger it, and no test covers it. Exercise 1 asks you to trace it.

### In chatixia-mesh

| Piece | Location |
|-------|----------|
| Collision detection | `sidecar/src/signaling.rs`, `"offer"` arm: `mesh.get_pc` plus `signaling_state() == HaveLocalOffer` |
| Tie-break rule | `sidecar/src/signaling.rs`, `glare_keep_local_offer` |
| Yield (polite side) | Same arm: `existing.close()` in a spawned task, then `webrtc_peer::handle_offer` |
| Keep (impolite side) | Same arm: early `return`; the answer is later applied in the `"answer"` arm |
| Tests | `glare_exactly_one_side_keeps_its_offer`, `glare_rule_is_antisymmetric_for_any_distinct_ids` |
| Log lines | `[SIG] offer glare with <peer>: keeping our offer (lower peer_id wins)` / `... yielding to their offer` |

---

## 3. Candidates That Arrive Too Early

### Trickle ICE recap

In Lesson 03, step 8 of the connection lifecycle said ICE candidates are "trickled in both directions". Trickle ICE (RFC 8838) sends each candidate as soon as the local ICE agent discovers it, instead of waiting for gathering to finish and embedding them all in the SDP. It saves seconds of setup, but it means candidates and descriptions travel as separate messages, and the receiver has to handle them in any order.

A remote candidate is only meaningful once the connection knows the remote side's ICE credentials, which arrive in the remote description. webrtc-rs enforces this directly: `RTCPeerConnection::add_ice_candidate` returns `Error::ErrNoRemoteDescription` if no remote description is set.

### Where the race came from

In `handle_signaling_message`, the arms do not all run the same way:

- `"offer"` spawns `webrtc_peer::handle_offer` in a separate task. That task creates the connection, wires its callbacks, calls `set_remote_description`, and only then calls `mesh.add_peer`.
- `"ice_candidate"` runs inline in the WebSocket read loop.

So the read loop can process the candidates that follow an offer before the spawned task has stored an answering connection. The offerer has a similar race. `handle_offer` calls `set_local_description(answer)`, which starts gathering, and only then sends the answer. A candidate found in between can be queued on the same outbound channel ahead of the answer. It reaches the offerer while the offering connection is still in `have-local-offer` with no remote description.

Before ADR-021, the candidate arm was essentially "if there is a connection for this peer, add the candidate" (Lesson 05 shows this older shape). A candidate with no connection was skipped. A candidate for a connection with no remote description failed in `add_ice_candidate` and was only logged. Either way it was gone.

```
  Offerer (A)                   Registry                  Answerer (B)
      |-- offer ----------------->|--------------------------->| spawn handle_offer
      |                           |                            |   create_peer_connection ...
      |-- ice_candidate (host) -->|--------------------------->| read loop: get_pc(A) = None
      |                           |                            |   -> DROPPED (before fix)
      |-- ice_candidate (srflx) ->|--------------------------->|   -> DROPPED
      |                           |                            |   set_remote_description
      |                           |                            |   add_peer(A)
      |-- ice_candidate (relay) ->|--------------------------->| applied
```

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

The same connection now looks like this:

```
  Offerer (A)                   Registry                  Answerer (B)
      |-- offer ----------------->|--------------------------->| spawn handle_offer
      |-- ice_candidate (host) -->|--------------------------->| no pc yet -> queue [host]
      |-- ice_candidate (srflx) ->|--------------------------->| no pc yet -> queue [host, srflx]
      |                           |                            | set_remote_description
      |                           |                            | add_peer(A)
      |                           |                            | flush -> apply host, srflx
      |<-- answer ----------------|<---------------------------|
```

### Trade-offs the buffer accepts

- **The queue is keyed by `peer_id`, not by connection.** The sidecar's `ice_candidate` payload carries `candidate`, `sdpMid` and `sdpMLineIndex` only (`username_fragment` is set to `None`), so it cannot tell which ICE session a candidate belongs to. During glare, the impolite side queues candidates from the polite side's abandoned offer (pcB1 in the Section 2 diagram) and later flushes them onto its surviving connection. They point at sockets that close with pcB1, so their checks simply fail. It is wasted work, not a correctness bug. MDN's pattern avoids it by ignoring candidates while an offer is being ignored.
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

Now follow the polite side of a glare collision. It calls `existing.close()` on its offering connection pcB1 in a spawned task, and `handle_offer` stores the answering connection pcB2 under the same `peer_id`. Closing pcB1 fires its state callback with `Closed`. A callback that runs `remove_peer("world-the-alpha")` removes whatever is stored under that name, and by now that is pcB2, the healthy replacement:

```
  Polite side (B), connections to peer A
      |
      |  peers["A"] = pcB1 (offering)
      |
      |  glare: spawn pcB1.close();  spawn handle_offer
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

The return value matters too. `peer_disconnected` and the re-dial only happen when the removal actually happened, so a stale callback does nothing that anyone can see. Whether pcB1's `Closed` fires before or after `handle_offer` stores pcB2 is a race, and Exercise 3 asks you to trace both orders.

### In chatixia-mesh

| Piece | Location |
|-------|----------|
| Connection-identity removal | `MeshManager::remove_peer_if_pc` |
| Channel-identity removal | `MeshManager::remove_peer_if_channel` |
| Callers | `setup_ice_forwarding` (state change), `setup_datachannel_handler` (`on_close`) |
| Unconditional removal (still used) | `MeshManager::remove_peer`, called by the two guarded versions |

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

- **`Disconnected` is treated as final.** ICE can recover from `Disconnected`, but the sidecar removes the peer as soon as it gets there, which is after 5 s of silence. A short blip that ICE would have ridden out now costs a full new handshake. The sidecar also does not call `close()` on the connection it stops tracking (`remove_peer` only drops map entries), so whatever happens to that connection afterwards is up to webrtc-rs.
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

---

## 6. How the Bugs Were Found, and Why Unit Tests Missed Them

Every bug in this lesson was found by running the real thing: two sidecars, one registry, real WebSockets, real ICE, started by chatixia-world's `scripts/mesh-demo.sh`. The third failure from ADR-021 is the clearest example of why that matters.

**The rustls provider panic.** `reqwest` 0.13 turns on rustls with the `aws-lc-rs` crypto backend. webrtc's DTLS stack turns it on with `ring`. With both compiled in, rustls cannot pick a process-wide default and panics on first use, which is the first DTLS handshake, right after ICE connects. The fix is a single line at the top of `main()` in `sidecar/src/main.rs`:

```rust
let _ = rustls::crypto::ring::default_provider().install_default();
```

ADR-021 notes that the lockfile has had this combination since the reqwest 0.13 bump in March 2026, so the sidecar most likely could not open a DataChannel for about six months. All that time the test suite passed.

Here is why each bug was out of reach for the unit tests:

| Bug | What it takes to trigger | Why unit tests could not see it |
|-----|--------------------------|----------------------------------|
| Offer glare | Two processes registering within milliseconds of each other | Tests use one `MeshManager` and never run an offer/answer between two sidecars |
| Early candidates | A spawned task losing a race to the WebSocket read loop | Needs a real relay; tests never call `handle_signaling_message` |
| Stale removal | A glare yield followed by a late `Closed` callback | Only happens after glare, which never happened in tests |
| rustls panic | A DTLS handshake | `mesh.rs` tests create `RTCPeerConnection`s but never connect two of them |
| No re-dial | 5-30 s of ICE silence while the WebSocket stays up | Needs a live connection and a way to kill its path, not its process |

This is Lesson 17's point about seams. The heartbeat bug there sat between a registry response and a runner that ignored it. These bugs sit between two copies of the same program, where each copy is correct on its own and the failure comes from how their timing combines.

### What the new unit tests do cover

The fixes came with three tests:

- `glare_exactly_one_side_keeps_its_offer` and `glare_rule_is_antisymmetric_for_any_distinct_ids` in `sidecar/src/signaling.rs`
- `test_should_redial_only_on_involuntary_end` in `sidecar/src/webrtc_peer.rs`

Look at what they test. Each one checks a **decision**, pulled out into a small pure function: who keeps the offer, which states re-dial. None of them checks the **wiring**: that the decision is consulted at the right moment, that the flush happens after `add_peer`, or that a stale callback is really ignored. That split is deliberate. Pure decisions are cheap to test exhaustively. Wiring can only be tested by running the protocol.

### What the evidence is instead

The ADRs are open about how the fixes were verified:

- **ADR-021:** five simultaneous restarts of both sidecars on one machine. Each time the DataChannel came back in 4-5 s, mostly the 3 s respawn delay. Glare and early candidates each happened during that run and were resolved. It was also the first end-to-end LLM dialogue across two sidecars, with 1-3 ms one-hop DataChannel latency on localhost.
- **ADR-022:** one sidecar paused with `SIGSTOP` past the ICE consent timeout, then resumed with `SIGCONT`. Pausing the process freezes it without closing its sockets, so the registry still sees a live WebSocket, which is the exact shape of the doze incident.

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

## 7. What Is Still Untested

**No automated test drives these paths.** ADR-021 says "pure-unit coverage still cannot exercise these paths", and ADR-022 says "still no integration test that can see this". As of this lesson, the evidence is the manual runs above. A two-sidecar integration test is being written separately. Until it lands, treat the ADRs' measurements as the only proof that glare, early candidates and re-dial work, and rerun them after any change to `signaling.rs`, `mesh.rs` or `webrtc_peer.rs`.

**No run across real NATs yet.** Every result so far is from one machine. ADR-021 lists the two-machine cross-NAT run as outstanding. `docs/DEPLOYMENT_GUIDE.md` ("chatixia-world across two NATs") points to the world's runbook, and describes the evidence to look for: the `[ICE] <peer> selected pair` line, where `srflx`, `prflx` or `relay` means a real NAT crossing and `host` means same LAN. The same data travels to the agent on the `peer_connected` IPC message. Across NATs, relay latency and wifi make every race in this lesson more likely, not less.

**Questions the code raises that nobody has tested.** Reading the current code, not the ADRs, turns up these. They are hypotheses, not confirmed bugs. They are exactly what an integration test should try to provoke:

| Observation | Where | What could happen |
|-------------|-------|-------------------|
| An offering connection is on record before it reaches `have-local-offer` | `initiate_connection`: `add_peer` before `set_local_description` | An offer arriving in that window is not detected as glare (Section 2) |
| `peer_list` checks `is_connected`, which only becomes true when a DataChannel opens | `"peer_list"` arm | A negotiation already in progress does not stop a second offer to the same peer |
| Candidates are queued by `peer_id`, not by session | `pending_candidates` | Candidates from an abandoned offer get applied to the surviving connection (Section 3) |
| `Disconnected` is treated as final, and the old connection is not closed | State callback, `remove_peer` | Needless handshakes on short blips; untracked connections left to the library (Section 5) |
| The registry removes a peer by name when a socket closes | `handle_ws` cleanup: `state.signaling.remove_peer(&peer_id)` | The same stale-removal pattern as Section 4, on the registry side (Exercise 4) |

---

## Exercises

### Exercise 1: The window before `have-local-offer`

In `webrtc_peer::initiate_connection`, the order is `create_peer_connection`, `setup_ice_forwarding`, `create_data_channel`, `mesh.add_peer`, `create_offer`, `set_local_description`, then send the offer.

1. Suppose B's offer reaches A after `mesh.add_peer` but before `set_local_description`. What does the `"offer"` arm on A see when it calls `existing.signaling_state()`? Which branch runs?
2. Follow both sidecars from there until the handshake either succeeds or both connections reach a final state. Which log lines appear? Does the re-dial path from Section 5 eventually recover?
3. Propose a fix. Compare at least two: a `making_offer` set in `MeshManager` (MDN's approach), and reordering `initiate_connection`. For each, explain what new race it could create (hint: `set_local_description` starts candidate gathering, and `on_ice_candidate` sends right away).

### Exercise 2: Why not let only the lower ID offer?

A colleague proposes removing glare completely: on `peer_list`, a sidecar offers only to peers whose `peer_id` is higher than its own, and the others wait for an offer.

1. Which of the three offer triggers from Section 1 does this break or complicate? Think about IPC `connect` from the agent with the higher ID, and about re-dial when only the higher-ID side notices the failure.
2. Would the glare check in the `"offer"` arm still be needed? Why or why not?
3. Write a short ADR-style Decision and Consequences section for either keeping the current design or switching. Include at least one (+) and one (-) for each.

### Exercise 3: Trace the stale `Closed` callback

On the polite side of a glare collision, `existing.close()` and `handle_offer` run in two separately spawned tasks.

1. Order 1: pcB1's `Closed` callback runs **before** `handle_offer` calls `mesh.add_peer`. What does `remove_peer_if_pc` return? Does the agent get `peer_disconnected`? Does a re-dial happen? What does `handle_offer`'s `add_peer` then do?
2. Order 2: the callback runs **after** `add_peer`. Answer the same questions.
3. In which order does the Python agent see a spurious `peer_disconnected` for a peer it was never told was connected? Is that harmful, given how `agent/chatixia/core/mesh_client.py` tracks peers?
4. Explain in two sentences why comparing `Weak::as_ptr` values cannot give a false match, even if the old connection has already been dropped.

### Exercise 4: The same bug on the registry side

`handle_ws` in `registry/src/main.rs` calls `state.signaling.add_peer(&peer_id, tx)` on connect and `state.signaling.remove_peer(&peer_id)` when its loop ends.

1. Describe a sequence where a sidecar's new WebSocket is registered before the registry notices its old one has closed. (Hint: think about a half-open TCP connection after a wifi drop, and how quickly `signaling::run` reconnects after a session ends.) What does the old socket's cleanup do to the new session?
2. What does the sidecar observe afterwards? Would the re-dial path from Section 5 help?
3. Write `SignalingState::remove_peer_if_sender(&self, peer_id: &str, tx: &mpsc::UnboundedSender<String>) -> bool` using `UnboundedSender::same_channel`, and show the change to `handle_ws`. Write a unit test in the style of the existing ones in `registry/src/signaling.rs` that fails without your fix.

### Exercise 5: Design the two-sidecar integration test

Design (do not necessarily implement) an automated test that would have caught every bug in this lesson.

1. List the scenarios: simultaneous start, a delayed offer relay (so candidates overtake it), a `SIGSTOP`/`SIGCONT` past the consent timeout, and a registry restart. For each, name the observable assertion: an IPC `peer_connected` event, a log line, or a DataChannel round trip.
2. The defaults make a consent-timeout test take 30+ seconds. How would you shorten it? `SettingEngine::set_ice_timeouts` exists in webrtc-rs but the sidecar does not call it. Sketch a test-only environment variable and argue whether it belongs in production code.
3. Where would you inject the delay for scenario 2: in a proxy between sidecar and registry, or in the registry itself? What does each choice cost in realism?
4. Which scenarios can run in CI on one machine, and which need two networks?

### Exercise 6 (open-ended): Should `Disconnected` trigger a re-dial?

The sidecar acts on `Disconnected` (5 s of silence). It could instead wait for `Failed` (30 s) and let ICE recover from short blips by itself. Argue both sides for chatixia-world's target deployment: a Raspberry Pi on home wifi talking to a laptop across NATs, with "no sidecar restart" as the pass condition and a thought log that users watch in real time. Consider recovery time, the cost of a needless handshake, the untracked connection left behind by `remove_peer`, and what the agent sees on the IPC channel in each case. State which you would choose, and what measurement would change your mind.

---

## Summary

The handshake from Lessons 03 and 05 is correct, but it only describes one ordering of events. The first real two-sidecar run found the others:

- **Offer glare.** When both peers offer at once, they have to agree on which offer survives without asking anyone. The sidecar compares `peer_id`s: the lower one keeps its offer (impolite), the higher one closes its connection and answers (polite). The rule works because both sides have the inputs, and it is antisymmetric and stable.
- **Early candidates.** Trickle ICE sends candidates independently of the offer and answer, and `add_ice_candidate` needs a remote description. `MeshManager::pending_candidates` holds early candidates and flushes them at every point where a description can appear.
- **Stale callbacks.** A connection that has been replaced must not remove its replacement. `remove_peer_if_pc` and `remove_peer_if_channel` compare identity, not names.
- **Dead path, live signaling.** ICE liveness checks (RFC 7675 consent freshness; 5 s to `Disconnected` and 30 s to `Failed` in webrtc-ice) can fail a connection while the WebSocket is fine. The sidecar now re-registers after `REDIAL_DELAY`, reusing the normal offer path and ADR-021's tie-break.

None of this was visible to unit tests, because each bug is about timing between two copies of the same program. The new tests check the decisions, but nothing yet checks the wiring. Until a two-sidecar integration test and a real cross-NAT run exist, the evidence is manual runs, and the code still has open questions to test.

---

## Related Lessons

- [Lesson 01: Why Distributed Systems](01-why-distributed-systems.md) -- control plane vs data plane; Section 5's incident is the two failing separately
- [Lesson 02: Peer-to-Peer Networking](02-peer-to-peer-networking.md) -- host, srflx and relay candidates; NAT behavior behind the hairpinning failure
- [Lesson 03: WebRTC Fundamentals](03-webrtc-fundamentals.md) -- the connection lifecycle this lesson breaks apart
- [Lesson 05: Signaling Protocol Design](05-signaling-protocol-design.md) -- `register` / `peer_list` / `offer` / `answer` / `ice_candidate`; its code excerpts show the candidate handling from before ADR-021
- [Lesson 10: The Sidecar Pattern](10-sidecar-pattern.md) -- `MeshManager` and the sidecar's internal structure
- [Lesson 15: Deployment Patterns](15-deployment-patterns.md) -- connectivity tiers and cross-network setup, where these races become more likely
- [Lesson 16: Architecture Decision Records](16-architecture-decision-records.md) -- ADR-021 and ADR-022 are incident-driven ADRs with measured consequences
- [Lesson 17: Testing Distributed Systems](17-testing-distributed-systems.md) -- seams, the E2E gap, and Exercise 4's signaling integration test, which Exercise 5 here extends

---

## Further Reading

- [RFC 8445](https://datatracker.ietf.org/doc/html/rfc8445) -- ICE. Candidate pairs, connectivity checks, nomination.
- [RFC 8838](https://datatracker.ietf.org/doc/html/rfc8838) -- Trickle ICE: incremental provisioning of candidates. Why candidates and descriptions travel separately.
- [RFC 7675](https://datatracker.ietf.org/doc/html/rfc7675) -- STUN usage for consent freshness. The liveness rule behind `Disconnected` and `Failed`.
- [RFC 9429](https://datatracker.ietf.org/doc/html/rfc9429) -- JSEP. The signaling state machine, offer/answer rules, and rollback.
- [MDN: Establishing a connection -- the WebRTC perfect negotiation pattern](https://developer.mozilla.org/en-US/docs/Web/API/WebRTC_API/Perfect_negotiation) -- polite and impolite peers, and why `makingOffer` is tracked separately from `signalingState`.
- [Perfect negotiation in WebRTC](https://blog.mozilla.org/webrtc/perfect-negotiation-in-webrtc/) -- Jan-Ivar Bruaroey's Mozilla blog post introducing the pattern.
- `docs/ADR.md` -- ADR-021 and ADR-022 in full, including the measurements quoted in this lesson.
- `docs/DEPLOYMENT_GUIDE.md` -- "chatixia-world across two NATs" and the `selected pair` evidence line.
- webrtc-ice 0.17.2 source, `src/agent/agent_config.rs` and `src/agent/agent_internal.rs` -- the keepalive, disconnected and failed timeouts, and the consent binding requests.
