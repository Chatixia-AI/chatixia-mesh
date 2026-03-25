# Lesson 09 -- AI Agent Architecture: Skills, LLMs, and Orchestration

> **Prerequisites:** [Lesson 06 -- Inter-Process Communication](06-inter-process-communication.md), [Lesson 07 -- Application Protocol Design](07-application-protocol-design.md)
>
> **Time estimate:** 75--90 minutes
>
> **Key files:**
> - `agent/chatixia/runner.py` -- agent lifecycle, task execution, P2P message handler
> - `agent/chatixia/config.py` -- AgentConfig and SidecarConfig dataclasses
> - `agent/chatixia/cli.py` -- CLI subcommands: init, run, validate, pair
> - `agent/chatixia/core/mesh_skills.py` -- built-in skill handler implementations
> - `agent/chatixia/core/mesh_client.py` -- MeshClient IPC bridge, MeshMessage
> - `agent/chatixia/scaffold.py` -- agent manifest scaffolding

---

## 1. What Is an AI Agent?

A chatbot responds to prompts. An agent *acts*.

The difference is the loop. A chatbot takes input, produces output, and stops. An agent takes input, decides what to do, executes an action, observes the result, and decides again. This cycle -- reason, act, observe -- repeats until the task is complete or a limit is reached.

### 1.1 The Tool-Use Loop

Modern LLM-based agents follow a pattern called the **tool-use loop** (sometimes called ReAct -- Reasoning + Acting):

```
                 +------------------+
                 |   User prompt    |
                 +--------+---------+
                          |
                          v
                 +------------------+
            +--->|   LLM reasons    |----> Done? ---> Return result
            |    +--------+---------+
            |             |
            |        Tool call
            |             |
            |             v
            |    +------------------+
            |    | Runtime executes |
            |    |   the tool       |
            |    +--------+---------+
            |             |
            |        Tool result
            |             |
            +-------------+
```

1. The LLM receives a prompt and a list of available tools (functions with descriptions and parameter schemas).
2. Instead of answering directly, the LLM generates a **tool call** -- a structured request to invoke a specific function with specific arguments.
3. The agent runtime executes the function and feeds the result back to the LLM.
4. The LLM incorporates the result and either makes another tool call or produces a final response.

This is the fundamental mechanism that separates agents from chatbots. The LLM does not execute code -- it *describes what to execute*, and the runtime does the work.

### 1.2 MCP: Model Context Protocol

MCP (Model Context Protocol) standardizes how tools are exposed to LLMs. Instead of every agent framework inventing its own tool definition format, MCP defines a JSON-RPC protocol for:

- **Tool discovery** -- the agent queries an MCP server for available tools, their names, descriptions, and parameter schemas
- **Tool invocation** -- the agent sends a tool call to the MCP server, which executes it and returns the result
- **Resource access** -- the agent reads data (files, database rows, API responses) through a uniform interface

chatixia-mesh's `AgentConfig` includes an `mcp_servers` field in its capabilities, and the registry tracks which MCP servers each agent exposes. The skill system described in this lesson is chatixia-mesh's own tool mechanism -- it predates MCP and can coexist with it.

### 1.3 From Single Agent to Multi-Agent

A single agent with tools is useful. Multiple agents that can *discover each other's tools and delegate work* are more interesting. This is where chatixia-mesh operates: it provides the infrastructure for agents to find peers, learn their capabilities, and send them work -- all over peer-to-peer DataChannels.

The rest of this lesson explains how chatixia-mesh implements this.

---

## 2. The Skill Model

In chatixia-mesh, an agent's capabilities are expressed as **skills** -- named functions with parameter schemas that other agents (or the hub dashboard) can invoke.

### 2.1 What a Skill Is

A skill has three parts:

1. **A name** -- a string identifier like `"delegate"` or `"list_agents"`
2. **A parameter schema** -- what inputs the skill accepts
3. **A handler function** -- the Python code that executes the skill

Skills are defined in `skill.json` files with this structure:

```json
{
  "name": "delegate",
  "description": "Delegate a task to another agent",
  "version": "1.0.0",
  "category": "Mesh",
  "parameters": {
    "message": {
      "type": "string",
      "description": "The task to delegate",
      "required": true
    },
    "target_agent_id": {
      "type": "string",
      "description": "Target agent (optional if skill is specified)",
      "required": false
    },
    "skill": {
      "type": "string",
      "description": "Route to an agent with this skill",
      "required": false
    }
  }
}
```

### 2.2 Skill Registration and Discovery

When an agent starts, it registers with the registry via `POST /api/registry/agents`, advertising its skills in the `capabilities` field:

```python
# From runner.py -- _register()
resp = requests.post(
    f"{registry}/api/registry/agents",
    json={
        "agent_id": agent_id,
        "hostname": socket.gethostname(),
        "sidecar_peer_id": f"{agent_id}-sidecar",
        "capabilities": {
            "skills": config.skills_builtin,   # ["delegate", "list_agents", ...]
            "mcp_servers": [],
            "goals_count": 0,
            "mode": "interactive",
        },
    },
    headers={"x-api-key": api_key},
    timeout=10,
)
```

The registry stores this information. Other agents can then query for skills:

```
GET /api/registry/route?skill=delegate
```

The registry responds with the agent best suited for that skill:

```json
{
  "agent_id": "researcher-01",
  "sidecar_peer_id": "researcher-01-sidecar"
}
```

This is the **control plane** in action. The registry knows which agents have which skills, and routes requests accordingly. The actual task execution happens over the **data plane** -- either P2P DataChannels or the HTTP task queue fallback.

### 2.3 The SKILL_HANDLERS Registry

Inside the agent, skills are wired through a simple dictionary in `runner.py`:

```python
# Skill name -> handler function (sync or async)
SKILL_HANDLERS: dict[str, Callable[..., str | Awaitable[str]]] = {
    "list_agents": handle_list_agents,
    "find_agent": handle_find_agent,
    "delegate": handle_delegate,
    "mesh_send": handle_mesh_send,
    "mesh_broadcast": handle_mesh_broadcast,
    "user_intervention": handle_user_intervention,
}
```

When a task arrives -- whether via P2P DataChannel or the heartbeat-polled task queue -- the runner looks up the skill name in this dictionary and calls the corresponding handler. The handler signature accepts keyword arguments (`**kwargs`) and returns a string result.

---

## 3. Built-in Skills

chatixia-mesh ships six built-in skills. They fall into two categories: **discovery** (read the control plane) and **communication** (use the data plane).

### 3.1 `list_agents` -- HTTP Discovery

**Handler:** `handle_list_agents()` in `mesh_skills.py`
**Transport:** HTTP (always)
**Pattern:** Synchronous, control plane

Lists all agents currently registered in the mesh. This is a read-only query to the registry's REST API:

```python
def handle_list_agents(**kwargs) -> str:
    registry = _registry_url()
    agents = _get(f"{registry}/api/registry/agents")
    # ... format and return agent list
```

The handler calls `GET /api/registry/agents` and formats the response as a markdown-style listing showing each agent's ID, health status, hostname, peer ID, and skills.

This is always HTTP because agent discovery is a control plane operation. The registry is the source of truth for "who is online and what can they do."

### 3.2 `find_agent` -- Skill-Based Routing

**Handler:** `handle_find_agent()` in `mesh_skills.py`
**Transport:** HTTP (always)
**Pattern:** Synchronous, control plane

Finds the best agent for a specific skill:

```python
def handle_find_agent(skill: str = "", **kwargs) -> str:
    registry = _registry_url()
    result = _get(f"{registry}/api/registry/route?skill={skill}")
    # ... return agent_id and peer_id
```

This calls `GET /api/registry/route?skill=X`. The registry looks through all registered agents to find one that advertises the requested skill and returns its identity and peer address.

Like `list_agents`, this is pure control plane -- it answers "who can do X?" but does not execute X.

### 3.3 `delegate` -- P2P Task Delegation with Response

**Handler:** `handle_delegate()` in `mesh_skills.py`
**Transport:** P2P DataChannel (preferred), HTTP task queue (fallback)
**Pattern:** Asynchronous, data plane

This is the most important skill. It sends a task to another agent and waits for the result. The handler implements a two-tier transport strategy:

```
Is the MeshClient connected?
  |
  +-- Yes --> Is the target peer connected via DataChannel?
  |             |
  |             +-- Yes --> Send task_request via P2P, await task_response
  |             |
  |             +-- No --> Fall through to HTTP
  |
  +-- No --> Submit task via POST /api/hub/tasks, poll for result
```

The P2P path constructs a `MeshMessage` with type `task_request` and uses `MeshClient.request()` to send it and await a correlated response (matched by `request_id`):

```python
msg = MeshMessage(
    msg_type="task_request",
    source_agent=agent_id,
    target_agent=target_agent_id,
    payload={"message": message, "skill": skill},
)
response = await _mesh_client.request(target_peer, msg, timeout=120.0)
```

If the P2P path is unavailable (no DataChannel to the target), the handler falls back to the registry's HTTP task queue. It submits the task via `POST /api/hub/tasks` and polls `GET /api/hub/tasks/{task_id}` every 3 seconds until completion or timeout.

The `wait` parameter controls whether `delegate` blocks for a response or fires and forgets. With `wait=False`, it sends the task and returns immediately.

### 3.4 `mesh_send` -- Fire-and-Forget Messaging

**Handler:** `handle_mesh_send()` in `mesh_skills.py`
**Transport:** P2P DataChannel (preferred), HTTP task queue (fallback)
**Pattern:** Asynchronous, data plane

Sends a direct message to a specific agent without waiting for a response:

```python
msg = MeshMessage(
    msg_type="agent_prompt",
    source_agent=agent_id,
    target_agent=target_agent_id,
    payload={"message": message, "direct": True},
)
await _mesh_client.send(target_peer, msg)
```

Notice the difference from `delegate`: this uses `MeshClient.send()` (fire-and-forget) instead of `MeshClient.request()` (send and wait). The message type is `agent_prompt` rather than `task_request`, because this is informal communication -- one agent talking to another -- not a structured task with an expected result.

### 3.5 `mesh_broadcast` -- Fire-and-Forget to All Peers

**Handler:** `handle_mesh_broadcast()` in `mesh_skills.py`
**Transport:** P2P DataChannel (preferred), HTTP per-agent task queue (fallback)
**Pattern:** Asynchronous, data plane

Broadcasts a message to every connected peer:

```python
msg = MeshMessage(
    msg_type="agent_prompt",
    source_agent=agent_id,
    target_agent="*",
    payload={"message": message, "broadcast": True},
)
await _mesh_client.broadcast(msg)
```

The `target_agent` is `"*"` to indicate broadcast. `MeshClient.broadcast()` sends the message to every peer in its connected peer set.

The HTTP fallback is more expensive: it queries `GET /api/registry/agents` to discover all active agents, then submits a separate task to each one via `POST /api/hub/tasks`.

### 3.6 `user_intervention` -- Human-in-the-Loop

**Handler:** `handle_user_intervention()` in `runner.py`
**Transport:** HTTP task queue (via hub dashboard)
**Pattern:** Synchronous

This skill handles free-form messages sent from the hub dashboard's intervention panel. When a human operator types a message in the dashboard, it becomes a task with skill `user_intervention` that the agent picks up:

```python
def handle_user_intervention(message: str = "", **kwargs: Any) -> str:
    if not message:
        return "Received empty intervention."
    logger.info("user intervention: %s", message)
    return f"Received: {message}"
```

This is the simplest skill -- it just acknowledges the message. In a production system, this handler would feed the message into the agent's LLM for processing.

### 3.7 Skill Summary

```
+-------------------+-------+--------+-----------------------------+
| Skill             | Async | Plane  | Purpose                     |
+-------------------+-------+--------+-----------------------------+
| list_agents       |  No   | Ctrl   | List all mesh agents        |
| find_agent        |  No   | Ctrl   | Route by skill name         |
| delegate          |  Yes  | Data   | Task with response          |
| mesh_send         |  Yes  | Data   | Direct message (one peer)   |
| mesh_broadcast    |  Yes  | Data   | Broadcast (all peers)       |
| user_intervention |  No   | Data   | Human-in-the-loop message   |
+-------------------+-------+--------+-----------------------------+
```

---

## 4. Sync vs Async Skill Handlers

Not all handlers are defined the same way. Some are `def` (synchronous), others are `async def` (asynchronous). This is not arbitrary -- it follows from what each handler needs to do.

### 4.1 Why Some Handlers Are Sync

`list_agents` and `find_agent` make HTTP requests to the registry. They use Python's `urllib.request` module, which is synchronous and blocking. These handlers do not need the MeshClient because they only interact with the registry's REST API -- the control plane:

```python
def handle_list_agents(**kwargs) -> str:
    registry = _registry_url()
    agents = _get(f"{registry}/api/registry/agents")   # blocking HTTP
    # ...
```

This works fine because HTTP requests to a local registry complete in milliseconds. The blocking call does not hold up the event loop for a meaningful amount of time.

### 4.2 Why Some Handlers Are Async

`delegate`, `mesh_send`, and `mesh_broadcast` need the `MeshClient` to send messages over the sidecar's IPC socket. The `MeshClient` is an async class -- its `send()`, `broadcast()`, and `request()` methods are all coroutines. You cannot call `await` inside a regular `def` function, so these handlers must be `async def`:

```python
async def handle_delegate(
    message: str = "",
    target_agent_id: str = "",
    skill: str = "",
    wait: bool = True,
    _mesh_client: MeshClient | None = None,
    **kwargs,
) -> str:
    # ...
    response = await _mesh_client.request(target_peer, msg, timeout=120.0)
```

The `_mesh_client` parameter is injected by the runner before calling the handler. This is a form of dependency injection -- the handler does not import or construct the MeshClient itself.

### 4.3 The Evolution: ADR-005 to ADR-016

This dual-mode design has a history recorded in the project's Architecture Decision Records:

**ADR-005 (March 21)** -- All skill handlers were synchronous. When `delegate` needed to send work to another agent, it submitted a task via the registry's HTTP task queue (`POST /api/hub/tasks`). The target agent picked it up on its next heartbeat, 3--15 seconds later. This worked, but it meant that all agent-to-agent data flowed through the registry -- contradicting the system's P2P architecture.

**ADR-016 (March 22)** -- Skill handlers were converted to async. `delegate`, `mesh_send`, and `mesh_broadcast` now send messages directly over WebRTC DataChannels via the MeshClient, with the HTTP task queue as a fallback for when peers are not directly connected.

This refactoring delivered two key improvements:

1. **Latency** dropped from 3--15 seconds (heartbeat poll interval) to sub-second (direct DataChannel)
2. **Architecture alignment** -- the registry is now truly out of the data path for connected peers

### 4.4 How the Runner Handles Both

The runner uses a simple pattern to handle both sync and async handlers uniformly:

```python
result = handler(**task_payload)
if asyncio.iscoroutine(result):
    result = await result
```

If the handler returns a coroutine (because it is `async def`), the runner awaits it. If it returns a plain string (because it is regular `def`), the runner uses it directly. This avoids forcing every handler to be async when some do not need to be.

---

## 5. Agent Configuration

Every chatixia agent is defined by an `agent.yaml` manifest file. The `AgentConfig` dataclass in `config.py` mirrors this structure.

### 5.1 The agent.yaml Manifest

Here is a complete manifest with all fields:

```yaml
# Agent identity
name: researcher-01
description: "Research agent specializing in web search"

# Registry -- the mesh signaling + coordination server
registry: "http://localhost:8080"

# LLM provider: azure | openai | ollama
provider: azure
# model: gpt-4o

# System prompt -- defines the agent's persona and behavior
prompt: |
  You are a research assistant connected to the Chatixia mesh.
  Use delegate to send tasks to other agents.
  Use list_agents to discover who is online.

# Sidecar -- the Rust WebRTC peer
sidecar:
  binary: chatixia-sidecar
  api_key: ak_dev_001
  socket: /tmp/chatixia-researcher-01.sock

# Skills configuration
skills:
  builtin:
    - delegate
    - list_agents
    - mesh_send
    - mesh_broadcast
    - find_agent
  # dirs:
  #   - ./skills          # Additional skill directories
  # disabled:
  #   - mesh_broadcast    # Skills to exclude

# Runtime settings
data_dir: .chatixia
max_turns: 10
context_window: 120000
```

### 5.2 The AgentConfig Dataclass

The `load_config()` function in `config.py` parses this YAML into a typed dataclass:

```python
@dataclass
class SidecarConfig:
    binary: str = "chatixia-sidecar"
    api_key: str = "ak_dev_001"
    socket: str = "/tmp/chatixia-sidecar.sock"

@dataclass
class AgentConfig:
    name: str
    description: str = ""
    registry: str = "http://localhost:8080"
    sidecar: SidecarConfig = field(default_factory=SidecarConfig)
    provider: str = "azure"            # azure | openai | ollama
    model: str = ""
    prompt: str = ""
    skills_builtin: list[str] = field(default_factory=list)
    skills_dirs: list[str] = field(default_factory=list)
    skills_disabled: list[str] = field(default_factory=list)
    data_dir: str = ".chatixia"
    max_turns: int = 10
    context_window: int = 120_000
    _source_dir: Path = field(default_factory=Path.cwd)
```

Important design choices:

- **Provider abstraction** -- the agent supports three LLM backends (Azure OpenAI, OpenAI, Ollama) selected by a single `provider` field. Credentials come from environment variables, not the manifest.
- **Sidecar as a nested config** -- the sidecar binary path, API key, and IPC socket path are grouped together. Each agent gets its own socket path to avoid collisions when running multiple agents on one machine.
- **Skills as lists** -- `skills_builtin` lists which built-in handlers to enable. `skills_dirs` points to directories containing custom skill definitions. `skills_disabled` excludes specific skills. This gives operators fine-grained control without modifying code.
- **Validation** -- `AgentConfig.validate()` checks that the name is present, the provider is valid, and the registry URL exists. The CLI's `validate` subcommand runs this before the agent starts.

### 5.3 Environment Variables and .env

Sensitive values (API keys, LLM credentials) live in a `.env` file, not in the manifest. The runner loads this file at startup using `python-dotenv`:

```python
env_path = config.resolve_path(".env")
if env_path.exists():
    from dotenv import load_dotenv
    load_dotenv(env_path)
```

The runner also exports several environment variables that downstream components (the sidecar, skill handlers) depend on:

```python
os.environ.setdefault("REGISTRY_URL", registry)
os.environ.setdefault("CHATIXIA_REGISTRY_URL", registry)
os.environ.setdefault("CHATIXIA_AGENT_ID", agent_id)
os.environ.setdefault("API_KEY", api_key)
os.environ.setdefault("SIGNALING_URL", f"{ws_scheme}://{ws_base}/ws")
os.environ.setdefault("TOKEN_URL", f"{registry}/api/token")
```

This means the skill handlers in `mesh_skills.py` do not need the `AgentConfig` object -- they read the registry URL and agent ID from environment variables. This decoupling is intentional: skills may be loaded from external directories and should not depend on the runner's internal types.

---

## 6. Agent Lifecycle

An agent goes through five phases from startup to shutdown. Here is the complete lifecycle:

```
  chatixia run agent.yaml
         |
         v
  +------+--------+
  | 1. LOAD CONFIG |  Parse agent.yaml, load .env,
  |                |  validate settings
  +------+---------+
         |
         v
  +------+---------+
  | 2. REGISTER    |  POST /api/registry/agents
  |                |  (name, hostname, skills, peer_id)
  +------+---------+
         |
         v
  +------+---------+
  | 3. CONNECT     |  Spawn sidecar, connect IPC socket,
  |    MESH        |  register P2P message handler
  +------+---------+
         |
         v
  +------+---------+
  | 4. HEARTBEAT   |  Every 15s: POST /api/hub/heartbeat
  |    LOOP        |  Pick up pending tasks, execute via
  |                |  asyncio.create_task()
  +------+---------+
         |
    SIGINT/SIGTERM
         |
         v
  +------+---------+
  | 5. SHUTDOWN    |  DELETE /api/registry/agents/{id}
  |                |  Stop sidecar, close IPC
  +------+---------+
```

### 6.1 Phase 1: Load Configuration

The CLI's `run` subcommand loads and validates the manifest:

```python
# cli.py -- _cmd_run()
config = load_config(args.manifest)
errors = config.validate()
if errors:
    for err in errors:
        print(f"  Error: {err}", file=sys.stderr)
    return 1
asyncio.run(run_agent(config))
```

`load_config()` accepts either a file path (`agent.yaml`) or a directory (it looks for `agent.yaml` inside it). If validation fails, the agent exits before making any network calls.

### 6.2 Phase 2: Register with Registry

The agent announces itself to the registry:

```python
# runner.py -- _register()
requests.post(
    f"{registry}/api/registry/agents",
    json={
        "agent_id": agent_id,
        "hostname": socket.gethostname(),
        "sidecar_peer_id": f"{agent_id}-sidecar",
        "capabilities": {
            "skills": config.skills_builtin,
            ...
        },
    },
    headers={"x-api-key": api_key},
    timeout=10,
)
```

This is a synchronous HTTP call. If the registry is unreachable, the agent exits with an error message explaining what went wrong (connection refused, timeout, authentication failure). The error messages include actionable advice:

```
Cannot connect to registry at http://localhost:8080
Is the registry running? Start it with:
  chatixia-registry          # default port 8080
  PORT=9090 chatixia-registry # custom port
```

### 6.3 Phase 3: Connect to Mesh

The runner creates a `MeshClient`, which spawns the Rust sidecar and connects to its IPC socket:

```python
client = MeshClient(
    socket_path=config.sidecar.socket,
    sidecar_binary=config.sidecar.binary,
)
await client.start()
```

`MeshClient.start()` does three things:
1. Spawns the sidecar binary as a subprocess with the IPC socket path as an environment variable
2. Waits up to 5 seconds for the socket file to appear on disk
3. Opens an async Unix socket connection and starts a background listen loop

Once connected, the runner registers a handler for incoming P2P task requests:

```python
async def _handle_p2p_message(data: dict[str, Any]) -> None:
    # Extract skill name from task_request
    # Look up handler in SKILL_HANDLERS
    # Execute handler, send task_response back
    ...

client.on("message", _handle_p2p_message)
```

The runner also installs signal handlers for clean shutdown:

```python
for sig in (signal.SIGINT, signal.SIGTERM):
    loop.add_signal_handler(
        sig,
        lambda: asyncio.create_task(
            _shutdown(client, registry, api_key, agent_id)
        ),
    )
```

### 6.4 Phase 4: Heartbeat Loop

The heartbeat serves two purposes: it tells the registry the agent is still alive, and it picks up any tasks assigned via the HTTP task queue:

```python
while True:
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
    await asyncio.sleep(15)
```

Key detail: tasks are dispatched with `asyncio.create_task()`, which runs each task as a separate coroutine. This means:

- The heartbeat loop does not block while a task runs
- Multiple tasks can execute concurrently
- A slow task does not delay the next heartbeat

The registry uses heartbeats to track agent health: active (heartbeat within 90 seconds), stale (90--270 seconds), offline (no heartbeat for 270+ seconds).

### 6.5 Phase 5: Shutdown

On SIGINT (Ctrl+C) or SIGTERM, the shutdown handler runs:

```python
async def _shutdown(client, registry, api_key, agent_id):
    requests.delete(
        f"{registry}/api/registry/agents/{agent_id}",
        headers={"x-api-key": api_key},
        timeout=5,
    )
    await client.stop()
    asyncio.get_running_loop().stop()
```

The `DELETE /api/registry/agents/{agent_id}` call removes the agent from the registry immediately. Without this, the agent would linger as "active" until the health check loop marks it stale after 90 seconds.

`client.stop()` terminates the sidecar process and closes the IPC connection.

---

## 7. Task Execution: Two Paths

Tasks reach an agent through two independent paths. Understanding both is essential for tracing message flows through the system.

### 7.1 Path A: P2P DataChannel (Low Latency)

When Agent A delegates to Agent B and both are connected via WebRTC:

```
Agent A                  Sidecar A            Sidecar B            Agent B
   |                        |                    |                    |
   | handle_delegate()      |                    |                    |
   |-- MeshMessage -------->|                    |                    |
   |   (task_request)       |-- DataChannel ---->|                    |
   |                        |   (DTLS encrypted) |-- IPC message ---->|
   |                        |                    |                    |
   |                        |                    |   _handle_p2p_message()
   |                        |                    |   SKILL_HANDLERS[skill]()
   |                        |                    |                    |
   |                        |                    |<-- IPC message ----|
   |                        |<-- DataChannel ----|   (task_response)  |
   |<-- MeshMessage --------|                    |                    |
   |   (task_response)      |                    |                    |
```

Latency: sub-second. The registry is not involved at all.

### 7.2 Path B: HTTP Task Queue (Fallback)

When peers are not directly connected (no DataChannel):

```
Agent A              Registry               Agent B
   |                    |                      |
   | POST /api/hub/tasks|                      |
   |------------------->|                      |
   |  task_id           |                      |
   |<-------------------|                      |
   |                    |                      |
   | (polls every 3s)   |  POST /api/hub/heartbeat
   |                    |<---------------------|
   |                    |  pending_tasks: [task]|
   |                    |--------------------->|
   |                    |                      |
   |                    |  _execute_task()     |
   |                    |  SKILL_HANDLERS[skill]()
   |                    |                      |
   |                    |  POST /api/hub/tasks/{id}
   |                    |<---------------------|
   |                    |  state: completed    |
   |                    |                      |
   | GET /api/hub/tasks/{id}                   |
   |------------------->|                      |
   |  result            |                      |
   |<-------------------|                      |
```

Latency: 3--15 seconds (depends on heartbeat timing). The registry is in the data path.

### 7.3 The _execute_task Function

Both paths converge on the same skill execution logic. For P2P tasks, `_handle_p2p_message` handles extraction and response. For HTTP tasks, `_execute_task` does the same work with an HTTP response:

```python
async def _execute_task(registry, api_key, task, mesh_client=None):
    handler = SKILL_HANDLERS.get(skill)
    if handler is None:
        _update_task(registry, api_key, task_id, "failed",
                     error=f"unknown skill: {skill}")
        return

    payload["_mesh_client"] = mesh_client
    result = handler(**payload)
    if asyncio.iscoroutine(result):
        result = await result

    _update_task(registry, api_key, task_id, "completed",
                 result=str(result))
```

Notice `payload["_mesh_client"] = mesh_client` -- this is how the MeshClient gets injected into skill handlers. The `_mesh_client` parameter is not part of the task payload; it is added by the runner before calling the handler.

---

## 8. Multi-Agent Collaboration

Now that you understand individual agent mechanics, let us trace a complete multi-agent collaboration scenario.

### 8.1 Discovery, Delegation, Execution

Suppose Agent A (a coordinator) needs web research done. Agent B (a researcher) has the `delegate` skill and web search capabilities.

**Step 1: Discovery**

Agent A needs to find an agent that can do research. It invokes `find_agent`:

```python
# Agent A's LLM generates this tool call:
find_agent(skill="web_search")

# Handler calls:
GET /api/registry/route?skill=web_search
# Response: {"agent_id": "researcher-01", "sidecar_peer_id": "researcher-01-sidecar"}
```

**Step 2: Delegation**

Agent A delegates the task:

```python
# Agent A's LLM generates this tool call:
delegate(
    message="Search for recent papers on multi-agent systems",
    target_agent_id="researcher-01"
)
```

The handler constructs a `MeshMessage` and sends it via DataChannel:

```json
{
  "type": "task_request",
  "request_id": "a1b2c3d4e5f6",
  "source_agent": "coordinator-01",
  "target_agent": "researcher-01",
  "payload": {
    "message": "Search for recent papers on multi-agent systems",
    "skill": ""
  }
}
```

**Step 3: Execution**

Agent B's `_handle_p2p_message` receives the task_request, finds the handler for the requested skill, executes it, and sends back a task_response:

```json
{
  "type": "task_response",
  "request_id": "a1b2c3d4e5f6",
  "source_agent": "researcher-01",
  "target_agent": "coordinator-01",
  "payload": {
    "result": "Found 3 relevant papers: ...",
    "error": ""
  }
}
```

**Step 4: Correlation**

Agent A's `MeshClient.request()` method correlates the response by `request_id`. The `_pending_responses` dictionary maps request IDs to asyncio Futures:

```python
# In MeshClient._dispatch():
if req_id and req_id in self._pending_responses:
    self._pending_responses[req_id].set_result(inner)
    return
```

The Future resolves, and `delegate` returns the result to Agent A's LLM.

### 8.2 Role-Based Agent Design

Agents in a mesh can be specialized for different roles. The `prompt` field in `agent.yaml` defines the agent's persona and behavior. While chatixia-mesh does not enforce role categories, the following patterns are useful for organizing multi-agent systems:

**Researcher** -- specializes in information gathering. Skills: web search, document retrieval, data extraction. Prompt instructs it to search broadly, cite sources, and report findings.

```yaml
name: researcher-01
prompt: |
  You are a research specialist. When given a topic, search for
  relevant information, synthesize findings, and cite your sources.
  Use delegate to pass analysis tasks to analyst agents.
skills:
  builtin: [delegate, list_agents, mesh_send, find_agent]
```

**Analyst** -- specializes in processing and interpreting data. Receives raw data from researchers, produces structured analysis.

```yaml
name: analyst-01
prompt: |
  You are a data analyst. You receive raw data and produce
  structured analysis with key findings and recommendations.
skills:
  builtin: [delegate, list_agents, mesh_send]
```

**Coordinator** -- orchestrates work across other agents. Does not do domain work itself -- it discovers agents, breaks tasks into subtasks, delegates, and assembles results.

```yaml
name: coordinator-01
prompt: |
  You are a task coordinator. Break complex requests into subtasks,
  find the right agent for each, delegate the work, and synthesize
  the results into a coherent response.
skills:
  builtin: [delegate, list_agents, find_agent, mesh_broadcast]
```

**Worker** -- a general-purpose agent with a narrow set of skills. Receives tasks, executes them, returns results. Does not initiate communication.

```yaml
name: worker-01
prompt: |
  You are a worker agent. Execute the tasks you receive and
  return results. Do not initiate tasks on your own.
skills:
  builtin: [delegate, mesh_send]
```

### 8.3 The CLI Workflow

The `chatixia` CLI provides the operator workflow for creating and running agents:

```
$ chatixia init researcher-01        # Scaffold agent.yaml, .env.example, .gitignore
$ cd researcher-01
$ cp .env.example .env               # Fill in LLM credentials
$ vim agent.yaml                     # Set role, prompt, skills
$ chatixia validate                  # Check manifest is valid
$ chatixia pair 482901               # Redeem invite code from admin
$ chatixia run                       # Register, connect, heartbeat
```

Each CLI subcommand maps to a specific phase:

| Command | What it does | Key function |
|---------|-------------|--------------|
| `chatixia init <name>` | Creates a directory with `agent.yaml`, `.env.example`, `.gitignore` | `scaffold.write_scaffold()` |
| `chatixia validate [manifest]` | Parses manifest, runs `AgentConfig.validate()`, prints summary | `config.load_config()` |
| `chatixia pair <code>` | Redeems a 6-digit invite code via `POST /api/pairing/pair` | `cli._cmd_pair()` |
| `chatixia run [manifest]` | Starts the full agent lifecycle (Sections 6.1--6.5) | `runner.run_agent()` |

### 8.4 The Scaffolded Agent

`chatixia init my-agent` generates three files:

**agent.yaml** -- the manifest (see Section 5.1). The default configuration includes four built-in skills (delegate, list_agents, mesh_send, mesh_broadcast), points to a local registry at `http://localhost:8080`, and uses Azure OpenAI as the LLM provider. The sidecar socket path is unique per agent name (`/tmp/chatixia-{name}.sock`).

**.env.example** -- template for environment variables. Includes placeholders for Azure OpenAI, OpenAI, and Ollama credentials, plus registry and sidecar settings.

**.gitignore** -- excludes `.env`, `.chatixia/`, and Python cache files.

---

## 9. Putting It All Together

Here is the complete picture of how all the pieces connect:

```
+------------------------------------------------------------------+
|                        Registry (Rust)                            |
|  +-------------------+  +------------------+  +---------------+  |
|  | Agent Registry    |  | Task Queue       |  | Signaling     |  |
|  | - agent records   |  | - pending tasks  |  | - SDP relay   |  |
|  | - skill routing   |  | - task lifecycle |  | - ICE relay   |  |
|  | - health checks   |  | - heartbeat poll |  | - peer track  |  |
|  +-------------------+  +------------------+  +---------------+  |
+----^-----------^--------------------^-------------------^--------+
     |           |                    |                   |
     | HTTP      | HTTP               | HTTP              | WebSocket
     | (control) | (fallback data)    | (heartbeat)       | (signaling)
     |           |                    |                   |
+----+-----------+--------------------+-------------------+--------+
|    Agent (Python)                   Sidecar (Rust)                |
|  +-------------------+           +---------------------------+   |
|  | AgentConfig       |           | WebRTC Peer               |   |
|  | SKILL_HANDLERS    |  IPC      | - DTLS encryption         |   |
|  | _handle_p2p_msg   |<--------->| - DataChannel management  |   |
|  | _execute_task     |  (Unix    | - Peer lifecycle events   |   |
|  | heartbeat loop    |  socket)  | - Message relay           |   |
|  +-------------------+           +---------------------------+   |
+------------------------------------------------------------------+
                                       |
                                       | WebRTC DataChannel
                                       | (P2P, DTLS encrypted)
                                       |
                                       v
                              Other Agent+Sidecar pairs
```

The agent is the application logic. The sidecar is the network layer. The registry is the control plane. Data flows P2P whenever possible, through the registry only when necessary.

---

## Exercises

### Exercise 1: Design a "summarize" Skill

Design a new skill called `summarize` that takes a text input and returns a summary. You need two things:

**Part A:** Write the `skill.json` definition. Include the skill name, description, version, category, and parameters. The skill should accept a `text` parameter (required, string) and a `max_length` parameter (optional, integer, default 200).

**Part B:** Write a skeleton handler function. It should:
- Accept `text`, `max_length`, and `**kwargs`
- Validate that `text` is provided
- Return a placeholder result (do not implement actual summarization)
- Be synchronous (it does not need the MeshClient)

Then answer: where would you register this handler so the agent can execute it? What line in which file would you modify?

### Exercise 2: Trace a Complete Task Flow

A user opens the hub dashboard and types "What agents are online?" into the intervention panel for agent `coordinator-01`. Trace the complete message flow from the user's keypress to the result appearing in the dashboard.

List every HTTP request, IPC message, and DataChannel message in order. For each, specify:
- The sender and receiver
- The protocol (HTTP, IPC, or DataChannel)
- The message type or endpoint
- The payload (summarized)

Hint: the hub submits intervention tasks via `POST /api/hub/tasks`. The agent picks them up during heartbeat. The result is posted back via `POST /api/hub/tasks/{id}`. The hub polls `GET /api/hub/tasks/all` to refresh the display.

### Exercise 3: The Heartbeat Blocking Problem

The heartbeat loop runs every 15 seconds. Consider this scenario:

1. Agent receives a task via heartbeat at T=0
2. The task's skill handler takes 30 seconds to complete
3. The next heartbeat is scheduled for T=15

**Part A:** If `_execute_task` were called with `await` (not `asyncio.create_task`), what would happen? When would the next heartbeat fire? Would the agent appear offline?

**Part B:** With the current `asyncio.create_task()` approach, what happens? Can two tasks execute concurrently? What is the maximum time the agent would appear unresponsive to the registry?

**Part C:** There is a subtlety in the current code. The heartbeat HTTP request itself (`requests.post(...)`) is a synchronous blocking call. Does `asyncio.create_task` fix this? What would you change to make the heartbeat fully non-blocking?

### Exercise 4: chatixia-mesh vs Google A2A

Google's Agent-to-Agent (A2A) protocol defines a standard for agent interoperability. Research the A2A protocol specification (https://google.github.io/A2A/) and compare it with chatixia-mesh's agent model.

Answer these questions:

1. A2A uses "Agent Cards" for discovery. chatixia-mesh has a `/.well-known/agent.json` endpoint in the registry. How do these compare? What fields would chatixia-mesh need to add to its Agent Card for A2A compliance?

2. A2A defines a task lifecycle (submitted, working, input-required, completed, failed, canceled). How does chatixia-mesh's task lifecycle compare? What states are missing?

3. A2A uses HTTP as the transport. chatixia-mesh uses WebRTC DataChannels. If you wanted to make chatixia-mesh A2A-compliant, would you add A2A's HTTP endpoints alongside the existing DataChannel path, or replace the DataChannel path? Justify your choice.

4. A2A supports "streaming" via Server-Sent Events (SSE). chatixia-mesh's protocol includes `task_stream_chunk` and `agent_stream_chunk` message types (defined in the sidecar's protocol module). How would you bridge SSE streaming to DataChannel streaming for a client that speaks A2A?

---

## Summary

- An AI agent is an LLM-powered system that reasons, acts (via tools), and observes results in a loop.
- chatixia-mesh models agent capabilities as **skills** -- named handlers with parameter schemas, registered in the `SKILL_HANDLERS` dictionary and advertised to the registry.
- Six built-in skills cover discovery (`list_agents`, `find_agent`), communication (`mesh_send`, `mesh_broadcast`), delegation (`delegate`), and human interaction (`user_intervention`).
- Sync handlers (`def`) are used for HTTP-only control plane operations. Async handlers (`async def`) are used when the MeshClient is needed for P2P communication. The runner handles both uniformly.
- Agent configuration lives in `agent.yaml`, parsed into the `AgentConfig` dataclass. Sensitive values go in `.env`.
- The agent lifecycle is: load config, register, connect mesh, heartbeat loop, shutdown. Tasks from the heartbeat are dispatched with `asyncio.create_task()` for non-blocking execution.
- Multi-agent collaboration follows a discover-delegate-execute pattern, with P2P DataChannels as the primary transport and HTTP task queue as fallback.

---

## Further Reading

- **Lesson 06** -- [Inter-Process Communication](06-inter-process-communication.md) for details on the Unix socket IPC protocol between agent and sidecar
- **Lesson 07** -- [Application Protocol Design](07-application-protocol-design.md) for the MeshMessage format and request/response correlation
- **Lesson 10** -- [The Sidecar Pattern](10-sidecar-pattern.md) for why networking is isolated in a separate Rust process
- **ADR-005** -- Hub Task Queue for Sync Skill Handlers (the original design)
- **ADR-013** -- Heartbeat-Driven Task Execution
- **ADR-016** -- P2P Task Execution via DataChannels (the async evolution)
- MCP specification: https://modelcontextprotocol.io/
- Google A2A protocol: https://google.github.io/A2A/
