"""Mesh-aware skills — delegate, discover, and communicate over WebRTC mesh.

Skills use P2P DataChannels via the MeshClient when available, with automatic
fallback to the registry HTTP task queue when peers are not directly reachable.
Discovery (list_agents, find_agent) always uses the registry — that's control plane.
"""

from __future__ import annotations

import asyncio
import json
import logging
import os
import urllib.request
from typing import TYPE_CHECKING, Any

if TYPE_CHECKING:
    from chatixia.core.mesh_client import MeshClient

logger = logging.getLogger("chatixia.mesh_skills")


def _registry_url() -> str:
    """Get the registry server URL."""
    return os.environ.get("CHATIXIA_REGISTRY_URL", "http://localhost:8080")


def _get(url: str) -> dict[str, Any]:
    req = urllib.request.Request(url, method="GET")
    try:
        with urllib.request.urlopen(req, timeout=10) as resp:
            return json.loads(resp.read().decode())
    except Exception as e:
        return {"error": str(e)}


def _post(url: str, data: dict) -> dict:
    body = json.dumps(data).encode()
    req = urllib.request.Request(
        url, data=body, headers={"Content-Type": "application/json"}, method="POST"
    )
    try:
        with urllib.request.urlopen(req, timeout=10) as resp:
            return json.loads(resp.read().decode())
    except Exception as e:
        return {"error": str(e)}


# ─── List Agents (control plane — always HTTP) ──────────────────────────


def handle_list_agents(**kwargs) -> str:
    """List all agents registered in the mesh."""
    registry = _registry_url()
    agents = _get(f"{registry}/api/registry/agents")

    if isinstance(agents, dict) and "error" in agents:
        return f"Error: {agents['error']}"

    if not agents:
        return "No agents connected to the mesh."

    agent_id = os.environ.get("CHATIXIA_AGENT_ID", "")
    lines = ["## Mesh Agents\n"]
    for a in agents:
        info = a if isinstance(a, dict) else {}
        aid = info.get("agent_id", "unknown")
        health = info.get("health", "unknown")
        mode = info.get("mode", "")
        hostname = info.get("hostname", "")
        ip = info.get("ip", "")
        port = info.get("port", "")
        skills = info.get("capabilities", {}).get("skills", [])
        peer_id = info.get("sidecar_peer_id", "")
        you = " **(you)**" if aid == agent_id else ""

        lines.append(f"- **{aid}**{you} [{health}]")
        lines.append(f"  host: {hostname} ({ip}:{port}), mode: {mode}")
        lines.append(f"  peer: {peer_id}")
        lines.append(f"  skills: {', '.join(skills[:10])}")
        if len(skills) > 10:
            lines.append(f"  ... and {len(skills) - 10} more")
        lines.append("")

    return "\n".join(lines)


# ─── Route by Skill (control plane — always HTTP) ───────────────────────


def handle_find_agent(skill: str = "", **kwargs) -> str:
    """Find the best agent for a specific skill."""
    if not skill:
        return "Error: 'skill' parameter is required."

    registry = _registry_url()
    result = _get(f"{registry}/api/registry/route?skill={skill}")

    if "error" in result:
        return f"No agent found with skill '{skill}': {result['error']}"

    aid = result.get("agent_id", "unknown")
    peer = result.get("sidecar_peer_id", "")
    return f"Agent '{aid}' (peer: {peer}) has skill '{skill}'"


# ─── Delegate via Mesh (P2P with registry fallback) ─────────────────────


async def handle_delegate(
    message: str = "",
    target_agent_id: str = "",
    skill: str = "",
    wait: bool = True,
    _mesh_client: MeshClient | None = None,
    **kwargs,
) -> str:
    """Delegate a task to another agent. Uses P2P DataChannel with registry fallback."""
    registry = _registry_url()
    agent_id = os.environ.get("CHATIXIA_AGENT_ID", "unknown")

    if not message:
        return "Error: 'message' parameter is required."

    # Route by skill if no target specified (discovery = control plane, HTTP is correct)
    if not target_agent_id and skill:
        route = _get(f"{registry}/api/registry/route?skill={skill}")
        if "error" not in route:
            target_agent_id = route.get("agent_id", "")

    if not target_agent_id:
        return "Error: Could not determine target agent. Specify target_agent_id or a valid skill."

    # ── P2P path: send task_request via DataChannel, await task_response ──
    if _mesh_client and _mesh_client.connected:
        from chatixia.core.mesh_client import MeshMessage

        target_peer = f"{target_agent_id}-sidecar"

        if _mesh_client.is_peer_connected(target_peer):
            msg = MeshMessage(
                msg_type="task_request",
                source_agent=agent_id,
                target_agent=target_agent_id,
                payload={"message": message, "skill": skill},
            )

            if not wait:
                await _mesh_client.send(target_peer, msg)
                return f"Task delegated to {target_agent_id} via P2P (fire-and-forget)"

            try:
                response = await _mesh_client.request(target_peer, msg, timeout=120.0)
                payload = response.get("payload", {})
                error = payload.get("error", "")
                if error:
                    return f"Task failed: {error}"
                return payload.get("result", "(no result)")
            except asyncio.TimeoutError:
                return f"Timeout: P2P task to {target_agent_id} timed out after 120s"
            except Exception as e:
                logger.warning("P2P delegate failed, falling back to registry: %s", e)
                # Fall through to HTTP fallback

    # ── Fallback: HTTP task queue ────────────────────────────────────────
    result = _post(
        f"{registry}/api/hub/tasks",
        {
            "skill": skill,
            "target_agent_id": target_agent_id,
            "source_agent_id": agent_id,
            "payload": {"message": message},
            "ttl": 300,
        },
    )

    task_id = result.get("task_id", "")
    if not task_id:
        return f"Error: Failed to submit task: {result}"

    if not wait:
        return f"Task submitted via registry: task_id={task_id}"

    # Poll for result (async — no longer blocks the event loop)
    deadline = asyncio.get_event_loop().time() + 120
    while asyncio.get_event_loop().time() < deadline:
        await asyncio.sleep(3)
        status = _get(f"{registry}/api/hub/tasks/{task_id}")
        state = status.get("state", "pending")
        if state == "completed":
            return status.get("result", "(no result)")
        if state == "failed":
            return f"Task failed: {status.get('error', 'unknown')}"

    return f"Timeout: task {task_id} still pending after 120s"


# ─── Mesh Send (P2P with registry fallback) ─────────────────────────────


async def handle_mesh_send(
    target_agent_id: str = "",
    message: str = "",
    _mesh_client: MeshClient | None = None,
    **kwargs,
) -> str:
    """Send a direct message to another agent over the WebRTC mesh."""
    if not target_agent_id or not message:
        return "Error: 'target_agent_id' and 'message' are required."

    agent_id = os.environ.get("CHATIXIA_AGENT_ID", "unknown")

    # ── P2P path: send via DataChannel ───────────────────────────────────
    if _mesh_client and _mesh_client.connected:
        from chatixia.core.mesh_client import MeshMessage

        target_peer = f"{target_agent_id}-sidecar"

        if _mesh_client.is_peer_connected(target_peer):
            msg = MeshMessage(
                msg_type="agent_prompt",
                source_agent=agent_id,
                target_agent=target_agent_id,
                payload={"message": message, "direct": True},
            )
            await _mesh_client.send(target_peer, msg)
            return f"Message sent to {target_agent_id} via P2P DataChannel"

    # ── Fallback: HTTP task queue ────────────────────────────────────────
    registry = _registry_url()
    result = _post(
        f"{registry}/api/hub/tasks",
        {
            "skill": "",
            "target_agent_id": target_agent_id,
            "source_agent_id": agent_id,
            "payload": {"message": message, "direct": True},
            "ttl": 60,
        },
    )

    task_id = result.get("task_id", "")
    return f"Message sent to {target_agent_id} via registry (task_id={task_id})"


# ─── Mesh Broadcast (P2P with registry fallback) ────────────────────────


async def handle_mesh_broadcast(
    message: str = "",
    _mesh_client: MeshClient | None = None,
    **kwargs,
) -> str:
    """Broadcast a message to all agents in the mesh."""
    if not message:
        return "Error: 'message' parameter is required."

    agent_id = os.environ.get("CHATIXIA_AGENT_ID", "unknown")

    # ── P2P path: broadcast via DataChannel ──────────────────────────────
    if _mesh_client and _mesh_client.connected and _mesh_client.peers:
        from chatixia.core.mesh_client import MeshMessage

        msg = MeshMessage(
            msg_type="agent_prompt",
            source_agent=agent_id,
            target_agent="*",
            payload={"message": message, "broadcast": True},
        )
        await _mesh_client.broadcast(msg)
        peer_count = len(_mesh_client.peers)
        return f"Broadcast sent to {peer_count} peer(s) via P2P DataChannel"

    # ── Fallback: HTTP to each agent ─────────────────────────────────────
    registry = _registry_url()
    agents = _get(f"{registry}/api/registry/agents")

    if isinstance(agents, dict) and "error" in agents:
        return f"Error: {agents['error']}"

    sent = 0
    for a in agents:
        aid = a.get("agent_id", "")
        if aid and aid != agent_id and a.get("health") == "active":
            _post(
                f"{registry}/api/hub/tasks",
                {
                    "target_agent_id": aid,
                    "source_agent_id": agent_id,
                    "payload": {"message": message, "broadcast": True},
                    "ttl": 60,
                },
            )
            sent += 1

    return f"Broadcast sent to {sent} agent(s) via registry"
