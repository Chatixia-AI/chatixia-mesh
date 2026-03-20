"""Mesh-aware skills — delegate, discover, and communicate over WebRTC mesh.

These skills replace the HTTP-based delegate/list_agents from chatixia-agent
with WebRTC DataChannel equivalents that go through the Rust sidecar.
"""

from __future__ import annotations

import json
import os
import time
import urllib.request
import uuid
from typing import Any


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


# ─── List Agents ──────────────────────────────────────────────────────────


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


# ─── Route by Skill ──────────────────────────────────────────────────────


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


# ─── Delegate via Mesh ───────────────────────────────────────────────────


def handle_delegate(
    message: str = "",
    target_agent_id: str = "",
    skill: str = "",
    wait: bool = True,
    **kwargs,
) -> str:
    """Delegate a task to another agent. Uses registry for routing, hub for task queue."""
    registry = _registry_url()
    agent_id = os.environ.get("CHATIXIA_AGENT_ID", "unknown")

    if not message:
        return "Error: 'message' parameter is required."

    # If no target specified, try to route by skill
    if not target_agent_id and skill:
        route = _get(f"{registry}/api/registry/route?skill={skill}")
        if "error" not in route:
            target_agent_id = route.get("agent_id", "")

    if not target_agent_id:
        return "Error: Could not determine target agent. Specify target_agent_id or a valid skill."

    # Submit task to hub
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
        return f"Task submitted: task_id={task_id}"

    # Poll for result
    deadline = time.time() + 120
    while time.time() < deadline:
        time.sleep(3)
        status = _get(f"{registry}/api/hub/tasks/{task_id}")
        state = status.get("state", "pending")
        if state == "completed":
            return status.get("result", "(no result)")
        if state == "failed":
            return f"Task failed: {status.get('error', 'unknown')}"

    return f"Timeout: task {task_id} still pending after 120s"


# ─── Mesh Send ────────────────────────────────────────────────────────────


def handle_mesh_send(
    target_agent_id: str = "",
    message: str = "",
    **kwargs,
) -> str:
    """Send a direct message to another agent over the WebRTC mesh.

    This uses the IPC bridge to the Rust sidecar, which sends over DataChannel.
    For synchronous skill handlers, we use the hub task queue as fallback.
    """
    if not target_agent_id or not message:
        return "Error: 'target_agent_id' and 'message' are required."

    registry = _registry_url()
    agent_id = os.environ.get("CHATIXIA_AGENT_ID", "unknown")

    # Use hub task queue (sync-compatible fallback)
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
    return f"Message sent to {target_agent_id} (task_id={task_id})"


# ─── Mesh Broadcast ──────────────────────────────────────────────────────


def handle_mesh_broadcast(message: str = "", **kwargs) -> str:
    """Broadcast a message to all agents in the mesh."""
    if not message:
        return "Error: 'message' parameter is required."

    registry = _registry_url()
    agents = _get(f"{registry}/api/registry/agents")

    if isinstance(agents, dict) and "error" in agents:
        return f"Error: {agents['error']}"

    agent_id = os.environ.get("CHATIXIA_AGENT_ID", "unknown")
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

    return f"Broadcast sent to {sent} agent(s)"
