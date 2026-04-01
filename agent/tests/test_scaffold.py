import pytest
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

    def test_sidecar_binary_is_bare_name(self, tmp_path):
        """Scaffold should use bare 'chatixia-sidecar' (PATH lookup), not a repo-relative path."""
        write_scaffold("test-agent", str(tmp_path))
        content = (tmp_path / "agent.yaml").read_text()
        assert "binary: chatixia-sidecar" in content
        assert "./target/release/" not in content

    def test_scaffold_contains_install_comment(self, tmp_path):
        write_scaffold("test-agent", str(tmp_path))
        content = (tmp_path / "agent.yaml").read_text()
        assert "cargo install" in content

    def test_creates_agent_md(self, tmp_path):
        write_scaffold("my-agent", str(tmp_path))
        md_file = tmp_path / "AGENT.md"
        assert md_file.exists()
        content = md_file.read_text()
        assert "my-agent" in content
        assert "Personality" in content
        assert "Goals" in content
        assert "Constraints" in content

    def test_agent_md_name_substitution(self, tmp_path):
        write_scaffold("custom-bot", str(tmp_path))
        content = (tmp_path / "AGENT.md").read_text()
        assert "custom-bot" in content

    def test_does_not_overwrite_agent_md(self, tmp_path):
        existing = "# My Custom Profile"
        (tmp_path / "AGENT.md").write_text(existing)
        write_scaffold("test-agent", str(tmp_path))
        assert (tmp_path / "AGENT.md").read_text() == existing

    def test_scaffold_creates_all_files(self, tmp_path):
        write_scaffold("full-agent", str(tmp_path))
        assert (tmp_path / "agent.yaml").exists()
        assert (tmp_path / "AGENT.md").exists()
        assert (tmp_path / ".env.example").exists()
        assert (tmp_path / ".gitignore").exists()
