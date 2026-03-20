import asyncio
import os
import signal
import socket

from dotenv import load_dotenv

load_dotenv()

import requests

from core.mesh_client import MeshClient

REGISTRY = os.environ.get("REGISTRY_URL", "http://localhost:8080")
API_KEY = os.environ.get("API_KEY", "ak_dev_001")
AGENT_ID = os.environ.get("AGENT_ID", f"agent-{socket.gethostname()}")


def register_with_registry():
    """Register this agent with the registry HTTP API."""
    resp = requests.post(
        f"{REGISTRY}/api/registry/agents",
        json={
            "agent_id": AGENT_ID,
            "hostname": socket.gethostname(),
            "sidecar_peer_id": "agent-001",
            "capabilities": {"skills": [], "mcp_servers": [], "goals_count": 0, "mode": "idle"},
            "status": "online",
            "mode": "idle",
        },
        headers={"x-api-key": API_KEY},
    )
    resp.raise_for_status()
    print(f"Registered as {AGENT_ID}")


def deregister():
    """Remove agent from registry on shutdown."""
    try:
        requests.delete(
            f"{REGISTRY}/api/registry/agents/{AGENT_ID}",
            headers={"x-api-key": API_KEY},
        )
        print(f"\nDeregistered {AGENT_ID}")
    except Exception:
        pass


async def main():
    register_with_registry()

    # Clean deregister on Ctrl+C
    loop = asyncio.get_running_loop()
    for sig in (signal.SIGINT, signal.SIGTERM):
        loop.add_signal_handler(sig, lambda: asyncio.create_task(shutdown(client)))

    client = MeshClient()
    await client.start()
    print("Agent connected to mesh")

    # Send heartbeats to stay active
    while True:
        try:
            requests.post(
                f"{REGISTRY}/api/hub/heartbeat",
                json={"agent_id": AGENT_ID},
                headers={"x-api-key": API_KEY},
            )
        except Exception:
            pass
        await asyncio.sleep(15)


async def shutdown(client: MeshClient):
    deregister()
    await client.stop()
    asyncio.get_running_loop().stop()


if __name__ == "__main__":
    asyncio.run(main())
