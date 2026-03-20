"""Mesh broadcast skill handler — send to all agents."""

from core.mesh_skills import handle_mesh_broadcast


def handle(**kwargs) -> str:
    return handle_mesh_broadcast(**kwargs)
