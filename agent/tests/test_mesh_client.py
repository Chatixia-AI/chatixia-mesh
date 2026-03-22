import pytest
from chatixia.core.mesh_client import MeshMessage, MeshClient


class TestMeshMessage:
    def test_to_dict(self):
        msg = MeshMessage(
            msg_type="task_request",
            request_id="req-123",
            source_agent="agent-a",
            target_agent="agent-b",
            payload={"key": "value"},
        )
        d = msg.to_dict()
        assert d["type"] == "task_request"
        assert d["request_id"] == "req-123"
        assert d["source_agent"] == "agent-a"
        assert d["target_agent"] == "agent-b"
        assert d["payload"] == {"key": "value"}

    def test_from_dict(self):
        d = {
            "type": "task_response",
            "request_id": "req-456",
            "source_agent": "agent-b",
            "target_agent": "agent-a",
            "payload": {"result": "done"},
        }
        msg = MeshMessage.from_dict(d)
        assert msg.msg_type == "task_response"
        assert msg.request_id == "req-456"
        assert msg.source_agent == "agent-b"
        assert msg.payload == {"result": "done"}

    def test_from_dict_defaults(self):
        msg = MeshMessage.from_dict({"type": "ping"})
        assert msg.msg_type == "ping"
        assert msg.request_id == ""
        assert msg.source_agent == ""
        assert msg.target_agent == ""
        assert msg.payload == {}

    def test_roundtrip(self):
        original = MeshMessage(
            msg_type="agent_prompt",
            request_id="abc",
            source_agent="s",
            target_agent="t",
            payload={"nested": {"deep": True}},
        )
        rebuilt = MeshMessage.from_dict(original.to_dict())
        assert rebuilt.msg_type == original.msg_type
        assert rebuilt.request_id == original.request_id
        assert rebuilt.payload == original.payload

    def test_from_empty_dict(self):
        msg = MeshMessage.from_dict({})
        assert msg.msg_type == ""


class TestMeshClient:
    def test_init_defaults(self):
        client = MeshClient()
        assert client._socket_path == "/tmp/chatixia-sidecar.sock"
        assert client._connected is False

    def test_init_custom_socket(self):
        client = MeshClient(socket_path="/tmp/custom.sock")
        assert client._socket_path == "/tmp/custom.sock"

    def test_connected_property(self):
        client = MeshClient()
        assert client.connected is False

    def test_on_registers_handler(self):
        client = MeshClient()
        handler = lambda data: None
        client.on("message", handler)
        assert handler in client._handlers["message"]

    def test_on_wildcard_handler(self):
        client = MeshClient()
        handler = lambda data: None
        client.on("*", handler)
        assert handler in client._handlers["*"]
