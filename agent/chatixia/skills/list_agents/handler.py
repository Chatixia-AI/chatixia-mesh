"""List agents skill handler — queries the mesh registry."""

from chatixia.core.mesh_skills import handle_list_agents


def handle(**kwargs) -> str:
    return handle_list_agents(**kwargs)
