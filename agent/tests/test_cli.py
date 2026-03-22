import pytest
from chatixia.cli import main


class TestCliMain:
    def test_no_args_shows_help(self, capsys):
        result = main([])
        assert result == 0

    def test_version_flag(self, capsys):
        with pytest.raises(SystemExit) as exc_info:
            main(["--version"])
        assert exc_info.value.code == 0

    def test_init_creates_scaffold(self, tmp_path, monkeypatch):
        monkeypatch.chdir(tmp_path)
        result = main(["init", "test-agent", "-d", str(tmp_path)])
        assert result == 0
        assert (tmp_path / "agent.yaml").exists()

    def test_init_default_name(self, tmp_path, monkeypatch):
        monkeypatch.chdir(tmp_path)
        result = main(["init", "-d", str(tmp_path)])
        assert result == 0
        content = (tmp_path / "agent.yaml").read_text()
        assert "name: my-agent" in content

    def test_init_fails_if_exists(self, tmp_path, monkeypatch):
        monkeypatch.chdir(tmp_path)
        (tmp_path / "agent.yaml").write_text("existing")
        result = main(["init", "test", "-d", str(tmp_path)])
        assert result == 1

    def test_validate_missing_manifest(self, tmp_path, monkeypatch):
        monkeypatch.chdir(tmp_path)
        result = main(["validate", str(tmp_path / "nonexistent.yaml")])
        assert result == 1

    def test_validate_valid_manifest(self, tmp_path):
        manifest = tmp_path / "agent.yaml"
        manifest.write_text("name: valid-agent\nprovider: azure\nregistry: http://localhost:8080\n")
        result = main(["validate", str(manifest)])
        assert result == 0

    def test_validate_invalid_manifest(self, tmp_path):
        manifest = tmp_path / "agent.yaml"
        manifest.write_text("name: ''\nprovider: invalid\n")
        result = main(["validate", str(manifest)])
        assert result == 1

    def test_pair_invalid_code(self, tmp_path):
        manifest = tmp_path / "agent.yaml"
        manifest.write_text("name: test\n")
        result = main(["pair", "abc", str(manifest)])
        assert result == 1

    def test_pair_short_code(self, tmp_path):
        manifest = tmp_path / "agent.yaml"
        manifest.write_text("name: test\n")
        result = main(["pair", "123", str(manifest)])
        assert result == 1
