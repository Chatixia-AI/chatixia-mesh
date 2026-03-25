# Lesson 17: Testing Distributed Systems -- From Unit Tests to End-to-End Validation

**Prerequisites:** [Lesson 05: Signaling Protocol Design](05-signaling-protocol-design.md), [Lesson 06: Inter-Process Communication](06-inter-process-communication.md), [Lesson 07: Application Protocol Design](07-application-protocol-design.md)

**Time estimate:** 75-90 minutes

**Key source files:**
- `sidecar/src/protocol.rs` -- Protocol structs and serialization round-trip tests
- `registry/src/auth.rs` -- AuthState, JWT token issuance and validation tests
- `registry/src/hub.rs` -- HubState, task lifecycle tests
- `agent/tests/test_mesh_client.py` -- MeshMessage and MeshClient unit tests
- `agent/tests/test_mesh_skills.py` -- Skill handler tests with mock MeshClient
- `agent/tests/test_runner.py` -- Runner env var derivation and registration tests
- `agent/tests/test_cli.py` -- CLI subcommand tests
- `agent/tests/test_scaffold.py` -- Scaffold file generation tests
- `agent/tests/test_config.py` -- Config parsing and validation tests
- `.github/workflows/ci.yml` -- CI pipeline for Rust, Python, Hub, version bumps, Docker
- `.github/workflows/publish-pypi.yml` -- PyPI publishing via OIDC trusted publisher
- `docs/ADR.md` -- ADR-013: Heartbeat-driven task execution

---

## Introduction

A distributed system is a collection of independent processes that communicate over a network to achieve a shared goal. Testing such a system is harder than testing a monolith because bugs can hide at any boundary: serialization, network transport, process coordination, timing, or state synchronization across components.

chatixia-mesh spans three languages (Rust, Python, TypeScript), four components (registry, sidecar, agent, hub), and three communication channels (WebSocket, WebRTC DataChannel, Unix socket IPC). A unit test that proves `MeshMessage` serializes correctly tells you nothing about whether the heartbeat loop actually processes the tasks it receives. A test that proves `AuthState` validates tokens tells you nothing about whether the sidecar includes the token in WebSocket headers.

This lesson examines the testing strategy chatixia-mesh uses -- and the testing gap it discovered the hard way during a live E2E session.

---

## 1. The Testing Pyramid for Distributed Systems

The classic testing pyramid applies to distributed systems, but the shape shifts. The base (unit tests) remains wide, but the middle layer (integration tests) and the top (end-to-end tests) carry more weight than in monolithic applications because the most dangerous bugs live at component boundaries.

```
                         /\
                        /  \
                       / E2E \        Full mesh: registry + 2 sidecars + 2 agents
                      / tests \       Expensive, slow, catches boundary bugs
                     /----------\
                    /            \
                   / Integration  \   Multi-component: signaling flow,
                  /    tests       \  IPC round-trips, skill handler + mock client
                 /------------------\
                /                    \
               /     Unit tests       \  Protocol parsing, state management,
              /                        \ config validation, token lifecycle
             /__________________________\
```

### Unit tests (base)

Unit tests verify individual functions, structs, and modules in isolation. They are fast, deterministic, and cheap to write. In chatixia-mesh, unit tests cover:

- **Protocol serialization** -- `SignalingMessage`, `MeshMessage`, `IpcMessage` round-trips (Rust)
- **State management** -- `HubState` task creation, assignment, expiration (Rust)
- **Authentication** -- `AuthState` token issuance, validation, expiration (Rust)
- **Config parsing** -- `AgentConfig` loading, validation, path resolution (Python)
- **Message conversion** -- `MeshMessage.to_dict()` / `from_dict()` round-trips (Python)
- **Client state** -- `MeshClient` peer tracking, handler registration (Python)

Unit tests cannot catch bugs that occur when components interact. A `MeshMessage` that serializes correctly in Rust may be misinterpreted by the Python deserializer if the two sides disagree on default values or field names.

### Integration tests (middle)

Integration tests verify that two or more components work together correctly. They are slower and more complex than unit tests because they require setting up dependencies -- but they can use mocks to avoid standing up the entire system. In chatixia-mesh, integration tests cover:

- **Skill handlers with mock MeshClient** -- `handle_delegate` P2P path, fallback path, fire-and-forget path
- **Runner registration with mock HTTP** -- `_register` sends correct payload and headers
- **Task update with mock HTTP** -- `_update_task` reports results, handles network failures
- **CLI subcommands with filesystem** -- `init` creates scaffolds, `validate` checks manifests

Integration tests catch a large class of real bugs: incorrect HTTP headers, malformed payloads, wrong URL construction, missing error handling. They are the best return-on-investment for most distributed system bugs.

### End-to-end tests (top)

E2E tests run the full system: registry, sidecars, agents, and (optionally) the hub. They verify that the entire message flow works from initiation to completion. They are the most expensive to write and maintain, but they catch a category of bugs that nothing else can: **cross-component state synchronization failures**.

The heartbeat bug (Section 3) is a textbook example: every component passed its own unit and integration tests, but the system as a whole failed to execute tasks because the agent discarded the registry's heartbeat response body.

### Why E2E is essential but expensive

E2E tests require:

- **Multiple processes** -- at minimum a registry, one sidecar, and one agent
- **Real network I/O** -- WebSocket connections, HTTP endpoints, optionally WebRTC
- **Timing sensitivity** -- heartbeat intervals, connection setup delays, task polling
- **Complex setup and teardown** -- process lifecycle, port allocation, cleanup

In chatixia-mesh, E2E testing is currently done manually with 2 agents against a running registry (documented in session notes). Automating this requires orchestration tooling that has not yet been built.

The insight: **you cannot skip E2E testing for distributed systems, but you can minimize how often you need it** by pushing as many boundary-crossing tests as possible into the integration tier.

---

## 2. Unit Testing Protocol Code

Protocol code is the best place to start testing because it has clear inputs, clear outputs, and no side effects. The pattern is always the same: construct a message, serialize it, deserialize it, verify the result matches the original.

### Serialization round-trips in Rust

The `sidecar/src/protocol.rs` test module demonstrates the standard approach for testing serde types. Each protocol struct gets three categories of tests:

**1. Serialize and verify JSON structure:**

```rust
// sidecar/src/protocol.rs

#[test]
fn test_signaling_message_serialize_with_target() {
    let msg = SignalingMessage {
        msg_type: "offer".into(),
        peer_id: "peer-1".into(),
        target_id: Some("peer-2".into()),
        payload: serde_json::json!({"sdp": "..."}),
    };
    let json: serde_json::Value = serde_json::to_value(&msg).unwrap();
    assert_eq!(json["type"], "offer");
    assert_eq!(json["peer_id"], "peer-1");
    assert_eq!(json["target_id"], "peer-2");
    // "msg_type" key must NOT appear (renamed to "type")
    assert!(json.get("msg_type").is_none());
}
```

This test catches a subtle but critical issue: the `#[serde(rename = "type")]` attribute on `msg_type`. If the rename annotation is removed, the JSON key would be `msg_type` instead of `type`, and every other component that expects `type` would break. The assertion `json.get("msg_type").is_none()` explicitly guards against this regression.

**2. Deserialize and verify defaults:**

```rust
#[test]
fn test_mesh_message_deserialize_defaults() {
    // Only "type" is required; everything else should default.
    let raw = r#"{"type":"ping"}"#;
    let msg: MeshMessage = serde_json::from_str(raw).unwrap();
    assert_eq!(msg.msg_type, "ping");
    assert_eq!(msg.request_id, "");
    assert_eq!(msg.source_agent, "");
    assert_eq!(msg.target_agent, "");
    assert_eq!(msg.payload, serde_json::Value::Null);
}
```

This test verifies the `#[serde(default)]` annotations on `MeshMessage` fields. A `ping` message legitimately contains only a `type` field. Without `default`, deserializing this JSON would fail with a missing field error, breaking the simplest possible message in the protocol.

**3. Full round-trip:**

```rust
#[test]
fn test_signaling_message_roundtrip() {
    let original = SignalingMessage {
        msg_type: "ice".into(),
        peer_id: "a".into(),
        target_id: Some("b".into()),
        payload: serde_json::json!({"candidate": "c1"}),
    };
    let json_str = serde_json::to_string(&original).unwrap();
    let decoded: SignalingMessage = serde_json::from_str(&json_str).unwrap();
    assert_eq!(decoded.msg_type, original.msg_type);
    assert_eq!(decoded.peer_id, original.peer_id);
    assert_eq!(decoded.target_id, original.target_id);
    assert_eq!(decoded.payload, original.payload);
}
```

Round-trip tests are the most important category. They prove that no data is lost or corrupted during the serialize-deserialize cycle. If a field is accidentally marked `#[serde(skip)]`, a round-trip test catches it immediately.

### The same pattern in Python

The Python side mirrors the Rust tests for `MeshMessage`:

```python
# agent/tests/test_mesh_client.py

class TestMeshMessage:
    def test_to_dict(self):
        msg = MeshMessage(
            msg_type="task_request",
            request_id="req-123",
            source_agent="agent-a",
            target_agent="agent-b",
            payload={"key": "value"},
        )
        d = msg.to_dict()
        assert d["type"] == "task_request"
        assert d["request_id"] == "req-123"

    def test_from_dict_defaults(self):
        msg = MeshMessage.from_dict({"type": "ping"})
        assert msg.msg_type == "ping"
        assert msg.request_id == ""
        assert msg.payload == {}

    def test_roundtrip(self):
        original = MeshMessage(
            msg_type="agent_prompt",
            request_id="abc",
            source_agent="s",
            target_agent="t",
            payload={"nested": {"deep": True}},
        )
        rebuilt = MeshMessage.from_dict(original.to_dict())
        assert rebuilt.msg_type == original.msg_type
        assert rebuilt.request_id == original.request_id
        assert rebuilt.payload == original.payload
```

The Python `MeshMessage` defaults differ slightly from Rust -- `payload` defaults to `{}` (empty dict) in Python but `serde_json::Value::Null` in Rust. This is intentional (Python callers expect a dict), but it illustrates why cross-language protocol testing matters. If a test on one side assumes a default that differs from the other side, the messages will be misinterpreted in production.

### AuthState token validation

The `registry/src/auth.rs` test module demonstrates testing a stateful component with security implications. The tests cover the three critical token validation scenarios:

```rust
// registry/src/auth.rs

#[test]
fn test_issue_and_validate_token() {
    let auth = AuthState::new("test-secret");
    let token = auth.issue_token("peer-1", "agent").unwrap();
    let claims = auth.validate_token(&token).unwrap();
    assert_eq!(claims.sub, "peer-1");
    assert_eq!(claims.role, "agent");
    assert!(claims.exp > claims.iat);
}

#[test]
fn test_validate_expired_token() {
    let auth = AuthState::new("test-secret");
    // Manually craft a token with past expiry
    let past = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as usize
        - 600;
    let claims = Claims {
        sub: "peer-1".into(),
        role: "agent".into(),
        iat: past,
        exp: past + 1, // expired 599s ago
    };
    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(b"test-secret"),
    )
    .unwrap();
    assert!(auth.validate_token(&token).is_err());
}

#[test]
fn test_validate_wrong_secret() {
    let auth_a = AuthState::new("secret-a");
    let auth_b = AuthState::new("secret-b");
    let token = auth_a.issue_token("peer-1", "agent").unwrap();
    assert!(auth_b.validate_token(&token).is_err());
}
```

Three scenarios, three guarantees:

| Test | What it proves |
|------|---------------|
| `test_issue_and_validate_token` | A freshly issued token is valid and contains the correct claims |
| `test_validate_expired_token` | An expired token is rejected (time-based expiry works) |
| `test_validate_wrong_secret` | A token from a different secret is rejected (HMAC verification works) |

The expired token test is worth examining closely. It does not wait for a real token to expire (that would take 5 minutes). Instead, it manually crafts a JWT with past timestamps using the same `encode` function that `issue_token` uses internally. This technique -- constructing invalid inputs that would be hard to produce through the normal API -- is essential for testing security boundaries.

### HubState task lifecycle

The `registry/src/hub.rs` test module tests task state transitions, which are the core state machine of the system:

```rust
// registry/src/hub.rs

#[test]
fn test_get_pending_marks_assigned() {
    let hub = HubState::new();
    hub.tasks
        .insert("t1".into(), make_task("t1", "search", "", "pending"));
    let result = hub.get_pending_for_agent("a1", &["search".to_string()]);
    assert_eq!(result[0].state, "assigned");
    // Verify it's also updated in the map
    assert_eq!(hub.tasks.get("t1").unwrap().state, "assigned");
}

#[test]
fn test_get_pending_skips_completed() {
    let hub = HubState::new();
    hub.tasks
        .insert("t1".into(), make_task("t1", "search", "a1", "completed"));
    let result = hub.get_pending_for_agent("a1", &["search".to_string()]);
    assert!(result.is_empty());
}
```

These tests verify that `get_pending_for_agent` atomically transitions tasks from `pending` to `assigned` -- and that it does not re-assign tasks that are already `completed`. The atomicity is important because `DashMap` (the concurrent hash map used by `HubState`) allows multiple heartbeat handlers to call `get_pending_for_agent` concurrently.

The helper function `make_task` creates test fixtures with sensible defaults:

```rust
fn make_task(id: &str, skill: &str, target: &str, state: &str) -> Task {
    let now = epoch_now();
    Task {
        id: id.to_string(),
        skill: skill.to_string(),
        target_agent_id: target.to_string(),
        source_agent_id: "src".into(),
        assigned_agent_id: String::new(),
        payload: serde_json::json!({}),
        state: state.to_string(),
        result: String::new(),
        error: String::new(),
        created_at: now,
        updated_at: now,
        ttl: 300,
    }
}
```

Test fixture helpers reduce boilerplate and make tests easier to read. The reader immediately sees that `make_task("t1", "search", "a1", "completed")` creates a completed task with skill `search` targeted at agent `a1`. The irrelevant fields (`source_agent_id`, `payload`, `ttl`) are filled with defaults.

---

## 3. The E2E Gap -- A Cautionary Tale

Session 4 of chatixia-mesh development ran a full E2E test: 1 registry, 2 agents (alpha and beta), each with a WebRTC sidecar. The session systematically tested every layer of the stack.

### What passed

| Layer | Test | Result |
|-------|------|--------|
| Registration | Both agents register with registry | Both visible in `/api/registry/agents` |
| Authentication | API keys exchange for JWTs | Valid tokens issued for both agents |
| Signaling | SDP offer/answer relayed | WebRTC connection established |
| DataChannel | ICE candidates exchanged | Bidirectional mesh edge formed |
| Task submission | POST task targeting agent-beta | Task created with state `pending` |
| Task assignment | Beta's heartbeat claims task | Task transitions to `assigned` |

Everything worked. Every unit test passed. Every component did its job.

### What failed

When the E2E test submitted a `list_agents` task targeting agent-beta, the task was created (`pending`), claimed on beta's next heartbeat (`assigned`) -- and then nothing happened. The task sat in `assigned` state indefinitely. Agent-beta never executed it.

### The root cause

The heartbeat loop in `runner.py` fired an HTTP POST to `/api/hub/heartbeat` every 15 seconds. The registry responded with a JSON body containing `pending_tasks` -- an array of tasks freshly assigned to this agent. But the runner discarded the response:

```python
# runner.py -- BEFORE the fix (Session 4)
# The heartbeat loop sent the POST but ignored the response body.
# Tasks transitioned from pending -> assigned server-side,
# but the agent never saw them.
resp = requests.post(f"{registry}/api/hub/heartbeat", json={...})
# resp.json() was never called -- the response body was discarded
```

The registry did its part: it assigned the task and returned it in the response. But the agent never read the response. The task was assigned to an agent that would never execute it.

### Why unit tests missed it

Every individual component passed its tests:

- `HubState.get_pending_for_agent` correctly transitions tasks to `assigned` and returns them -- tested in `registry/src/hub.rs`.
- `SKILL_HANDLERS` maps skill names to handler functions -- the handlers are tested in `agent/tests/test_mesh_skills.py`.
- The heartbeat HTTP POST sends the correct payload -- verifiable by mocking `requests.post`.

The bug was not in any single component. It was in the **seam** between the registry's heartbeat response and the runner's heartbeat loop. The registry returned data; the runner ignored it. No unit test covered this seam because it spans two processes written in two languages.

### The fix (ADR-013)

ADR-013 modified the heartbeat loop to parse and execute tasks from the response:

```python
# runner.py -- AFTER the fix (ADR-013)
while True:
    try:
        resp = requests.post(
            f"{registry}/api/hub/heartbeat",
            json={
                "agent_id": agent_id,
                "hostname": socket.gethostname(),
                "sidecar_peer_id": f"{agent_id}-sidecar",
                "skill_names": config.skills_builtin,
            },
            headers={"x-api-key": api_key},
            timeout=5,
        )
        body = resp.json()
        for task in body.get("pending_tasks", []):
            asyncio.create_task(
                _execute_task(registry, api_key, task, mesh_client=client)
            )
    except Exception as exc:
        logger.debug("heartbeat failed: %s", exc)
    await asyncio.sleep(15)
```

The key changes:

1. `resp.json()` is called to parse the response body.
2. `body.get("pending_tasks", [])` extracts the task array.
3. Each task is dispatched to `_execute_task`, which looks up the skill handler and POSTs the result back to the hub.
4. `asyncio.create_task` runs each task concurrently so long-running skills do not block the heartbeat loop.

### The lesson

The heartbeat bug is a textbook example of why E2E testing is essential for distributed systems. The bug was invisible to:

- **Unit tests** -- each function worked correctly in isolation.
- **Integration tests** -- the runner's registration and env var derivation were tested with mocks.
- **Code review** -- the heartbeat POST looked correct; the missing `resp.json()` was a sin of omission, not commission.

Only running two actual agents against a real registry and submitting a real task revealed that the last mile -- parsing and acting on the response -- was missing.

**Guideline:** For any distributed system, identify the seams where data crosses process boundaries. Write at least one test per seam that verifies the receiving side acts on what the sending side provides. If you cannot write that test at the integration level, you must test it E2E.

---

## 4. Testing Async Code

chatixia-mesh is async on both sides: Rust uses `tokio`, Python uses `asyncio`. Testing async code requires special test runners and patterns.

### Rust: `#[tokio::test]`

In Rust, the `#[tokio::test]` attribute replaces `#[test]` for async test functions. It sets up a tokio runtime that drives the async code:

```rust
#[tokio::test]
async fn test_channel_send_receive() {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(10);
    tx.send("hello".into()).await.unwrap();
    let received = rx.recv().await.unwrap();
    assert_eq!(received, "hello");
}
```

The `#[tokio::test]` macro creates a single-threaded runtime by default. For tests that require multiple tasks running concurrently (e.g., testing a server and client), use `#[tokio::test(flavor = "multi_thread")]`.

**Testing timeouts:**

```rust
#[tokio::test]
async fn test_operation_timeout() {
    let result = tokio::time::timeout(
        Duration::from_millis(100),
        async {
            tokio::time::sleep(Duration::from_secs(10)).await;
            "should not reach here"
        },
    )
    .await;
    assert!(result.is_err()); // Elapsed error
}
```

### Python: `pytest-asyncio`

In Python, `pytest-asyncio` provides the `@pytest.mark.asyncio` decorator for async test functions. The test runner creates an event loop and drives the coroutine:

```python
import pytest

@pytest.mark.asyncio
async def test_async_handler():
    result = await handle_delegate(message="hello")
    assert "Error" in result
```

chatixia-mesh's `test_mesh_skills.py` uses this pattern extensively. Every P2P path test is async because the skill handlers are async functions:

```python
# agent/tests/test_mesh_skills.py

class TestHandleDelegate:
    @pytest.mark.asyncio
    async def test_p2p_path_fire_and_forget(self):
        mock_client = MagicMock()
        mock_client.connected = True
        mock_client.is_peer_connected.return_value = True
        mock_client.send = AsyncMock()

        result = await handle_delegate(
            message="do something",
            target_agent_id="agent-b",
            skill="research",
            wait=False,
            _mesh_client=mock_client,
        )
        assert "P2P" in result
        mock_client.send.assert_called_once()
```

Key patterns:

- **`MagicMock` for sync properties** -- `connected` and `is_peer_connected` are sync, so regular `MagicMock` works.
- **`AsyncMock` for async methods** -- `send` and `request` are async, so they need `AsyncMock` from `unittest.mock`.
- **`_mesh_client` parameter injection** -- The skill handlers accept an optional `_mesh_client` parameter for testability. In production, the runner passes the real `MeshClient` instance. In tests, a mock is injected.

### Testing async channels and concurrent operations

When testing components that communicate through channels, you need to run both sides concurrently. In Rust:

```rust
#[tokio::test]
async fn test_concurrent_send_receive() {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(10);

    // Spawn sender
    let sender = tokio::spawn(async move {
        for i in 0..5 {
            tx.send(format!("msg-{}", i)).await.unwrap();
        }
    });

    // Collect results
    let mut received = Vec::new();
    for _ in 0..5 {
        received.push(rx.recv().await.unwrap());
    }
    sender.await.unwrap();

    assert_eq!(received.len(), 5);
    assert_eq!(received[0], "msg-0");
}
```

In Python, `asyncio.create_task` and `asyncio.gather` serve the same purpose:

```python
@pytest.mark.asyncio
async def test_concurrent_operations():
    results = []

    async def producer(queue):
        for i in range(5):
            await queue.put(f"item-{i}")

    async def consumer(queue):
        for _ in range(5):
            item = await queue.get()
            results.append(item)

    queue = asyncio.Queue()
    await asyncio.gather(producer(queue), consumer(queue))
    assert len(results) == 5
```

---

## 5. Integration Test Strategies

Integration tests verify that two or more components work together. The key skill is choosing what to mock and what to run for real. Mock too much and you are writing unit tests. Mock too little and you have a slow, flaky E2E test.

### Strategy 1: Test skill handlers with mock MeshClient

The most valuable integration tests in chatixia-mesh are the skill handler tests in `test_mesh_skills.py`. They test the full handler logic -- argument validation, path selection, message construction, response parsing -- while mocking only the network layer (MeshClient).

**Testing the P2P path:**

```python
# agent/tests/test_mesh_skills.py

@pytest.mark.asyncio
async def test_p2p_path_request_response(self):
    """P2P delegate with wait=True uses request() for response."""
    mock_client = MagicMock()
    mock_client.connected = True
    mock_client.is_peer_connected.return_value = True
    mock_client.request = AsyncMock(
        return_value={"payload": {"result": "done", "error": ""}}
    )

    result = await handle_delegate(
        message="do something",
        target_agent_id="agent-b",
        wait=True,
        _mesh_client=mock_client,
    )
    assert result == "done"
    mock_client.request.assert_called_once()
```

This test verifies that:
1. When the mesh client is connected and the peer is reachable, the P2P path is taken.
2. With `wait=True`, the handler calls `request()` (not `send()`).
3. The response payload's `result` field is extracted and returned.

**Testing the fallback path:**

```python
@pytest.mark.asyncio
async def test_fallback_when_peer_not_connected(self):
    """When peer is not reachable via P2P, falls back to registry."""
    mock_client = MagicMock()
    mock_client.connected = True
    mock_client.is_peer_connected.return_value = False

    result = await handle_delegate(
        message="hello", target_agent_id="agent-b", _mesh_client=mock_client
    )
    assert isinstance(result, str)
```

The fallback path is tested by setting `is_peer_connected` to `False`. The handler detects that the target peer is not directly reachable and falls back to the HTTP task queue. Without a running registry, the HTTP call fails, but the handler catches the exception and returns an error string rather than raising.

**Testing the broadcast path:**

```python
@pytest.mark.asyncio
async def test_p2p_path(self):
    """When MeshClient is connected with peers, broadcast via P2P."""
    mock_client = MagicMock()
    mock_client.connected = True
    mock_client.peers = {"peer-a-sidecar", "peer-b-sidecar"}
    mock_client.broadcast = AsyncMock()

    result = await handle_mesh_broadcast(
        message="hello all",
        _mesh_client=mock_client,
    )
    assert "P2P DataChannel" in result
    mock_client.broadcast.assert_called_once()
```

### Strategy 2: Test registration with mock HTTP

The runner tests in `test_runner.py` mock the `requests.post` function to test registration without a live registry:

```python
# agent/tests/test_runner.py

class TestRegisterHelper:
    @patch("chatixia.runner.requests.post")
    def test_register_sends_correct_payload(self, mock_post):
        from chatixia.runner import _register

        mock_resp = MagicMock()
        mock_resp.raise_for_status = MagicMock()
        mock_post.return_value = mock_resp

        config = AgentConfig(
            name="reg-agent",
            skills_builtin=["delegate", "mesh_send"],
        )
        _register("http://localhost:8080", "ak_test", "reg-agent", config)

        mock_post.assert_called_once()
        call_kwargs = mock_post.call_args
        json_body = call_kwargs.kwargs.get("json") or call_kwargs[1].get("json")
        assert json_body["agent_id"] == "reg-agent"
        assert json_body["capabilities"]["skills"] == ["delegate", "mesh_send"]
```

This test verifies the exact HTTP payload the agent sends to the registry during registration. If someone changes the JSON field names, this test catches it immediately -- even without a running registry.

### Strategy 3: Test error handling in task updates

```python
# agent/tests/test_runner.py

class TestUpdateTask:
    @patch("chatixia.runner.requests.post")
    def test_update_task_swallows_network_errors(self, mock_post):
        from chatixia.runner import _update_task

        mock_post.side_effect = ConnectionError("network down")
        # Should not raise
        _update_task("http://localhost:8080", "ak_test", "task-789", "failed",
                     error="oops")
```

This test verifies that `_update_task` catches network exceptions rather than propagating them. In a distributed system, network failures are expected. A task update failure should not crash the heartbeat loop.

### Strategy 4: Test env var derivation

The runner derives WebSocket and token URLs from the registry HTTP URL. This URL transformation logic is a common source of bugs:

```python
# agent/tests/test_runner.py

class TestEnvVarDerivation:
    def test_https_registry(self, monkeypatch):
        env = self._run_env_setup(monkeypatch, "https://mesh.example.com")
        assert env["SIGNALING_URL"] == "wss://mesh.example.com/ws"
        assert env["TOKEN_URL"] == "https://mesh.example.com/api/token"

    def test_trailing_slash_stripped(self, monkeypatch):
        env = self._run_env_setup(monkeypatch, "http://localhost:8080/")
        assert env["SIGNALING_URL"] == "ws://localhost:8080/ws"
```

These tests catch protocol scheme mismatches (`http` vs `ws`, `https` vs `wss`) and trailing slash issues. Both bugs would cause the sidecar to fail to connect, but only at runtime when the sidecar tries to open a WebSocket.

---

## 6. CI for Multi-Language Projects

chatixia-mesh's CI pipeline (`.github/workflows/ci.yml`) runs on every push to `main` and every pull request. It tests all three languages in parallel.

### Pipeline structure

```
ci.yml
  |
  +-- rust-lint         cargo fmt --check && cargo clippy
  +-- rust-test         cargo test --workspace
  +-- python-lint       ruff check . && ruff format --check .
  +-- python-test       uv sync --all-groups && uv run pytest -v
  +-- hub               pnpm install && tsc --noEmit && pnpm build
  +-- python-version-check   (PRs only) version bump enforcement
  +-- docker            (PRs only) docker build for registry, sidecar, agent
```

### Rust checks

```yaml
# .github/workflows/ci.yml (excerpt)

rust-lint:
  name: Rust Lint
  runs-on: ubuntu-latest
  steps:
    - uses: actions/checkout@v4
    - uses: dtolnay/rust-toolchain@stable
      with:
        components: rustfmt, clippy
    - uses: Swatinem/rust-cache@v2
    - name: cargo fmt
      run: cargo fmt --all -- --check
    - name: cargo clippy
      run: cargo clippy --workspace --all-targets -- -D warnings

rust-test:
  name: Rust Test
  runs-on: ubuntu-latest
  steps:
    - uses: actions/checkout@v4
    - uses: dtolnay/rust-toolchain@stable
    - uses: Swatinem/rust-cache@v2
    - name: cargo test
      run: cargo test --workspace
```

The Rust pipeline has two jobs:

1. **Lint** -- `cargo fmt --check` enforces formatting, `cargo clippy` catches common mistakes. The `RUSTFLAGS: "-Dwarnings"` env var (set at the workflow level) promotes warnings to errors, so the build fails if clippy finds anything.
2. **Test** -- `cargo test --workspace` runs all unit tests across the `registry` and `sidecar` crates. This includes the protocol, auth, and hub tests discussed in Section 2.

`Swatinem/rust-cache@v2` caches the compiled dependencies between runs, reducing build times from ~3 minutes to ~30 seconds for incremental changes.

### Python checks

```yaml
python-lint:
  name: Python Lint
  runs-on: ubuntu-latest
  defaults:
    run:
      working-directory: agent
  steps:
    - uses: actions/checkout@v4
    - uses: astral-sh/setup-uv@v6
    - name: ruff check
      run: uvx ruff@0.15.0 check .
    - name: ruff format
      run: uvx ruff@0.15.0 format --check .

python-test:
  name: Python Test
  runs-on: ubuntu-latest
  defaults:
    run:
      working-directory: agent
  steps:
    - uses: actions/checkout@v4
    - uses: astral-sh/setup-uv@v6
    - name: Install dependencies
      run: uv sync --all-groups
    - name: pytest
      run: uv run pytest -v
```

The Python pipeline uses `uv` (not `pip`) for package management and `ruff` for linting. `uvx ruff@0.15.0` pins the linter version so lint results are reproducible. `uv sync --all-groups` installs all dependency groups including test dependencies (pytest, pytest-asyncio).

### Hub checks

```yaml
hub:
  name: Hub Build
  runs-on: ubuntu-latest
  defaults:
    run:
      working-directory: hub
  steps:
    - uses: actions/checkout@v4
    - uses: pnpm/action-setup@v4
      with:
        version: 10
    - uses: actions/setup-node@v4
      with:
        node-version: 22
        cache: pnpm
        cache-dependency-path: hub/pnpm-lock.yaml
    - name: tsc --noEmit
      run: pnpm exec tsc --noEmit
    - name: Build
      run: pnpm build
```

The hub has no runtime tests. `tsc --noEmit` type-checks the TypeScript without producing output files. `pnpm build` runs the Vite production build, which catches import errors and build-time issues.

### Version bump enforcement

```yaml
python-version-check:
  name: Version Bump Check
  runs-on: ubuntu-latest
  if: github.event_name == 'pull_request'
  steps:
    - uses: actions/checkout@v4
      with:
        fetch-depth: 0
    - name: Check version bump when agent/ changed
      run: |
        BASE="${{ github.event.pull_request.base.sha }}"
        CHANGED=$(git diff --name-only "$BASE"...HEAD -- \
          'agent/chatixia/**' 'agent/pyproject.toml' | \
          grep -v '__pycache__' || true)
        if [ -z "$CHANGED" ]; then
          exit 0
        fi
        OLD=$(git show "$BASE":agent/pyproject.toml | \
          python3 -c "import sys,tomllib; \
          print(tomllib.load(sys.stdin.buffer)['project']['version'])")
        NEW=$(python3 -c "import tomllib; \
          print(tomllib.load(open('agent/pyproject.toml','rb'))['project']['version'])")
        if [ "$OLD" = "$NEW" ]; then
          echo "::warning::agent/ source files changed but version is still $OLD."
          exit 1
        fi
        echo "Version bumped: $OLD -> $NEW"
```

This check runs only on PRs. It compares the `pyproject.toml` version between the base branch and the PR. If any Python source files changed but the version did not bump, the check fails. This prevents merging code changes without a version increment, which is required for PyPI publishing.

### PyPI automated publishing

```yaml
# .github/workflows/publish-pypi.yml

on:
  release:
    types: [published]

permissions:
  id-token: write  # OIDC for PyPI trusted publisher

jobs:
  publish:
    if: startsWith(github.event.release.tag_name, 'v')
    environment: pypi
    steps:
      - uses: actions/checkout@v4
      - uses: astral-sh/setup-uv@v6
      - name: Verify tag matches package version
        run: |
          TAG="${GITHUB_REF_NAME#v}"
          PKG=$(uv run python -c "...")
          if [ "$TAG" != "$PKG" ]; then exit 1; fi
      - name: Build package
        run: uv build
      - name: Publish to PyPI
        uses: pypa/gh-action-pypi-publish@release/v1
        with:
          packages-dir: agent/dist/
```

Publishing uses OIDC trusted publisher -- no API tokens stored in GitHub secrets. The workflow verifies that the git tag matches the `pyproject.toml` version before publishing. This creates a two-step safety net: the version bump check prevents merging without a bump, and the tag check prevents publishing a version that does not match the code.

### Docker build validation

```yaml
docker:
  name: Docker Build
  runs-on: ubuntu-latest
  if: github.event_name == 'pull_request'
  strategy:
    matrix:
      target: [registry, sidecar, agent]
  steps:
    - uses: actions/checkout@v4
    - uses: docker/setup-buildx-action@v3
    - name: Build ${{ matrix.target }}
      uses: docker/build-push-action@v6
      with:
        context: .
        file: ${{ matrix.target }}/Dockerfile
        push: false
        cache-from: type=gha
        cache-to: type=gha,mode=max
```

On PRs, Docker images are built for all three components but not pushed. This catches Dockerfile errors (missing dependencies, broken COPY commands) before they reach the main branch. The GitHub Actions cache (`type=gha`) shares Docker layer cache between runs.

---

## Exercises

### Exercise 1: Write a unit test for a new MeshMessage type

The mesh protocol needs a new `agent_heartbeat` message type with the following fields in its payload:

- `agent_id` (string) -- the agent's identifier
- `uptime_seconds` (integer) -- how long the agent has been running
- `skills` (array of strings) -- the agent's registered skills

Write a Rust unit test (in the style of the existing `protocol.rs` tests) that:

1. Creates a `MeshMessage` with `msg_type` set to `"agent_heartbeat"`.
2. Sets `source_agent` to `"agent-alpha"`.
3. Sets the payload to a JSON object containing `agent_id`, `uptime_seconds`, and `skills`.
4. Serializes the message to a JSON string.
5. Deserializes the JSON string back to a `MeshMessage`.
6. Asserts that all fields round-trip correctly, including the nested payload fields.

Then write the equivalent Python test using `MeshMessage.to_dict()` and `MeshMessage.from_dict()`.

### Exercise 2: Design an integration test for IPC round-trip

Design a test that verifies the IPC protocol between the sidecar and the Python agent. The test should:

1. Create a mock Unix domain socket server (simulating the sidecar).
2. Have the mock server listen for a JSON-line message of type `"send"`.
3. When the `"send"` message is received, echo back a `"message"` event containing the same payload.
4. On the Python side, connect a `MeshClient` to the mock socket, send a message, and verify the echoed response.

Write the test skeleton in Python using `asyncio` and `unittest.mock`. You do not need to implement the full `MeshClient` connection logic -- focus on the socket I/O and JSON-line framing. Indicate which parts would need the real `MeshClient` and which can be mocked.

Hint: `asyncio.start_unix_server` creates an async Unix domain socket server. Each line on the socket is a complete JSON object terminated by `\n`.

### Exercise 3: Catch the heartbeat bug at integration level

The heartbeat bug from Session 4 was only caught during E2E testing. Propose an integration test that would catch this bug without running a full registry.

Your test should:

1. Mock `requests.post` to return a response with `pending_tasks` in the JSON body.
2. Call the code path that processes the heartbeat response.
3. Assert that the tasks in `pending_tasks` are dispatched to the appropriate skill handlers.
4. Assert that `_update_task` is called with the correct task ID and result.

Explain what you would mock, what you would run for real, and what assertions would catch the original bug (where `resp.json()` was never called). Write the test skeleton.

### Exercise 4: Propose a signaling protocol integration test

The signaling protocol involves the registry's WebSocket handler and the sidecar's signaling client. Currently, these are only tested together during E2E sessions.

Design an integration test that validates the signaling protocol between registry and sidecar within a single test process. Your proposal should address:

1. **How to start the registry** -- Can you run the axum server on `localhost` within a `#[tokio::test]`? What port should you use?
2. **How to simulate the sidecar** -- Do you need the full sidecar, or can a lightweight WebSocket client substitute?
3. **What to verify** -- After the client sends a `"join"` message, what should the registry do? After a second client joins, what signaling messages should the registry relay?
4. **How to make it deterministic** -- WebSocket message ordering, connection timing, and cleanup.

Write a test outline in Rust using `tokio` and a WebSocket client library. Explain why this test is more valuable than testing the signaling handler in isolation, and what categories of bugs it would catch that unit tests would miss.

---

## Summary

Testing distributed systems requires tests at every level of the pyramid, with special attention to the boundaries between components.

**Unit tests** verify that individual functions -- serialization, state transitions, token validation -- behave correctly. They are fast and deterministic, and they should cover every protocol struct, every state machine transition, and every security boundary.

**Integration tests** verify that components interact correctly. Mock the network and filesystem, but run the real handler logic. The skill handler tests with mock `MeshClient` are the highest-value tests in the Python codebase because they cover both the P2P and fallback paths.

**E2E tests** verify that the entire system works end-to-end. They are expensive but essential. The heartbeat bug -- where tasks were assigned but never executed because the response body was discarded -- was invisible to unit and integration tests. Only a live E2E test with real agents revealed it.

**Async testing** requires special tooling: `#[tokio::test]` in Rust, `@pytest.mark.asyncio` in Python. Mock async methods with `AsyncMock`, test timeouts explicitly, and use channels or queues to test concurrent operations.

**CI for multi-language projects** runs checks in parallel: Rust (fmt, clippy, test), Python (ruff, pytest), TypeScript (tsc, build). Version bump enforcement and Docker build validation on PRs prevent common release mistakes. OIDC trusted publishing eliminates stored API tokens.

The core lesson: **in a distributed system, the most dangerous bugs hide at the seams between components.** Push as many seam-crossing tests as possible into the integration tier, and use E2E tests as the final safety net for the interactions that cannot be simulated.
