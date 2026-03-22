import os
import stat

import pytest

from chatixia.core.mesh_client import MeshMessage, MeshClient, _resolve_sidecar_binary


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
        assert client._peers == set()

    def test_init_custom_socket(self):
        client = MeshClient(socket_path="/tmp/custom.sock")
        assert client._socket_path == "/tmp/custom.sock"

    def test_connected_property(self):
        client = MeshClient()
        assert client.connected is False

    def test_on_registers_handler(self):
        client = MeshClient()

        def handler(data):
            return None

        client.on("message", handler)
        assert handler in client._handlers["message"]

    def test_on_wildcard_handler(self):
        client = MeshClient()

        def handler(data):
            return None

        client.on("*", handler)
        assert handler in client._handlers["*"]


class TestPeerTracking:
    def test_on_peer_connected(self):
        client = MeshClient()
        client._on_peer_connected({"payload": {"peer_id": "peer-abc"}})
        assert "peer-abc" in client._peers

    def test_on_peer_connected_ignores_empty(self):
        client = MeshClient()
        client._on_peer_connected({"payload": {"peer_id": ""}})
        assert len(client._peers) == 0

    def test_on_peer_disconnected(self):
        client = MeshClient()
        client._peers.add("peer-abc")
        client._on_peer_disconnected({"payload": {"peer_id": "peer-abc"}})
        assert "peer-abc" not in client._peers

    def test_on_peer_disconnected_nonexistent(self):
        client = MeshClient()
        # Should not raise
        client._on_peer_disconnected({"payload": {"peer_id": "ghost"}})
        assert len(client._peers) == 0

    def test_on_peer_list(self):
        client = MeshClient()
        client._peers.add("old-peer")
        client._on_peer_list({"payload": {"peers": ["a", "b", "c"]}})
        assert client._peers == {"a", "b", "c"}

    def test_on_peer_list_empty(self):
        client = MeshClient()
        client._peers.add("old")
        client._on_peer_list({"payload": {"peers": []}})
        assert client._peers == set()

    def test_is_peer_connected(self):
        client = MeshClient()
        client._peers.add("peer-x")
        assert client.is_peer_connected("peer-x") is True
        assert client.is_peer_connected("peer-y") is False

    def test_peers_property_returns_copy(self):
        client = MeshClient()
        client._peers.add("p1")
        peers = client.peers
        peers.add("p2")
        assert "p2" not in client._peers


class TestResolveSidecarBinary:
    def test_absolute_path_exists(self, tmp_path):
        binary = tmp_path / "chatixia-sidecar"
        binary.write_text("#!/bin/sh\n")
        binary.chmod(binary.stat().st_mode | stat.S_IEXEC)
        result = _resolve_sidecar_binary(str(binary))
        assert result == str(binary.resolve())

    def test_absolute_path_not_found_raises(self, tmp_path, monkeypatch):
        monkeypatch.delenv("SIDECAR_BINARY", raising=False)
        # Ensure nothing in PATH matches
        monkeypatch.setenv("PATH", str(tmp_path))
        with pytest.raises(RuntimeError, match="not found"):
            _resolve_sidecar_binary("/nonexistent/chatixia-sidecar")

    def test_env_var_override(self, tmp_path, monkeypatch):
        binary = tmp_path / "my-sidecar"
        binary.write_text("#!/bin/sh\n")
        binary.chmod(binary.stat().st_mode | stat.S_IEXEC)
        monkeypatch.setenv("SIDECAR_BINARY", str(binary))
        # Pass a non-existent configured path
        result = _resolve_sidecar_binary("/nonexistent/sidecar")
        assert result == str(binary.resolve())

    def test_env_var_path_lookup(self, tmp_path, monkeypatch):
        """SIDECAR_BINARY as bare name found in PATH."""
        binary = tmp_path / "custom-sidecar"
        binary.write_text("#!/bin/sh\n")
        binary.chmod(binary.stat().st_mode | stat.S_IEXEC)
        monkeypatch.setenv("PATH", str(tmp_path))
        monkeypatch.setenv("SIDECAR_BINARY", "custom-sidecar")
        result = _resolve_sidecar_binary("/nonexistent/sidecar")
        assert result.endswith("custom-sidecar")

    def test_path_lookup_bare_name(self, tmp_path, monkeypatch):
        binary = tmp_path / "chatixia-sidecar"
        binary.write_text("#!/bin/sh\n")
        binary.chmod(binary.stat().st_mode | stat.S_IEXEC)
        monkeypatch.delenv("SIDECAR_BINARY", raising=False)
        monkeypatch.setenv("PATH", str(tmp_path))
        result = _resolve_sidecar_binary("chatixia-sidecar")
        assert result.endswith("chatixia-sidecar")

    def test_not_executable_skipped(self, tmp_path, monkeypatch):
        """A file that exists but isn't executable should be skipped."""
        binary = tmp_path / "chatixia-sidecar"
        binary.write_text("not executable")
        binary.chmod(0o644)
        monkeypatch.delenv("SIDECAR_BINARY", raising=False)
        monkeypatch.setenv("PATH", "")
        with pytest.raises(RuntimeError, match="not found"):
            _resolve_sidecar_binary(str(binary))

    def test_error_message_contains_install_instructions(self, tmp_path, monkeypatch):
        monkeypatch.delenv("SIDECAR_BINARY", raising=False)
        monkeypatch.setenv("PATH", str(tmp_path))
        with pytest.raises(RuntimeError, match="cargo install"):
            _resolve_sidecar_binary("nonexistent-binary")
