import pytest
from pathlib import Path
from chatixia.scaffold import write_scaffold


class TestWriteScaffold:
    def test_creates_agent_yaml(self, tmp_path):
        result = write_scaffold("my-agent", str(tmp_path))
        assert result == tmp_path / "agent.yaml"
        assert result.exists()
        content = result.read_text()
        assert "name: my-agent" in content

    def test_creates_env_example(self, tmp_path):
        write_scaffold("test-agent", str(tmp_path))
        env_file = tmp_path / ".env.example"
        assert env_file.exists()
        content = env_file.read_text()
        assert "REGISTRY_URL" in content

    def test_creates_gitignore(self, tmp_path):
        write_scaffold("test-agent", str(tmp_path))
        gitignore = tmp_path / ".gitignore"
        assert gitignore.exists()
        content = gitignore.read_text()
        assert ".env" in content

    def test_fails_if_manifest_exists(self, tmp_path):
        (tmp_path / "agent.yaml").write_text("existing content")
        with pytest.raises(FileExistsError):
            write_scaffold("test-agent", str(tmp_path))

    def test_creates_nested_directory(self, tmp_path):
        nested = tmp_path / "deep" / "nested"
        write_scaffold("nested-agent", str(nested))
        assert (nested / "agent.yaml").exists()

    def test_does_not_overwrite_env_example(self, tmp_path):
        existing_content = "MY_CUSTOM_VAR=hello"
        (tmp_path / ".env.example").write_text(existing_content)
        write_scaffold("test-agent", str(tmp_path))
        assert (tmp_path / ".env.example").read_text() == existing_content

    def test_manifest_contains_sidecar_config(self, tmp_path):
        write_scaffold("my-agent", str(tmp_path))
        content = (tmp_path / "agent.yaml").read_text()
        assert "sidecar:" in content
        assert "socket:" in content

    def test_name_substitution(self, tmp_path):
        write_scaffold("custom-name", str(tmp_path))
        content = (tmp_path / "agent.yaml").read_text()
        assert "name: custom-name" in content
        assert "chatixia-custom-name" in content  # socket name
