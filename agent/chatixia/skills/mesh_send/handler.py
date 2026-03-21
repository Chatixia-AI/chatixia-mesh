"""Mesh send skill handler — direct P2P messaging."""

from chatixia.core.mesh_skills import handle_mesh_send


def handle(**kwargs) -> str:
    return handle_mesh_send(**kwargs)
