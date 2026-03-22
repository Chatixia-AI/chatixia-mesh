"""Tests for chatixia.runner — env var derivation and registration helpers."""

from unittest.mock import MagicMock, patch

import pytest

from chatixia.config import AgentConfig, SidecarConfig


class TestEnvVarDerivation:
    """Test that run_agent derives SIGNALING_URL and TOKEN_URL from registry."""

    def _run_env_setup(self, monkeypatch, registry: str) -> dict[str, str]:
        """Simulate the env var setup portion of run_agent and return env state.

        We patch _register and MeshClient to avoid needing a live registry.
        """
        import os

        # Clear relevant env vars so setdefault takes effect
        for var in (
            "REGISTRY_URL",
            "CHATIXIA_REGISTRY_URL",
            "CHATIXIA_AGENT_ID",
            "API_KEY",
            "SIGNALING_URL",
            "TOKEN_URL",
        ):
            monkeypatch.delenv(var, raising=False)

        config = AgentConfig(
            name="test-agent",
            registry=registry,
            sidecar=SidecarConfig(api_key="ak_test"),
        )

        registry_clean = config.registry.rstrip("/")
        api_key = config.sidecar.api_key or "ak_dev_001"
        agent_id = config.name

        os.environ.setdefault("REGISTRY_URL", registry_clean)
        os.environ.setdefault("CHATIXIA_REGISTRY_URL", registry_clean)
        os.environ.setdefault("CHATIXIA_AGENT_ID", agent_id)
        os.environ.setdefault("API_KEY", api_key)

        ws_scheme = "wss" if registry_clean.startswith("https") else "ws"
        ws_base = registry_clean.replace("https://", "").replace("http://", "")
        os.environ.setdefault("SIGNALING_URL", f"{ws_scheme}://{ws_base}/ws")
        os.environ.setdefault("TOKEN_URL", f"{registry_clean}/api/token")

        return {
            "REGISTRY_URL": os.environ["REGISTRY_URL"],
            "SIGNALING_URL": os.environ["SIGNALING_URL"],
            "TOKEN_URL": os.environ["TOKEN_URL"],
            "API_KEY": os.environ["API_KEY"],
        }

    def test_default_port(self, monkeypatch):
        env = self._run_env_setup(monkeypatch, "http://localhost:8080")
        assert env["SIGNALING_URL"] == "ws://localhost:8080/ws"
        assert env["TOKEN_URL"] == "http://localhost:8080/api/token"

    def test_custom_port(self, monkeypatch):
        env = self._run_env_setup(monkeypatch, "http://localhost:9090")
        assert env["SIGNALING_URL"] == "ws://localhost:9090/ws"
        assert env["TOKEN_URL"] == "http://localhost:9090/api/token"

    def test_remote_host(self, monkeypatch):
        env = self._run_env_setup(monkeypatch, "http://192.168.1.100:8080")
        assert env["SIGNALING_URL"] == "ws://192.168.1.100:8080/ws"
        assert env["TOKEN_URL"] == "http://192.168.1.100:8080/api/token"

    def test_https_registry(self, monkeypatch):
        env = self._run_env_setup(monkeypatch, "https://mesh.example.com")
        assert env["SIGNALING_URL"] == "wss://mesh.example.com/ws"
        assert env["TOKEN_URL"] == "https://mesh.example.com/api/token"

    def test_trailing_slash_stripped(self, monkeypatch):
        env = self._run_env_setup(monkeypatch, "http://localhost:8080/")
        assert env["SIGNALING_URL"] == "ws://localhost:8080/ws"
        assert env["TOKEN_URL"] == "http://localhost:8080/api/token"

    def test_existing_env_not_overwritten(self, monkeypatch):
        """If SIGNALING_URL is already set, setdefault should not overwrite it."""
        import os

        # Clear everything first, then set the ones we want to preserve
        for var in ("REGISTRY_URL", "CHATIXIA_REGISTRY_URL", "CHATIXIA_AGENT_ID",
                    "API_KEY", "SIGNALING_URL", "TOKEN_URL"):
            monkeypatch.delenv(var, raising=False)

        monkeypatch.setenv("SIGNALING_URL", "ws://custom:1234/ws")
        monkeypatch.setenv("TOKEN_URL", "http://custom:1234/api/token")

        registry = "http://localhost:9090"
        os.environ.setdefault("REGISTRY_URL", registry)
        os.environ.setdefault("CHATIXIA_REGISTRY_URL", registry)
        os.environ.setdefault("CHATIXIA_AGENT_ID", "test")
        os.environ.setdefault("API_KEY", "ak_test")
        ws_scheme = "ws"
        ws_base = "localhost:9090"
        os.environ.setdefault("SIGNALING_URL", f"{ws_scheme}://{ws_base}/ws")
        os.environ.setdefault("TOKEN_URL", f"{registry}/api/token")

        assert os.environ["SIGNALING_URL"] == "ws://custom:1234/ws"
        assert os.environ["TOKEN_URL"] == "http://custom:1234/api/token"

    def test_api_key_from_config(self, monkeypatch):
        env = self._run_env_setup(monkeypatch, "http://localhost:8080")
        assert env["API_KEY"] == "ak_test"


class TestRegisterHelper:
    """Test the _register function."""

    @patch("chatixia.runner.requests.post")
    def test_register_sends_correct_payload(self, mock_post):
        from chatixia.runner import _register

        mock_resp = MagicMock()
        mock_resp.raise_for_status = MagicMock()
        mock_post.return_value = mock_resp

        config = AgentConfig(
            name="reg-agent",
            skills_builtin=["delegate", "mesh_send"],
        )
        _register("http://localhost:8080", "ak_test", "reg-agent", config)

        mock_post.assert_called_once()
        call_kwargs = mock_post.call_args
        json_body = call_kwargs.kwargs.get("json") or call_kwargs[1].get("json")
        assert json_body["agent_id"] == "reg-agent"
        assert json_body["capabilities"]["skills"] == ["delegate", "mesh_send"]
        assert json_body["capabilities"]["mode"] == "interactive"

    @patch("chatixia.runner.requests.post")
    def test_register_includes_api_key_header(self, mock_post):
        from chatixia.runner import _register

        mock_resp = MagicMock()
        mock_resp.raise_for_status = MagicMock()
        mock_post.return_value = mock_resp

        config = AgentConfig(name="test")
        _register("http://localhost:8080", "ak_secret", "test", config)

        call_kwargs = mock_post.call_args
        headers = call_kwargs.kwargs.get("headers") or call_kwargs[1].get("headers")
        assert headers["x-api-key"] == "ak_secret"

    @patch("chatixia.runner.requests.post")
    def test_register_raises_on_http_error(self, mock_post):
        from chatixia.runner import _register

        mock_resp = MagicMock()
        mock_resp.raise_for_status.side_effect = Exception("401 Unauthorized")
        mock_post.return_value = mock_resp

        config = AgentConfig(name="test")
        with pytest.raises(Exception, match="401"):
            _register("http://localhost:8080", "bad_key", "test", config)


class TestUpdateTask:
    """Test the _update_task helper."""

    @patch("chatixia.runner.requests.post")
    def test_update_task_completed(self, mock_post):
        from chatixia.runner import _update_task

        mock_post.return_value = MagicMock()
        _update_task("http://localhost:8080", "ak_test", "task-123", "completed", result="done")

        call_kwargs = mock_post.call_args
        json_body = call_kwargs.kwargs.get("json") or call_kwargs[1].get("json")
        assert json_body["state"] == "completed"
        assert json_body["result"] == "done"

    @patch("chatixia.runner.requests.post")
    def test_update_task_failed(self, mock_post):
        from chatixia.runner import _update_task

        mock_post.return_value = MagicMock()
        _update_task("http://localhost:8080", "ak_test", "task-456", "failed", error="timeout")

        call_kwargs = mock_post.call_args
        json_body = call_kwargs.kwargs.get("json") or call_kwargs[1].get("json")
        assert json_body["state"] == "failed"
        assert json_body["error"] == "timeout"

    @patch("chatixia.runner.requests.post")
    def test_update_task_swallows_network_errors(self, mock_post):
        from chatixia.runner import _update_task

        mock_post.side_effect = ConnectionError("network down")
        # Should not raise
        _update_task("http://localhost:8080", "ak_test", "task-789", "failed", error="oops")
