import pytest
from chatixia.core.mesh_skills import (
    handle_list_agents,
    handle_find_agent,
    handle_delegate,
    handle_mesh_send,
    handle_mesh_broadcast,
    _registry_url,
)


class TestRegistryUrl:
    def test_default_url(self, monkeypatch):
        monkeypatch.delenv("CHATIXIA_REGISTRY_URL", raising=False)
        assert _registry_url() == "http://localhost:8080"

    def test_custom_url(self, monkeypatch):
        monkeypatch.setenv("CHATIXIA_REGISTRY_URL", "http://custom:9090")
        assert _registry_url() == "http://custom:9090"


class TestHandleFindAgent:
    def test_missing_skill(self):
        result = handle_find_agent()
        assert "Error" in result
        assert "skill" in result.lower()

    def test_empty_skill(self):
        result = handle_find_agent(skill="")
        assert "Error" in result


class TestHandleDelegate:
    def test_missing_message(self):
        result = handle_delegate()
        assert "Error" in result
        assert "message" in result.lower()

    def test_empty_message(self):
        result = handle_delegate(message="")
        assert "Error" in result

    def test_no_target_no_skill(self):
        result = handle_delegate(message="hello")
        assert "Error" in result
        assert "target" in result.lower()


class TestHandleMeshSend:
    def test_missing_target(self):
        result = handle_mesh_send(message="hello")
        assert "Error" in result

    def test_missing_message(self):
        result = handle_mesh_send(target_agent_id="agent-1")
        assert "Error" in result

    def test_both_missing(self):
        result = handle_mesh_send()
        assert "Error" in result


class TestHandleMeshBroadcast:
    def test_missing_message(self):
        result = handle_mesh_broadcast()
        assert "Error" in result
        assert "message" in result.lower()

    def test_empty_message(self):
        result = handle_mesh_broadcast(message="")
        assert "Error" in result
