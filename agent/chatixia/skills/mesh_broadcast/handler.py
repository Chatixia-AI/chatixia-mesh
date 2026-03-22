"""Mesh broadcast skill handler — broadcast to all agents via P2P mesh."""

from chatixia.core.mesh_skills import handle_mesh_broadcast


async def handle(**kwargs) -> str:
    return await handle_mesh_broadcast(**kwargs)
