import pytest
from pathlib import Path
from chatixia.config import AgentConfig, SidecarConfig, load_config, _parse_config


class TestSidecarConfig:
    def test_defaults(self):
        sc = SidecarConfig()
        assert sc.binary == "chatixia-sidecar"
        assert sc.api_key == "ak_dev_001"
        assert sc.socket == "/tmp/chatixia-sidecar.sock"


class TestAgentConfig:
    def test_validate_valid(self):
        cfg = AgentConfig(name="test-agent")
        assert cfg.validate() == []

    def test_validate_missing_name(self):
        cfg = AgentConfig(name="")
        errors = cfg.validate()
        assert any("name" in e for e in errors)

    def test_validate_invalid_provider(self):
        cfg = AgentConfig(name="test", provider="invalid")
        errors = cfg.validate()
        assert any("provider" in e.lower() or "Unknown" in e for e in errors)

    def test_validate_valid_providers(self):
        for provider in ("azure", "openai", "ollama"):
            cfg = AgentConfig(name="test", provider=provider)
            assert cfg.validate() == []

    def test_validate_missing_registry(self):
        cfg = AgentConfig(name="test", registry="")
        errors = cfg.validate()
        assert any("registry" in e.lower() for e in errors)

    def test_resolve_path_absolute(self):
        cfg = AgentConfig(name="test", _source_dir=Path("/some/dir"))
        result = cfg.resolve_path("/absolute/path")
        assert result == Path("/absolute/path")

    def test_resolve_path_relative(self):
        cfg = AgentConfig(name="test", _source_dir=Path("/some/dir"))
        result = cfg.resolve_path("relative/path")
        assert result == Path("/some/dir/relative/path")

    def test_default_values(self):
        cfg = AgentConfig(name="test")
        assert cfg.registry == "http://localhost:8080"
        assert cfg.provider == "azure"
        assert cfg.max_turns == 10
        assert cfg.context_window == 120_000
        assert cfg.data_dir == ".chatixia"


class TestParseConfig:
    def test_minimal_config(self):
        raw = {"name": "my-agent"}
        cfg = _parse_config(raw, Path("/tmp"))
        assert cfg.name == "my-agent"
        assert cfg.provider == "azure"
        assert cfg.registry == "http://localhost:8080"

    def test_full_config(self):
        raw = {
            "name": "full-agent",
            "description": "A test agent",
            "registry": "http://example.com:9090",
            "provider": "openai",
            "model": "gpt-4",
            "prompt": "Be helpful",
            "sidecar": {
                "binary": "/usr/bin/sidecar",
                "api_key": "my-key",
                "socket": "/tmp/my.sock",
            },
            "skills": {
                "builtin": ["delegate", "list_agents"],
                "dirs": ["./custom-skills"],
                "disabled": ["mesh_broadcast"],
            },
            "data_dir": ".data",
            "max_turns": 20,
            "context_window": 200_000,
        }
        cfg = _parse_config(raw, Path("/project"))
        assert cfg.name == "full-agent"
        assert cfg.description == "A test agent"
        assert cfg.registry == "http://example.com:9090"
        assert cfg.provider == "openai"
        assert cfg.model == "gpt-4"
        assert cfg.sidecar.binary == "/usr/bin/sidecar"
        assert cfg.sidecar.api_key == "my-key"
        assert cfg.skills_builtin == ["delegate", "list_agents"]
        assert cfg.skills_dirs == ["./custom-skills"]
        assert cfg.skills_disabled == ["mesh_broadcast"]
        assert cfg.max_turns == 20

    def test_skills_as_list(self):
        raw = {"name": "test", "skills": ["a", "b", "c"]}
        cfg = _parse_config(raw, Path("/tmp"))
        assert cfg.skills_builtin == ["a", "b", "c"]
        assert cfg.skills_dirs == []

    def test_empty_config(self):
        cfg = _parse_config({}, Path("/tmp"))
        assert cfg.name == "unnamed-agent"

    def test_sidecar_non_dict(self):
        raw = {"name": "test", "sidecar": "invalid"}
        cfg = _parse_config(raw, Path("/tmp"))
        assert cfg.sidecar.binary == "chatixia-sidecar"  # defaults


class TestLoadConfig:
    def test_load_from_file(self, tmp_path):
        yaml_file = tmp_path / "agent.yaml"
        yaml_file.write_text("name: file-agent\nprovider: openai\n")
        cfg = load_config(yaml_file)
        assert cfg.name == "file-agent"
        assert cfg.provider == "openai"

    def test_load_from_directory(self, tmp_path):
        yaml_file = tmp_path / "agent.yaml"
        yaml_file.write_text("name: dir-agent\n")
        cfg = load_config(tmp_path)
        assert cfg.name == "dir-agent"

    def test_load_from_directory_yml(self, tmp_path):
        yaml_file = tmp_path / "agent.yml"
        yaml_file.write_text("name: yml-agent\n")
        cfg = load_config(tmp_path)
        assert cfg.name == "yml-agent"

    def test_load_missing_file(self):
        with pytest.raises(FileNotFoundError):
            load_config("/nonexistent/path/agent.yaml")

    def test_load_empty_directory(self, tmp_path):
        with pytest.raises(FileNotFoundError, match="No agent.yaml found"):
            load_config(tmp_path)

    def test_load_with_agent_md(self, tmp_path):
        yaml_file = tmp_path / "agent.yaml"
        yaml_file.write_text("name: md-agent\n")
        md_file = tmp_path / "AGENT.md"
        md_file.write_text("# My Agent\n\n## Goals\n- Be helpful\n")
        cfg = load_config(tmp_path)
        assert cfg.name == "md-agent"
        assert "My Agent" in cfg.agent_md
        assert "Be helpful" in cfg.agent_md

    def test_load_without_agent_md(self, tmp_path):
        yaml_file = tmp_path / "agent.yaml"
        yaml_file.write_text("name: no-md\n")
        cfg = load_config(tmp_path)
        assert cfg.agent_md == ""

    def test_source_dir_set_from_file(self, tmp_path):
        yaml_file = tmp_path / "agent.yaml"
        yaml_file.write_text("name: test\n")
        cfg = load_config(yaml_file)
        assert cfg._source_dir == tmp_path.resolve()

    def test_source_dir_set_from_directory(self, tmp_path):
        yaml_file = tmp_path / "agent.yaml"
        yaml_file.write_text("name: test\n")
        cfg = load_config(tmp_path)
        assert cfg._source_dir == tmp_path.resolve()


class TestSystemPrompt:
    def test_prompt_only(self):
        cfg = AgentConfig(name="test", prompt="You are helpful.")
        assert cfg.system_prompt == "You are helpful."

    def test_agent_md_only(self):
        cfg = AgentConfig(name="test", agent_md="# Agent\n## Goals\n- Help")
        assert cfg.system_prompt == "# Agent\n## Goals\n- Help"

    def test_combined_prompt_and_agent_md(self):
        cfg = AgentConfig(
            name="test",
            prompt="You are a helpful assistant.",
            agent_md="# Profile\n## Goals\n- Assist users",
        )
        prompt = cfg.system_prompt
        assert "You are a helpful assistant." in prompt
        assert "# Profile" in prompt
        # prompt comes first, agent_md second
        assert prompt.index("helpful assistant") < prompt.index("# Profile")

    def test_empty_prompt_and_agent_md(self):
        cfg = AgentConfig(name="test")
        assert cfg.system_prompt == ""

    def test_whitespace_handling(self):
        cfg = AgentConfig(name="test", prompt="  \n  Hello  \n  ", agent_md="  \n  World  \n  ")
        assert cfg.system_prompt == "Hello\n\nWorld"
