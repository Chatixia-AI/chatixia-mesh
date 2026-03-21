"""Agent runner — wires an ``AgentConfig`` to mesh components and runs the agent."""

from __future__ import annotations

import asyncio
import logging
import os
import signal
import socket

import requests

from chatixia.config import AgentConfig
from core.mesh_client import MeshClient

logger = logging.getLogger("chatixia.runner")


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
    print(f"Agent '{agent_id}' connected to mesh at {registry}")
    print(f"  Sidecar: {config.sidecar.socket}")
    print(f"  Skills:  {', '.join(config.skills_builtin) or '(none)'}")
    print()

    # 3. Heartbeat loop
    while True:
        try:
            requests.post(
                f"{registry}/api/hub/heartbeat",
                json={
                    "agent_id": agent_id,
                    "skill_names": config.skills_builtin,
                },
                headers={"x-api-key": api_key},
                timeout=5,
            )
        except Exception as exc:
            logger.debug("heartbeat failed: %s", exc)
        await asyncio.sleep(15)


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
