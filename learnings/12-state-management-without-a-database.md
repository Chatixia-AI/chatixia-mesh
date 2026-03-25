# Lesson 12: In-Memory State -- DashMap, Eventual Consistency, and the Database Question

**Prerequisites:** Lesson 04 (Registry Architecture), Lesson 07 (Tokio and Async Runtime)

---

## 1. The Spectrum of State Management

Every networked service needs to store state -- who is connected, what work is pending, what configuration applies. The question is *where* that state lives and what guarantees it provides.

There are three broad tiers:

| Tier | Example | Durability | Speed | Complexity | Scaling |
|---|---|---|---|---|---|
| In-memory | `HashMap`, `DashMap` | None -- lost on restart | Nanoseconds | Minimal -- no dependencies | Single process |
| Embedded DB | SQLite, RocksDB | Durable to disk | Microseconds | Low -- file on disk, no server | Single process (SQLite) |
| External DB | PostgreSQL, Redis | Durable, replicated | Milliseconds (network hop) | High -- separate service to deploy | Multi-instance, horizontal |

The chatixia-mesh registry sits firmly in the first tier. All state lives in `DashMap` instances -- concurrent hash maps that exist only in the registry process's memory. When the process stops, everything is gone.

This is a deliberate choice (see ADR-004). The registry is a single binary with zero external dependencies. No database to install, no connection strings to configure, no schema migrations to run. For a mesh network where agents re-register themselves every 15 seconds, this trade-off is surprisingly workable.

The key insight: **if your clients will re-send their state on a fixed interval, you can treat the server as a cache rather than a source of truth**. The agents *are* the source of truth for their own existence. The registry merely aggregates that truth.

---

## 2. DashMap: A Concurrent HashMap for Rust

Rust's standard `HashMap` is not thread-safe. You cannot share it across async tasks without wrapping it in a `Mutex<HashMap<K, V>>` or `RwLock<HashMap<K, V>>`. Both options have a problem: the *entire* map is locked for every operation.

`DashMap` solves this with **sharded locking**. Internally, it splits the map into multiple shards (16 by default). Each shard has its own `RwLock`. When you access a key, DashMap hashes it, determines which shard it belongs to, and locks only that shard. Two operations on keys in different shards proceed in parallel with zero contention.

```
DashMap internals (simplified):

  Key "agent-alpha" --hash--> shard 3  --lock shard 3--> read/write
  Key "agent-beta"  --hash--> shard 7  --lock shard 7--> read/write (concurrent!)
  Key "agent-gamma" --hash--> shard 3  --lock shard 3--> waits for "agent-alpha"

  +--------+--------+--------+--------+-----+----------+
  | Shard 0| Shard 1| Shard 2| Shard 3| ... | Shard 15 |
  +--------+--------+--------+--------+-----+----------+
  | RwLock | RwLock | RwLock | RwLock |     | RwLock   |
  +--------+--------+--------+--------+-----+----------+
```

### How chatixia-mesh uses DashMap

The registry uses 6 DashMap instances across 4 state structs. Each one maps a string key to a domain-specific record:

| State struct | DashMap field | Key | Value | File |
|---|---|---|---|---|
| `RegistryState` | `agents` | `agent_id` | `AgentRecord` | `registry/src/registry.rs` |
| `HubState` | `tasks` | `task_id` | `Task` | `registry/src/hub.rs` |
| `SignalingState` | `peers` | `peer_id` | `mpsc::UnboundedSender<String>` | `registry/src/signaling.rs` |
| `PairingState` | `codes` | code string | `InviteCode` | `registry/src/pairing.rs` |
| `PairingState` | `onboarding` | entry id | `OnboardingEntry` | `registry/src/pairing.rs` |
| `PairingState` | `rate_limits` | IP address | `Vec<Instant>` | `registry/src/pairing.rs` |

All six are created with `DashMap::new()` and require no configuration. They are wrapped in `Arc` at the application level so they can be shared across Tokio tasks and HTTP handlers:

```rust
// registry/src/main.rs (lines 39-46, 60-66)
#[derive(Clone)]
pub struct AppState {
    pub auth: Arc<AuthState>,
    pub signaling: Arc<SignalingState>,
    pub registry: Arc<RegistryState>,
    pub hub: Arc<HubState>,
    pub pairing: Arc<PairingState>,
}

let state = AppState {
    registry: Arc::new(RegistryState::new()),
    hub: Arc::new(HubState::new()),
    // ...
};
```

### Common DashMap operations in the codebase

**Insert (overwrite):**

```rust
// registry/src/registry.rs, line 176
state.registry.agents.insert(info.agent_id.clone(), record);
```

No `entry()` API needed here -- if the agent already exists, the old record is replaced.

**Get (read-only):**

```rust
// registry/src/registry.rs, lines 127-129
pub fn get(&self, agent_id: &str) -> Option<AgentRecord> {
    self.agents.get(agent_id).map(|e| e.value().clone())
}
```

The `.get()` method returns a `Ref<K, V>` -- a guard that holds the shard's read lock. You must `.clone()` the value or extract what you need before the guard is dropped. Never hold the guard across an `.await` point.

**Get mutable (read-write):**

```rust
// registry/src/registry.rs, lines 235-248
if let Some(mut entry) = state.registry.agents.get_mut(&hb.agent_id) {
    entry.last_heartbeat = now_str;
    entry.last_heartbeat_epoch = now_epoch;
    entry.health = "active".into();
    // ... update fields ...
}
```

The `.get_mut()` method returns a `RefMut<K, V>` that holds the shard's *write* lock. Only one writer per shard at a time. The heartbeat handler uses this to update an existing agent's record in place without removing and re-inserting it.

**Iterate with mutation:**

```rust
// registry/src/registry.rs, lines 108-117
for mut entry in self.agents.iter_mut() {
    let age = now - entry.last_heartbeat_epoch;
    entry.health = if age > 270.0 {
        "offline".into()
    } else if age > 90.0 {
        "stale".into()
    } else {
        "active".into()
    };
}
```

`iter_mut()` locks each shard in sequence as it iterates. This is safe but means the health check loop briefly blocks writes to each shard, one at a time.

**Retain (bulk removal):**

```rust
// registry/src/pairing.rs, lines 196-197
self.codes
    .retain(|_, ic| !ic.used && ic.created_at.elapsed().as_secs() < CODE_TTL_SECS);
```

`retain()` iterates all entries and removes those where the closure returns `false`. This is used for TTL-based cleanup of expired invite codes.

**Remove:**

```rust
// registry/src/registry.rs, line 191
state.registry.agents.remove(&agent_id);
```

Removes a key-value pair atomically and returns the old value (if any).

---

## 3. Eventual Consistency via Heartbeats

The chatixia-mesh registry does not require agents to be pre-configured. There is no agent list in a config file, no database seed, no startup ordering. Agents announce themselves by sending heartbeats, and the registry builds its view of the world from those heartbeats.

The heartbeat handler (`POST /api/hub/heartbeat`) in `registry/src/registry.rs` (lines 227-287) performs an **upsert**:

1. If the agent exists in the DashMap, update its fields (hostname, IP, skills, timestamp).
2. If the agent does not exist, create a new `AgentRecord` and insert it.

This means the registry's state is always **eventually consistent** with the actual set of running agents. The convergence window is one heartbeat interval: 15 seconds.

```
Timeline of eventual consistency:

  T=0s    Registry starts (empty state, zero agents)
  T=3s    Agent-A sends heartbeat --> registry now knows about Agent-A
  T=8s    Agent-B sends heartbeat --> registry now knows about A and B
  T=15s   Agent-A sends heartbeat --> A's record refreshed
  T=18s   Agent-B sends heartbeat --> B's record refreshed

  Registry restart at T=20s:

  T=20s   Registry restarts (empty state again)
  T=25s   Agent-A sends heartbeat --> registry re-learns Agent-A
  T=33s   Agent-B sends heartbeat --> registry re-learns Agent-B
  T=33s   Full convergence -- same state as before restart
```

This design has a profound operational benefit: **the registry has no startup dependencies**. You can restart it at any time, and within 15 seconds the world rebuilds itself. There is no database to restore from backup, no WAL to replay, no snapshot to load.

The trade-off is equally clear: during those 15 seconds after a restart, the registry has an incomplete view. Any request to list agents or route by skill during that window will return partial results. For a monitoring dashboard, this is a brief flicker. For a task queue, this means tasks submitted during the gap may not find their target agent.

---

## 4. The Health State Machine

The registry tracks each agent's health as a simple state machine driven by elapsed time since the last heartbeat. The `health_check_loop` in `registry/src/registry.rs` (lines 104-119) runs every 15 seconds and scans all agents:

```
                     Health State Machine
                     ====================

                  heartbeat received
                  (any state --> active)
                         |
                         v
                   +----------+
                   |  active  |   last heartbeat < 90s ago
                   +----------+
                         |
                         | 90s without heartbeat
                         v
                   +----------+
                   |  stale   |   last heartbeat 90-270s ago
                   +----------+
                         |
                         | 270s without heartbeat
                         v
                   +----------+
                   |  offline |   last heartbeat > 270s ago
                   +----------+

  Time thresholds:
    active  ->  stale:    90 seconds  (6 missed heartbeats)
    stale   ->  offline: 270 seconds (18 missed heartbeats)
    any     ->  active:  next heartbeat received
```

The implementation is a straightforward scan:

```rust
// registry/src/registry.rs, lines 104-119
pub async fn health_check_loop(&self) {
    loop {
        tokio::time::sleep(Duration::from_secs(15)).await;
        let now = epoch_now();
        for mut entry in self.agents.iter_mut() {
            let age = now - entry.last_heartbeat_epoch;
            entry.health = if age > 270.0 {
                "offline".into()
            } else if age > 90.0 {
                "stale".into()
            } else {
                "active".into()
            };
        }
    }
}
```

A few things worth noting:

- **The loop uses `tokio::time::sleep`, not `tokio::time::interval`.** The difference: `sleep` waits 15 seconds *after* the previous iteration completes. If the scan takes 100ms, the period is 15.1 seconds. `interval` would try to maintain exactly 15-second ticks, potentially running immediately if the previous iteration took longer than 15 seconds. For a health check, `sleep` is the simpler and safer choice.

- **Health transitions are one-directional within a scan.** An agent can only move from active to stale to offline based on elapsed time. The only way back to "active" is receiving a heartbeat (handled separately in the `heartbeat` handler, line 238).

- **The scan locks each shard briefly via `iter_mut()`.** With 16 shards and a typical agent count under 50, this is negligible.

- **Skill routing filters by health.** The `find_by_skill` method (line 132) only returns agents with `health == "active"`. A stale or offline agent is invisible to skill-based routing, which prevents task assignment to agents that are probably dead.

---

## 5. The TTL Pattern

Three separate background loops in the registry use the same pattern: sleep, scan, expire. Each one is a `tokio::spawn`'ed task that runs for the lifetime of the process.

```
Background TTL loops:

  +---------------------+----------+-------------------+---------------------------+
  | Loop                | Interval | TTL               | What expires              |
  +---------------------+----------+-------------------+---------------------------+
  | health_check_loop   | 15s      | 90s stale / 270s  | Agent health transitions  |
  |                     |          | offline           |                           |
  +---------------------+----------+-------------------+---------------------------+
  | expire_tasks_loop   | 30s      | 300s (task.ttl)   | Pending/assigned tasks    |
  |                     |          |                   | marked "failed"           |
  +---------------------+----------+-------------------+---------------------------+
  | cleanup_loop        | 60s      | 300s (codes)      | Used/expired invite codes |
  |                     |          | empty (rate lim.) | Empty rate-limit buckets  |
  +---------------------+----------+-------------------+---------------------------+
```

All three are spawned in `registry/src/main.rs` (lines 69-76):

```rust
let reg = state.registry.clone();
tokio::spawn(async move { reg.health_check_loop().await });

let hub = state.hub.clone();
tokio::spawn(async move { hub.expire_tasks_loop().await });

let pairing = state.pairing.clone();
tokio::spawn(async move { pairing.cleanup_loop().await });
```

### Task expiration

The `expire_tasks_loop` in `registry/src/hub.rs` (lines 71-87) runs every 30 seconds and checks each task's age against its TTL:

```rust
pub async fn expire_tasks_loop(&self) {
    loop {
        tokio::time::sleep(Duration::from_secs(30)).await;
        let now = epoch_now();
        for mut entry in self.tasks.iter_mut() {
            let task = entry.value_mut();
            if task.state == "completed" || task.state == "failed" {
                continue;
            }
            if now - task.created_at > task.ttl as f64 {
                task.state = "failed".into();
                task.error = "TTL expired".into();
                task.updated_at = now;
            }
        }
    }
}
```

Key details:
- The default TTL is 300 seconds (5 minutes), set by `default_ttl()` in `hub.rs` line 28.
- Tasks that are already `completed` or `failed` are skipped, not removed. The DashMap grows unboundedly for completed tasks. (This is a known limitation -- there is no garbage collection of finished tasks.)
- Expired tasks are marked `failed` with error `"TTL expired"`, not removed. This preserves them for debugging.

### Invite code cleanup

The `cleanup_loop` in `registry/src/pairing.rs` (lines 190-206) runs every 60 seconds and does two things:

```rust
pub async fn cleanup_loop(&self) {
    loop {
        tokio::time::sleep(Duration::from_secs(60)).await;

        // Remove expired / used codes
        self.codes
            .retain(|_, ic| !ic.used && ic.created_at.elapsed().as_secs() < CODE_TTL_SECS);

        // Prune empty rate-limit buckets
        self.rate_limits.retain(|_, v| !v.is_empty());
    }
}
```

1. **Invite codes** are removed if they have been used or if they are older than 300 seconds (`CODE_TTL_SECS`). The `retain()` method on DashMap is atomic per-shard -- it locks each shard, filters its entries, and unlocks.

2. **Rate-limit buckets** are pruned if they are empty. The rate-limit entries themselves (timestamps within each bucket) are pruned on access in `check_rate_limit()` (line 181), which removes timestamps older than the 60-second window. The cleanup loop just removes the empty bucket shells.

### Why this pattern works

The TTL pattern avoids the need for per-entry timers. Instead of setting a timer for each task or code (which would consume a Tokio timer slot per entry), a single background scan handles all entries in one pass. This is O(N) per scan but runs infrequently relative to the entry count, making it efficient in practice.

The worst-case staleness of any TTL is the TTL value plus one scan interval. A task with a 300-second TTL could live for up to 330 seconds (300 + 30s scan interval) before being marked failed.

---

## 6. When to Add a Database

The in-memory approach works because the chatixia-mesh registry is a **coordination point, not a system of record**. But it has clear limits. Here is exactly what happens when the registry process restarts:

```
What is lost on registry restart:

  +---------------------+------------------+------------------------------------+
  | Data                | Recovery         | Impact                             |
  +---------------------+------------------+------------------------------------+
  | Agent registrations | Auto-recover     | Agents re-register within 15s via  |
  |                     | (~15s)           | heartbeat. Brief dashboard flicker.|
  +---------------------+------------------+------------------------------------+
  | Task queue          | LOST permanently | Pending/assigned tasks vanish.     |
  |                     |                  | No retry, no notification.         |
  +---------------------+------------------+------------------------------------+
  | Signaling peers     | Auto-recover     | Sidecars reconnect WebSocket and   |
  |                     | (seconds)        | re-register. Brief connectivity    |
  |                     |                  | gap.                               |
  +---------------------+------------------+------------------------------------+
  | Invite codes        | LOST permanently | In-flight pairing attempts fail.   |
  |                     |                  | Admins must generate new codes.    |
  +---------------------+------------------+------------------------------------+
  | Onboarding entries  | LOST permanently | Approved agents lose their device  |
  |                     |                  | tokens. Must re-pair.              |
  +---------------------+------------------+------------------------------------+
  | Rate-limit buckets  | Reset (harmless) | Rate limits restart from zero.     |
  |                     |                  | Briefly allows extra attempts.     |
  +---------------------+------------------+------------------------------------+
```

The auto-recovering items (agent registrations, signaling peers) are fine because the agents continuously re-announce themselves. The permanently lost items (tasks, invite codes, onboarding entries) represent real data loss.

ADR-004 explicitly documents the migration path:

> **Migration path:** Add PostgreSQL for task queue and agent registry when persistence or multi-instance is needed.

### What a database migration would look like

You would not replace all DashMaps with database tables. The right approach is selective: move only the state that *must* survive a restart to the database, and keep the rest in memory.

**Move to database:**
- **Task queue** -- tasks have a lifecycle that spans minutes and must not be lost.
- **Onboarding entries** -- approved device tokens are long-lived credentials.

**Keep in memory:**
- **Agent registrations** -- rebuilt from heartbeats; storing them is redundant.
- **Signaling peers** -- WebSocket sender channels are inherently in-process; you cannot serialize a `mpsc::UnboundedSender`.
- **Rate-limit buckets** -- ephemeral by nature; losing them on restart is harmless.
- **Invite codes** -- 5-minute TTL means they are nearly expired by the time you would restore them.

### What a database enables beyond durability

The second reason to add a database is **horizontal scaling**. With all state in DashMap, only one registry instance can run at a time. Two instances would have divergent views of the world, with agents randomly registering with one or the other.

With a shared database, you can run multiple registry instances behind a load balancer. Agent heartbeats go to any instance; all instances see the same task queue. The signaling peer map would still need coordination (via Redis pub/sub or a shared WebSocket state layer), but the registry API would scale horizontally.

---

## Exercises

### Exercise 1: Recovery Analysis

The registry process is killed with `kill -9` (SIGKILL, no clean shutdown). Ten seconds later, it is restarted.

1. List every category of state that was lost.
2. For each category, state whether it recovers automatically, and if so, how long until full recovery.
3. A task was in `assigned` state when the registry died. The assigned agent completes the work 5 seconds after the restart. What happens when it tries to POST the result back?

### Exercise 2: PostgreSQL Migration for Tasks

Design a PostgreSQL schema for the task queue (and only the task queue). Consider:

1. What table(s) do you need? What columns and types?
2. Write the SQL query that replaces `get_pending_for_agent` -- it must atomically find pending tasks matching an agent's skills and update their state to `assigned`.
3. How do you handle task expiration? Do you keep `expire_tasks_loop` or use a database-level mechanism?
4. What happens to the `DashMap<String, Task>` in `HubState`? Does it stay as a cache, or is it removed entirely?

### Exercise 3: Timing Analysis

An agent crashes at T=0 and never sends another heartbeat.

1. The `health_check_loop` runs every 15 seconds with `tokio::time::sleep(Duration::from_secs(15))`. What is the **worst-case** time for the agent to appear as "stale" (health check must observe `age > 90.0`)? Show your reasoning.
2. What is the worst-case time to appear as "offline" (`age > 270.0`)?
3. Now consider: the agent sent its last heartbeat at T=-1s (one second before crashing). Does this change your answer?

*Hint: the worst case depends on when the last heartbeat was sent relative to the crash AND when the next health check scan runs.*

### Exercise 4: Horizontal Scaling

You need to run two registry instances behind a load balancer.

1. Which DashMaps must be replaced with shared state? For each one, explain why it cannot remain instance-local.
2. The `SignalingState` peers map holds `mpsc::UnboundedSender<String>` -- an in-process channel handle. This cannot be stored in a database. How would you coordinate signaling across two instances?
3. Agent heartbeats are load-balanced randomly between the two instances. How does `get_pending_for_agent` (which atomically reads AND updates task state) work without double-assignment?
4. What coordination mechanism would you use for the shared state? Evaluate: shared PostgreSQL, Redis, or a custom protocol between instances.

---

## Summary

The chatixia-mesh registry uses in-memory `DashMap` instances as its sole state store. This eliminates infrastructure dependencies and delivers nanosecond-scale reads, but it means all state is volatile. The system compensates through two mechanisms:

1. **Heartbeat-driven eventual consistency** -- agents continuously re-announce themselves, so the registry rebuilds its agent view within 15 seconds of any restart.
2. **Background TTL loops** -- three `tokio::spawn`'ed tasks scan their respective DashMaps on fixed intervals, expiring stale entries and transitioning health states.

The state that *cannot* self-heal (task queue, onboarding entries) is the state that would justify adding a database. ADR-004 documents this as a planned migration path for when the system needs durability or multi-instance scaling. Until then, the simplicity of a single-binary, zero-dependency deployment is the stronger priority.
