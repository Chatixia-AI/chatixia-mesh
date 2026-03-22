import asyncio
from unittest.mock import AsyncMock, MagicMock

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


# ─── Sync handlers (control plane — unchanged) ──────────────────────────


class TestHandleFindAgent:
    def test_missing_skill(self):
        result = handle_find_agent()
        assert "Error" in result
        assert "skill" in result.lower()

    def test_empty_skill(self):
        result = handle_find_agent(skill="")
        assert "Error" in result


# ─── Async handlers (P2P data plane) ────────────────────────────────────


class TestHandleDelegate:
    @pytest.mark.asyncio
    async def test_missing_message(self):
        result = await handle_delegate()
        assert "Error" in result
        assert "message" in result.lower()

    @pytest.mark.asyncio
    async def test_empty_message(self):
        result = await handle_delegate(message="")
        assert "Error" in result

    @pytest.mark.asyncio
    async def test_no_target_no_skill(self):
        result = await handle_delegate(message="hello")
        assert "Error" in result
        assert "target" in result.lower()

    @pytest.mark.asyncio
    async def test_p2p_path_fire_and_forget(self):
        """When MeshClient is connected and peer is reachable, delegate via P2P."""
        mock_client = MagicMock()
        mock_client.connected = True
        mock_client.is_peer_connected.return_value = True
        mock_client.send = AsyncMock()

        result = await handle_delegate(
            message="do something",
            target_agent_id="agent-b",
            skill="research",
            wait=False,
            _mesh_client=mock_client,
        )
        assert "P2P" in result
        mock_client.send.assert_called_once()

    @pytest.mark.asyncio
    async def test_p2p_path_request_response(self):
        """P2P delegate with wait=True uses request() for response."""
        mock_client = MagicMock()
        mock_client.connected = True
        mock_client.is_peer_connected.return_value = True
        mock_client.request = AsyncMock(
            return_value={"payload": {"result": "done", "error": ""}}
        )

        result = await handle_delegate(
            message="do something",
            target_agent_id="agent-b",
            wait=True,
            _mesh_client=mock_client,
        )
        assert result == "done"
        mock_client.request.assert_called_once()

    @pytest.mark.asyncio
    async def test_fallback_when_no_client(self):
        """Without MeshClient, falls back to registry HTTP (which will fail without server)."""
        result = await handle_delegate(
            message="hello", target_agent_id="agent-b", _mesh_client=None
        )
        # Without a running registry, this will get an HTTP error, but it should not raise
        assert isinstance(result, str)

    @pytest.mark.asyncio
    async def test_fallback_when_peer_not_connected(self):
        """When peer is not reachable via P2P, falls back to registry."""
        mock_client = MagicMock()
        mock_client.connected = True
        mock_client.is_peer_connected.return_value = False

        result = await handle_delegate(
            message="hello", target_agent_id="agent-b", _mesh_client=mock_client
        )
        # Falls back to HTTP — will fail without server, but should not raise
        assert isinstance(result, str)


class TestHandleMeshSend:
    @pytest.mark.asyncio
    async def test_missing_target(self):
        result = await handle_mesh_send(message="hello")
        assert "Error" in result

    @pytest.mark.asyncio
    async def test_missing_message(self):
        result = await handle_mesh_send(target_agent_id="agent-1")
        assert "Error" in result

    @pytest.mark.asyncio
    async def test_both_missing(self):
        result = await handle_mesh_send()
        assert "Error" in result

    @pytest.mark.asyncio
    async def test_p2p_path(self):
        """When MeshClient is connected and peer reachable, send via P2P."""
        mock_client = MagicMock()
        mock_client.connected = True
        mock_client.is_peer_connected.return_value = True
        mock_client.send = AsyncMock()

        result = await handle_mesh_send(
            target_agent_id="agent-b",
            message="hello",
            _mesh_client=mock_client,
        )
        assert "P2P DataChannel" in result
        mock_client.send.assert_called_once()

    @pytest.mark.asyncio
    async def test_fallback_when_peer_not_connected(self):
        """When peer is not reachable, falls back to registry."""
        mock_client = MagicMock()
        mock_client.connected = True
        mock_client.is_peer_connected.return_value = False

        result = await handle_mesh_send(
            target_agent_id="agent-b",
            message="hello",
            _mesh_client=mock_client,
        )
        # Falls back to HTTP — will fail without server, but should not raise
        assert "P2P" not in result


class TestHandleMeshBroadcast:
    @pytest.mark.asyncio
    async def test_missing_message(self):
        result = await handle_mesh_broadcast()
        assert "Error" in result
        assert "message" in result.lower()

    @pytest.mark.asyncio
    async def test_empty_message(self):
        result = await handle_mesh_broadcast(message="")
        assert "Error" in result

    @pytest.mark.asyncio
    async def test_p2p_path(self):
        """When MeshClient is connected with peers, broadcast via P2P."""
        mock_client = MagicMock()
        mock_client.connected = True
        mock_client.peers = {"peer-a-sidecar", "peer-b-sidecar"}
        mock_client.broadcast = AsyncMock()

        result = await handle_mesh_broadcast(
            message="hello all",
            _mesh_client=mock_client,
        )
        assert "P2P DataChannel" in result
        mock_client.broadcast.assert_called_once()

    @pytest.mark.asyncio
    async def test_fallback_when_no_peers(self):
        """When no peers connected, falls back to registry."""
        mock_client = MagicMock()
        mock_client.connected = True
        mock_client.peers = set()

        result = await handle_mesh_broadcast(
            message="hello all",
            _mesh_client=mock_client,
        )
        # Falls back to HTTP — will fail without server, but should not raise
        assert "P2P" not in result
