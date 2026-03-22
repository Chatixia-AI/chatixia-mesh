"""Delegate skill handler — routes tasks to other agents via P2P mesh."""

from chatixia.core.mesh_skills import handle_delegate


async def handle(**kwargs) -> str:
    return await handle_delegate(**kwargs)
