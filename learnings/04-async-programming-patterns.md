# Lesson 04: Async Programming -- Concurrency Without Threads

**Prerequisites:** None (parallel to Lessons 01--03).

---

## 1. The Problem with Threads

### Thread-per-connection

The simplest model for a network server is to spawn one thread for every client connection. A chat server using this model might look like:

```
Client A  --->  Thread A (blocks on read)
Client B  --->  Thread B (blocks on read)
Client C  --->  Thread C (blocks on read)
...
Client N  --->  Thread N (blocks on read)
```

Each thread blocks on I/O (waiting for the client to send data), wakes up to process the message, then blocks again. This works well for a handful of clients. It breaks down at scale.

### Why threads get expensive

Every OS thread carries costs:

- **Stack memory.** Each thread gets its own stack, typically 1--8 MB. 1,000 threads means 1--8 GB of stack space alone.
- **Context switching.** When the OS switches from one thread to another, it saves and restores registers, updates page tables, and flushes caches. This costs roughly 1--6 microseconds per switch. At high thread counts, the CPU spends more time switching than doing useful work.
- **Scheduling overhead.** The OS scheduler must decide which thread runs next. With thousands of runnable threads, this decision itself becomes expensive.
- **Shared mutable state.** When multiple threads access the same data, you need locks (mutexes). Locks introduce new problems: lock contention slows things down, and incorrect lock ordering causes deadlocks -- where two threads each hold a lock the other needs, so both freeze forever.

### The C10K problem

In 1999, Dan Kegel posed the C10K problem: how do you handle 10,000 concurrent connections on a single server? The thread-per-connection model fails here. 10,000 threads consume gigabytes of stack memory and generate devastating context-switch overhead. The OS scheduler, designed for dozens of threads, collapses under the load.

The solution, broadly, is to stop mapping one connection to one thread. Instead, use a small number of threads to handle many connections by only paying attention to connections that have data ready. This is the foundation of event-driven programming.

---

## 2. Event Loops and async/await

### The reactor pattern

Instead of blocking a thread per connection, the **reactor pattern** uses a single thread (or a small pool of threads) with an event loop:

1. Register interest in I/O events (e.g., "notify me when socket A has data").
2. Call the OS to wait for any registered event (Linux: `epoll`, macOS: `kqueue`).
3. When an event fires, run the handler for that event.
4. Go back to step 2.

```
Event Loop (single thread)
  |
  |--- Socket A has data --> run handler_A()
  |--- Socket B has data --> run handler_B()
  |--- Timer expired      --> run timer_handler()
  |--- Socket C has data --> run handler_C()
  |
  (loop forever)
```

A single thread can now handle thousands of connections because it never blocks on any one of them. It only processes connections that have work to do.

### Cooperative multitasking vs. preemptive

Traditional threads use **preemptive multitasking**: the OS forcibly interrupts a running thread after a time slice (typically 1--10 ms) and switches to another. The thread has no say in when it gets paused.

Async programming uses **cooperative multitasking**: each task voluntarily yields control when it hits an I/O wait point. The runtime never interrupts a task mid-computation. This means:

- No context-switch overhead from the OS.
- No need for locks on data that is only accessed between yield points.
- But a task that does heavy computation without yielding will block all other tasks on that thread.

### async/await syntax

Modern languages wrap the reactor pattern in `async/await` syntax, which lets you write code that reads like blocking code but actually yields at every `await`:

```
// Looks sequential, but actually cooperative
async function handle_connection(socket):
    data = await socket.read()     // yields here, comes back when data arrives
    result = process(data)
    await socket.write(result)     // yields here too
```

The `await` keyword marks yield points. When the runtime hits an `await`, it parks the current task and runs another one that is ready. When the I/O completes, the runtime resumes the original task exactly where it left off.

---

## 3. Rust Async: Tokio

Rust's async model is built on **futures**: values that represent a computation that will complete in the future. Unlike JavaScript promises, Rust futures are lazy -- they do nothing until you `.await` them or hand them to a runtime.

The Tokio runtime is the standard async runtime for Rust. chatixia-mesh uses it in both the registry and the sidecar.

### async fn and .await

```rust
// An async function returns a Future. Nothing happens until you .await it.
async fn fetch_data(url: &str) -> Result<String, Error> {
    let response = reqwest::get(url).await?;  // yield point
    let body = response.text().await?;         // yield point
    Ok(body)
}
```

### tokio::spawn -- fire-and-forget tasks

`tokio::spawn` takes a future and runs it as an independent task on the Tokio runtime. The task runs concurrently with the spawning code. This is how chatixia-mesh starts background work.

From the registry (`registry/src/main.rs`):

```rust
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // ... setup ...

    // Spawn background health checker -- runs forever alongside the server
    let reg = state.registry.clone();
    tokio::spawn(async move { reg.health_check_loop().await });

    // Spawn task expiry loop
    let hub = state.hub.clone();
    tokio::spawn(async move { hub.expire_tasks_loop().await });

    // Spawn pairing cleanup loop
    let pairing = state.pairing.clone();
    tokio::spawn(async move { pairing.cleanup_loop().await });

    // Start the HTTP server (also async)
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
```

Three background tasks and the HTTP server all run concurrently on the same Tokio runtime. No threads were explicitly created -- Tokio manages a small pool of worker threads internally and multiplexes all spawned tasks across them.

### Channels: tokio::sync::mpsc

Channels are the async equivalent of a thread-safe queue. One end sends messages, the other receives them. Tokio provides `mpsc` (multi-producer, single-consumer) channels.

```rust
use tokio::sync::mpsc;

#[tokio::main]
async fn main() {
    // Create an unbounded channel (no backpressure -- use with care)
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();

    // Producer: send a message (does not block, returns immediately)
    tx.send("hello".to_string()).unwrap();

    // Consumer: receive a message (yields until one arrives)
    if let Some(msg) = rx.recv().await {
        println!("got: {}", msg);
    }
}
```

In chatixia-mesh, the registry creates one channel per connected WebSocket peer (`signaling.rs`). When a signaling message needs to be relayed to a specific peer, the registry looks up that peer's sender (`tx`) in a `DashMap` and pushes the message into the channel. The WebSocket handler for that peer receives it from the other end (`rx`) and writes it to the socket. This decouples message production from delivery.

### tokio::select! -- multiplexing

`tokio::select!` waits on multiple async operations simultaneously and runs the branch for whichever completes first. This is the fundamental tool for multiplexing in Tokio.

From the registry's WebSocket handler (`registry/src/main.rs`):

```rust
async fn handle_ws(mut socket: WebSocket, peer_id: String, state: AppState) {
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    state.signaling.add_peer(&peer_id, tx);

    loop {
        tokio::select! {
            // Branch 1: a message arrived in the channel (from another handler)
            // -- forward it to this peer's WebSocket
            Some(msg) = rx.recv() => {
                if socket.send(Message::Text(msg.into())).await.is_err() {
                    break;
                }
            }
            // Branch 2: the peer sent a message over WebSocket
            // -- parse and handle it
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        // parse and relay signaling message
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
        }
    }
}
```

Without `select!`, you would need two separate tasks -- one reading from the channel, one reading from the WebSocket -- and some way to coordinate them. `select!` collapses this into a single, clear loop.

From the sidecar (`sidecar/src/main.rs`), `select!` monitors independent subsystems:

```rust
tokio::select! {
    _ = ipc_handle => { error!("[MAIN] IPC server exited"); }
    _ = sig_handle => { error!("[MAIN] signaling connection exited"); }
}
```

This waits for either the IPC server or the signaling connection to finish. If either one exits (which normally means an error), the entire sidecar shuts down. This is the async equivalent of "run these two things and stop if either fails."

---

## 4. Python Async: asyncio

Python's `asyncio` module provides an event loop, coroutines, and tasks for cooperative multitasking. chatixia-mesh uses it in the Python agent.

### Coroutines and await

```python
import asyncio

async def fetch_data(host: str, port: int) -> str:
    reader, writer = await asyncio.open_connection(host, port)  # yield
    writer.write(b"GET / HTTP/1.0\r\n\r\n")
    await writer.drain()         # yield: wait until OS buffer accepts data
    data = await reader.read()   # yield: wait for response
    writer.close()
    return data.decode()
```

### asyncio.create_task -- spawning concurrent work

`asyncio.create_task()` schedules a coroutine to run concurrently on the event loop. It returns a `Task` object (a handle to the running coroutine).

```python
import asyncio

async def timer(name: str, seconds: int):
    await asyncio.sleep(seconds)
    print(f"{name}: done after {seconds}s")

async def main():
    # These run concurrently, not sequentially
    task_a = asyncio.create_task(timer("A", 2))
    task_b = asyncio.create_task(timer("B", 1))

    # Wait for both to complete
    await task_a
    await task_b
    # Output (after ~2s total, not 3s):
    #   B: done after 1s
    #   A: done after 2s

asyncio.run(main())
```

### The event loop

`asyncio.run()` creates an event loop, runs the given coroutine to completion, and shuts down the loop. Inside a running loop, `asyncio.get_running_loop()` returns the current loop.

The event loop is the single-threaded scheduler. It decides which coroutine to resume when an I/O operation completes or a timer fires.

### How the Python agent uses asyncio

From the agent runner (`agent/chatixia/runner.py`):

```python
async def run_agent(config: AgentConfig) -> None:
    # ... setup ...

    client = MeshClient(socket_path=config.sidecar.socket)
    await client.start()

    # Register handler for incoming P2P task requests
    client.on("message", _handle_p2p_message)

    # Heartbeat loop -- runs forever
    while True:
        try:
            resp = requests.post(f"{registry}/api/hub/heartbeat", ...)
            for task in resp.json().get("pending_tasks", []):
                # Fire-and-forget: spawn task execution without blocking heartbeat
                asyncio.create_task(
                    _execute_task(registry, api_key, task, mesh_client=client)
                )
        except Exception as exc:
            logger.debug("heartbeat failed: %s", exc)
        await asyncio.sleep(15)  # yield for 15 seconds
```

Several things run concurrently here:

1. The `_listen_loop` inside `MeshClient` (started by `client.start()`) reads from the sidecar IPC socket.
2. The heartbeat loop sends heartbeats every 15 seconds.
3. Task execution coroutines spawned by `asyncio.create_task()` run skills.

All of this happens on a single Python thread.

---

## 5. Channels for Message Passing

### The pattern

Instead of sharing data structures between tasks and protecting them with locks, pass messages through channels. Each task owns its own data and communicates exclusively by sending and receiving messages.

This is sometimes called the "actor model" or "share memory by communicating" (the Go proverb).

### Rust: mpsc::unbounded_channel

Section 3 introduced the basic channel API. The key property for message passing is that the sender half (`tx`) can be **cloned**. This lets multiple producer tasks send into the same channel without shared mutable state:

```rust
use tokio::sync::mpsc;

#[tokio::main]
async fn main() {
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();

    // Clone the sender for each producer -- each task owns its own clone
    for i in 0..3 {
        let tx = tx.clone();
        tokio::spawn(async move {
            tx.send(format!("message from task {}", i)).unwrap();
        });
    }

    // Drop the original sender so the channel closes when all clones are dropped
    drop(tx);

    // Single consumer drains all messages
    while let Some(msg) = rx.recv().await {
        println!("{}", msg);
    }
}
```

Each producer task owns its own `tx` clone. No mutex, no shared reference. The channel itself handles the synchronization internally.

### Python: asyncio.Queue

Python's `asyncio.Queue` serves the same role for coroutines.

```python
import asyncio

async def producer(queue: asyncio.Queue):
    for i in range(5):
        await queue.put(f"item {i}")
        await asyncio.sleep(0.1)

async def consumer(queue: asyncio.Queue):
    while True:
        item = await queue.get()   # yields until an item is available
        print(f"processing: {item}")
        queue.task_done()

async def main():
    queue = asyncio.Queue()
    asyncio.create_task(producer(queue))
    asyncio.create_task(consumer(queue))
    await asyncio.sleep(1)

asyncio.run(main())
```

### Why chatixia-mesh uses channels

The sidecar has three subsystems that run concurrently:

1. **Signaling** -- WebSocket connection to the registry.
2. **IPC** -- Unix socket connection to the Python agent.
3. **WebRTC** -- Data channels to other sidecars.

These subsystems need to exchange messages. For example, when a message arrives on a DataChannel, it needs to reach the Python agent via IPC. Rather than having the DataChannel handler directly write to the IPC socket (which would require shared mutable access to the socket writer), the sidecar uses channels:

```
DataChannel handler --> [mpsc channel] --> IPC writer task
                                            |
                                            v
                                      Unix socket to Python
```

From the sidecar (`sidecar/src/main.rs`):

```rust
// Channel for messages flowing from mesh to the Python agent
let (to_agent_tx, to_agent_rx) = mpsc::unbounded_channel::<protocol::IpcMessage>();

// IPC server receives from this channel and writes to Unix socket
let ipc_handle = tokio::spawn(async move {
    ipc::serve(&ipc_socket_path, to_agent_rx, ipc_mesh, ipc_to_agent_tx).await
});

// Signaling (and WebRTC handlers) send to this channel
let sig_handle = tokio::spawn(async move {
    signaling::run(&ws_url, &peer_id, sig_tx, sig_rx, mesh, to_agent_tx).await
});
```

The `to_agent_tx` sender is passed into the signaling module. When a DataChannel message arrives from another sidecar, the signaling/WebRTC code calls `to_agent_tx.send(ipc_message)`. The IPC server task receives it and writes it to the Unix socket. No shared mutable state. No locks. Just a message in a channel.

The same pattern exists on the registry side. Each WebSocket peer gets its own channel. The signaling handler writes relay messages into the target peer's channel. The WebSocket writer task drains the channel into the socket.

---

## 6. Concurrent Data Structures

### The problem with HashMap + Mutex

The most straightforward way to share a map between async tasks is wrapping it in a `Mutex`:

```rust
use std::sync::Mutex;
use std::collections::HashMap;

let map = Arc::new(Mutex::new(HashMap::<String, AgentRecord>::new()));
```

This works but has a bottleneck: every read and every write locks the entire map. If 100 WebSocket handlers all need to look up or update agent records, they serialize on this single lock. Under load, most time is spent waiting for the lock rather than doing work.

### DashMap: sharded concurrent HashMap

`DashMap` is a concurrent hash map that uses **sharding** internally. Instead of one lock for the entire map, it partitions the keys into multiple shards, each with its own lock. Two operations on keys in different shards proceed in parallel without contention.

```rust
use dashmap::DashMap;

let map = DashMap::<String, AgentRecord>::new();

// No explicit locking needed -- DashMap handles it internally
map.insert("agent-1".to_string(), record);

if let Some(entry) = map.get("agent-1") {
    println!("found: {}", entry.info.agent_id);
}

// Iteration also works without holding a single global lock
for entry in map.iter() {
    println!("{}: {}", entry.key(), entry.value().health);
}
```

### DashMap in chatixia-mesh

The registry uses five `DashMap` instances across its state modules:

| State module | DashMap key | DashMap value | Purpose |
|---|---|---|---|
| `SignalingState` | peer_id (`String`) | `UnboundedSender<String>` | WebSocket sender for each connected peer |
| `RegistryState` | agent_id (`String`) | `AgentRecord` | Registered agents with health status |
| `HubState` | task_id (`String`) | `Task` | Task queue with lifecycle tracking |
| `PairingState` | invite code (`String`) | `InviteCode` | Ephemeral 6-digit invite codes |
| `PairingState` | entry_id (`String`) | `OnboardingEntry` | Agent onboarding lifecycle |

Each `DashMap` is wrapped in an `Arc` (via the containing state struct) and shared across all request handlers. An HTTP handler for heartbeat can update `RegistryState.agents` while a WebSocket handler simultaneously reads from `SignalingState.peers` with no contention between them. Even within a single `DashMap`, operations on different keys are unlikely to contend because they hit different shards.

This is the code that creates all five shared state containers (`registry/src/main.rs`):

```rust
let state = AppState {
    auth: Arc::new(AuthState::new(&signaling_secret)),
    signaling: Arc::new(SignalingState::new()),   // DashMap<String, UnboundedSender>
    registry: Arc::new(RegistryState::new()),     // DashMap<String, AgentRecord>
    hub: Arc::new(HubState::new()),               // DashMap<String, Task>
    pairing: Arc::new(PairingState::new()),       // 2x DashMap + rate limiter DashMap
};
```

### When to use locks vs. lock-free structures

| Approach | Use when | Trade-off |
|---|---|---|
| `Mutex<HashMap>` | Low contention (few concurrent readers/writers), simple access patterns | Simple code, but serializes all access |
| `RwLock<HashMap>` | Many readers, few writers | Readers don't block each other, but writers still block everyone |
| `DashMap` | High contention, many concurrent readers and writers on different keys | More memory (shards), but operations on different keys run in parallel |
| Channels (no shared map) | Data flows in one direction between tasks | Most contention-free, but restructures how you think about data |

The registry chose `DashMap` because HTTP request handlers and WebSocket connections run concurrently on the Tokio runtime. Each handler needs read or write access to the agent map, task map, or peer map. With a `Mutex<HashMap>`, a slow heartbeat handler would block all other handlers. With `DashMap`, a heartbeat updating agent "A" does not block a handler reading agent "B."

---

## Exercises

### Exercise 1: Tokio select! with two channels

Write a Tokio program that creates two `mpsc::unbounded_channel` instances. Spawn one task that sends a number to channel A every 500ms and another that sends a string to channel B every 700ms. In the main task, use `tokio::select!` to receive from both channels and print each message as it arrives. Run for 3 seconds, then exit.

Expected output (approximate timing):

```
[0.5s] channel A: 1
[0.7s] channel B: "hello"
[1.0s] channel A: 2
[1.4s] channel B: "hello"
[1.5s] channel A: 3
...
```

Starter structure:

```rust
use tokio::sync::mpsc;
use tokio::time::{sleep, Duration};

#[tokio::main]
async fn main() {
    let (tx_a, mut rx_a) = mpsc::unbounded_channel::<i32>();
    let (tx_b, mut rx_b) = mpsc::unbounded_channel::<String>();

    // TODO: spawn producer for channel A (sends i32 every 500ms)
    // TODO: spawn producer for channel B (sends String every 700ms)

    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);

    loop {
        if tokio::time::Instant::now() >= deadline {
            break;
        }

        tokio::select! {
            // TODO: receive from rx_a
            // TODO: receive from rx_b
        }
    }
}
```

### Exercise 2: asyncio concurrent coroutines

Write a Python asyncio program with two concurrent coroutines:

1. A **timer** coroutine that prints "tick" every 1 second, 5 times.
2. A **listener** coroutine that reads lines from an `asyncio.Queue` and prints each one.

Push three messages into the queue at 0.5s intervals from a third "producer" coroutine. All three should run concurrently using `asyncio.create_task()`.

Starter structure:

```python
import asyncio

async def timer(count: int):
    # TODO: print "tick" every 1 second, `count` times
    pass

async def listener(queue: asyncio.Queue):
    # TODO: read from queue and print each message
    # Exit after receiving "STOP"
    pass

async def producer(queue: asyncio.Queue):
    # TODO: put 3 messages into the queue at 0.5s intervals
    # Then put "STOP"
    pass

async def main():
    queue = asyncio.Queue()
    # TODO: create_task for all three coroutines
    # TODO: await all tasks

asyncio.run(main())
```

### Exercise 3: Why create_task instead of await?

In the agent's heartbeat loop (`agent/chatixia/runner.py`), task execution uses `asyncio.create_task()` instead of `await`:

```python
while True:
    resp = requests.post(f"{registry}/api/hub/heartbeat", ...)
    for task in resp.json().get("pending_tasks", []):
        asyncio.create_task(
            _execute_task(registry, api_key, task, mesh_client=client)
        )
    await asyncio.sleep(15)
```

Answer the following:

1. What would happen if the code used `await _execute_task(...)` instead of `asyncio.create_task(...)`?
2. Suppose the heartbeat returns 3 pending tasks, each taking 10 seconds to execute. Under the current code, how long before the next heartbeat fires? Under the `await` alternative?
3. Is there a risk to using `create_task` here? What happens if a task raises an exception?

### Exercise 4: DashMap vs. HashMap with Mutex

The registry stores agents in a `DashMap<String, AgentRecord>`. Consider replacing it with `HashMap<String, Mutex<AgentRecord>>` (each agent record gets its own mutex):

```rust
// Current
agents: DashMap<String, AgentRecord>

// Alternative
agents: Mutex<HashMap<String, Mutex<AgentRecord>>>
```

Answer the following:

1. Why does the alternative need two levels of locking (one outer `Mutex` for the `HashMap` itself, one inner `Mutex` per value)?
2. What operations require locking the outer `Mutex`? What operations only need the inner `Mutex`?
3. The `health_check_loop` iterates over all agents every 15 seconds and updates each one's `health` field. How does this behave differently under `DashMap` vs. the two-level `Mutex` approach?
4. Could you use `RwLock<HashMap<String, AgentRecord>>` instead? What is the trade-off compared to `DashMap`?
