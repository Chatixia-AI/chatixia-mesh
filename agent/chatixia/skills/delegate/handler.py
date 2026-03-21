"""Delegate skill handler — routes tasks to other agents via the mesh registry."""

from chatixia.core.mesh_skills import handle_delegate


def handle(**kwargs) -> str:
    return handle_delegate(**kwargs)
