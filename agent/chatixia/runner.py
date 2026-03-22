"""Agent runner — wires an ``AgentConfig`` to mesh components and runs the agent."""

from __future__ import annotations

import asyncio
import logging
import os
import signal
import socket
from typing import Any, Awaitable, Callable

import requests

from chatixia.config import AgentConfig
from chatixia.core.mesh_client import MeshClient, MeshMessage
from chatixia.core.mesh_skills import (
    handle_delegate,
    handle_find_agent,
    handle_list_agents,
    handle_mesh_broadcast,
    handle_mesh_send,
)

logger = logging.getLogger("chatixia.runner")

# Skill name → handler function (sync or async)
SKILL_HANDLERS: dict[str, Callable[..., str | Awaitable[str]]] = {
    "list_agents": handle_list_agents,
    "find_agent": handle_find_agent,
    "delegate": handle_delegate,
    "mesh_send": handle_mesh_send,
    "mesh_broadcast": handle_mesh_broadcast,
}


async def run_agent(config: AgentConfig) -> None:
    """Run an agent: register with registry, connect to mesh, heartbeat."""
    # Load .env if present
    env_path = config.resolve_path(".env")
    if env_path.exists():
        from dotenv import load_dotenv

        load_dotenv(env_path)

    registry = config.registry.rstrip("/")
    api_key = config.sidecar.api_key or os.environ.get("API_KEY", "ak_dev_001")
    agent_id = config.name

    # Export env vars for sidecar and skill handlers
    os.environ.setdefault("REGISTRY_URL", registry)
    os.environ.setdefault("CHATIXIA_REGISTRY_URL", registry)
    os.environ.setdefault("CHATIXIA_AGENT_ID", agent_id)
    os.environ.setdefault("API_KEY", api_key)

    # 1. Register with registry
    _register(registry, api_key, agent_id, config)
    print(f"Registered as {agent_id}")

    # 2. Connect to mesh via sidecar
    client = MeshClient(
        socket_path=config.sidecar.socket,
        sidecar_binary=config.sidecar.binary,
    )

    # Clean shutdown on signals
    loop = asyncio.get_running_loop()
    for sig in (signal.SIGINT, signal.SIGTERM):
        loop.add_signal_handler(
            sig,
            lambda: asyncio.create_task(_shutdown(client, registry, api_key, agent_id)),
        )

    await client.start()

    # Register handler for incoming P2P task requests via DataChannel
    async def _handle_p2p_message(data: dict[str, Any]) -> None:
        payload = data.get("payload", {})
        inner = payload.get("message", {})
        msg_type = inner.get("type", "")

        if msg_type != "task_request":
            return  # Not a task request — skip

        source_agent = inner.get("source_agent", "unknown")
        request_id = inner.get("request_id", "")
        task_payload = inner.get("payload", {})
        skill = task_payload.get("skill", "")
        from_peer = payload.get("from_peer", "")

        handler = SKILL_HANDLERS.get(skill)
        if handler is None:
            logger.warning("P2P: no handler for skill %r from %s", skill, source_agent)
            if request_id and client.connected:
                resp = MeshMessage(
                    msg_type="task_response",
                    request_id=request_id,
                    source_agent=agent_id,
                    target_agent=source_agent,
                    payload={"error": f"unknown skill: {skill}"},
                )
                await client.send(from_peer, resp)
            return

        logger.info(
            "P2P task from %s: skill=%s request_id=%s", source_agent, skill, request_id
        )
        try:
            task_payload["_mesh_client"] = client
            result = handler(**task_payload)
            if asyncio.iscoroutine(result):
                result = await result
            error_msg = ""
        except Exception as exc:
            logger.error("P2P task failed: %s", exc)
            result = ""
            error_msg = str(exc)

        # Send task_response back via P2P
        if request_id and client.connected:
            resp = MeshMessage(
                msg_type="task_response",
                request_id=request_id,
                source_agent=agent_id,
                target_agent=source_agent,
                payload={"result": result or "", "error": error_msg},
            )
            await client.send(from_peer, resp)

    client.on("message", _handle_p2p_message)

    print(f"Agent '{agent_id}' connected to mesh at {registry}")
    print(f"  Sidecar: {config.sidecar.socket}")
    print(f"  Skills:  {', '.join(config.skills_builtin) or '(none)'}")
    print()

    # 3. Heartbeat loop — also picks up assigned tasks (registry fallback path)
    while True:
        try:
            resp = requests.post(
                f"{registry}/api/hub/heartbeat",
                json={
                    "agent_id": agent_id,
                    "skill_names": config.skills_builtin,
                },
                headers={"x-api-key": api_key},
                timeout=5,
            )
            body = resp.json()
            for task in body.get("pending_tasks", []):
                # Non-blocking: spawn each task as a separate coroutine
                asyncio.create_task(
                    _execute_task(registry, api_key, task, mesh_client=client)
                )
        except Exception as exc:
            logger.debug("heartbeat failed: %s", exc)
        await asyncio.sleep(15)


async def _execute_task(
    registry: str,
    api_key: str,
    task: dict,
    mesh_client: MeshClient | None = None,
) -> None:
    """Execute an assigned task and report the result back to the hub."""
    task_id = task.get("id", "")
    skill = task.get("skill", "")
    payload = task.get("payload", {})
    source = task.get("source_agent_id", "?")

    handler = SKILL_HANDLERS.get(skill)
    if handler is None:
        logger.warning("no handler for skill %r (task %s)", skill, task_id)
        _update_task(
            registry, api_key, task_id, "failed", error=f"unknown skill: {skill}"
        )
        return

    logger.info("executing task %s: skill=%s from=%s", task_id, skill, source)
    try:
        if isinstance(payload, dict):
            payload["_mesh_client"] = mesh_client
            result = handler(**payload)
        else:
            result = handler(_mesh_client=mesh_client)

        # Await if the handler is async
        if asyncio.iscoroutine(result):
            result = await result

        logger.info("task %s completed", task_id)
        _update_task(registry, api_key, task_id, "completed", result=str(result))
    except Exception as exc:
        logger.error("task %s failed: %s", task_id, exc)
        _update_task(registry, api_key, task_id, "failed", error=str(exc))


def _update_task(
    registry: str,
    api_key: str,
    task_id: str,
    state: str,
    result: str = "",
    error: str = "",
) -> None:
    """POST task result back to the hub."""
    try:
        requests.post(
            f"{registry}/api/hub/tasks/{task_id}",
            json={"state": state, "result": result, "error": error},
            headers={"x-api-key": api_key},
            timeout=10,
        )
    except Exception as exc:
        logger.error("failed to update task %s: %s", task_id, exc)


def _register(
    registry: str,
    api_key: str,
    agent_id: str,
    config: AgentConfig,
) -> None:
    """Register this agent with the registry HTTP API."""
    resp = requests.post(
        f"{registry}/api/registry/agents",
        json={
            "agent_id": agent_id,
            "hostname": socket.gethostname(),
            "sidecar_peer_id": f"{agent_id}-sidecar",
            "capabilities": {
                "skills": config.skills_builtin,
                "mcp_servers": [],
                "goals_count": 0,
                "mode": "interactive",
            },
        },
        headers={"x-api-key": api_key},
        timeout=10,
    )
    resp.raise_for_status()


async def _shutdown(
    client: MeshClient,
    registry: str,
    api_key: str,
    agent_id: str,
) -> None:
    """Deregister and disconnect."""
    try:
        requests.delete(
            f"{registry}/api/registry/agents/{agent_id}",
            headers={"x-api-key": api_key},
            timeout=5,
        )
        print(f"\nDeregistered {agent_id}")
    except Exception:
        pass
    await client.stop()
    asyncio.get_running_loop().stop()
