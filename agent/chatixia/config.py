"""Agent configuration loader — parses ``agent.yaml`` manifests."""

from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

import yaml


@dataclass
class SidecarConfig:
    """Sidecar (Rust WebRTC peer) configuration."""

    binary: str = "chatixia-sidecar"
    api_key: str = "ak_dev_001"
    socket: str = "/tmp/chatixia-sidecar.sock"


@dataclass
class AgentConfig:
    """Complete agent configuration parsed from ``agent.yaml``."""

    name: str
    description: str = ""

    # Mesh
    registry: str = "http://localhost:8080"
    sidecar: SidecarConfig = field(default_factory=SidecarConfig)

    # LLM
    provider: str = "azure"
    model: str = ""

    # Prompt
    prompt: str = ""

    # Skills
    skills_builtin: list[str] = field(default_factory=list)
    skills_dirs: list[str] = field(default_factory=list)
    skills_disabled: list[str] = field(default_factory=list)

    # Runtime
    data_dir: str = ".chatixia"
    max_turns: int = 10
    context_window: int = 120_000

    # Internal: directory containing agent.yaml
    _source_dir: Path = field(default_factory=Path.cwd)

    def resolve_path(self, path: str) -> Path:
        """Resolve a path relative to the manifest directory."""
        p = Path(path).expanduser()
        if p.is_absolute():
            return p
        return (self._source_dir / p).resolve()

    def validate(self) -> list[str]:
        """Return a list of validation error messages (empty = valid)."""
        errors: list[str] = []
        if not self.name:
            errors.append("'name' is required")
        if self.provider not in ("azure", "openai", "ollama"):
            errors.append(
                f"Unknown provider: {self.provider!r} (expected azure|openai|ollama)"
            )
        if not self.registry:
            errors.append("'registry' URL is required")
        return errors


def load_config(path: str | Path) -> AgentConfig:
    """Load an ``AgentConfig`` from a YAML file or directory.

    If *path* is a directory, looks for ``agent.yaml`` (or ``.yml``) inside it.
    """
    path = Path(path)
    if path.is_dir():
        for candidate in ("agent.yaml", "agent.yml"):
            yaml_path = path / candidate
            if yaml_path.exists():
                break
        else:
            raise FileNotFoundError(f"No agent.yaml found in {path}")
        source_dir = path.resolve()
        path = yaml_path
    else:
        if not path.exists():
            raise FileNotFoundError(f"Manifest not found: {path}")
        source_dir = path.resolve().parent

    with open(path, encoding="utf-8") as fh:
        raw: dict[str, Any] = yaml.safe_load(fh) or {}

    return _parse_config(raw, source_dir)


def _parse_config(raw: dict[str, Any], source_dir: Path) -> AgentConfig:
    """Parse raw YAML dict into an ``AgentConfig``."""

    # -- Sidecar ---
    sidecar_raw = raw.get("sidecar", {})
    if isinstance(sidecar_raw, dict):
        sidecar = SidecarConfig(
            binary=sidecar_raw.get("binary", "chatixia-sidecar"),
            api_key=sidecar_raw.get("api_key", "ak_dev_001"),
            socket=sidecar_raw.get("socket", "/tmp/chatixia-sidecar.sock"),
        )
    else:
        sidecar = SidecarConfig()

    # -- Skills ---
    skills_raw = raw.get("skills", {})
    if isinstance(skills_raw, list):
        skills_builtin = skills_raw
        skills_dirs: list[str] = []
        skills_disabled: list[str] = []
    elif isinstance(skills_raw, dict):
        skills_builtin = skills_raw.get("builtin", [])
        skills_dirs = skills_raw.get("dirs", [])
        skills_disabled = skills_raw.get("disabled", [])
    else:
        skills_builtin = []
        skills_dirs = []
        skills_disabled = []

    return AgentConfig(
        name=raw.get("name", "unnamed-agent"),
        description=raw.get("description", ""),
        registry=raw.get("registry", "http://localhost:8080"),
        sidecar=sidecar,
        provider=raw.get("provider", "azure"),
        model=raw.get("model", ""),
        prompt=raw.get("prompt", ""),
        skills_builtin=skills_builtin,
        skills_dirs=skills_dirs,
        skills_disabled=skills_disabled,
        data_dir=raw.get("data_dir", ".chatixia"),
        max_turns=int(raw.get("max_turns", 10)),
        context_window=int(raw.get("context_window", 120_000)),
        _source_dir=source_dir,
    )
