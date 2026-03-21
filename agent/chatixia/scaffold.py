"""Agent manifest scaffolding for ``chatixia init``."""

from __future__ import annotations

from pathlib import Path

_MANIFEST_TEMPLATE = """\
# Chatixia Mesh Agent
# https://github.com/Chatixia-AI/chatixia-mesh

name: {name}
description: ""

# Registry — the mesh signaling + coordination server
registry: "http://localhost:8080"

# LLM provider: azure | openai | ollama
provider: azure
# model: gpt-4o   # Override default model

# System prompt — defines the agent's persona and behaviour
prompt: |
  You are a helpful AI assistant connected to the Chatixia mesh.
  Use the available tools to help the user.
  You can collaborate with other agents using delegate and mesh_send.

# Sidecar — the Rust WebRTC peer that bridges you to the mesh
sidecar:
  binary: ./target/release/chatixia-sidecar
  api_key: ak_dev_001
  socket: /tmp/chatixia-{name}.sock

# Skills configuration
skills:
  builtin:
    - delegate
    - list_agents
    - mesh_send
    - mesh_broadcast
  # dirs:                  # Additional skill directories
  #   - ./skills
  # disabled:              # Skills to exclude
  #   - mesh_broadcast

# Autonomous goals (activated with future --daemon support)
# goals:
#   - name: example_monitor
#     sensor: "Check for new items"
#     action: "Summarize and report"
#     interval: 300

# Runtime settings
data_dir: .chatixia
# max_turns: 10
# context_window: 120000
"""

_ENV_EXAMPLE = """\
# ──────────────────────────────────────────────────────────
# Chatixia Agent — Environment Variables
# ──────────────────────────────────────────────────────────

# Registry (mesh signaling server)
REGISTRY_URL=http://localhost:8080
API_KEY=ak_dev_001

# Azure OpenAI (provider: azure)
AZURE_OPENAI_ENDPOINT=https://your-resource.openai.azure.com/
AZURE_OPENAI_API_KEY=
AZURE_OPENAI_DEPLOYMENT=gpt-4o

# OpenAI (provider: openai)
# OPENAI_API_KEY=

# Ollama (provider: ollama — no credentials needed)
# OLLAMA_BASE_URL=http://localhost:11434/v1
# OLLAMA_MODEL=qwen3.5:4b

# Sidecar
# SIDECAR_BINARY=./target/release/chatixia-sidecar
# IPC_SOCKET=/tmp/chatixia-sidecar.sock

# Logging
# LOG_LEVEL=WARNING
"""

_GITIGNORE = """\
.env
.chatixia/
__pycache__/
*.pyc
"""


def write_scaffold(name: str, directory: str = ".") -> Path:
    """Write an agent manifest scaffold to *directory*.

    Returns the path to the created ``agent.yaml``.
    """
    dir_path = Path(directory)
    dir_path.mkdir(parents=True, exist_ok=True)

    manifest_path = dir_path / "agent.yaml"
    if manifest_path.exists():
        raise FileExistsError(f"{manifest_path} already exists")

    manifest_path.write_text(
        _MANIFEST_TEMPLATE.format(name=name),
        encoding="utf-8",
    )

    env_example = dir_path / ".env.example"
    if not env_example.exists():
        env_example.write_text(_ENV_EXAMPLE, encoding="utf-8")

    gitignore = dir_path / ".gitignore"
    if not gitignore.exists():
        gitignore.write_text(_GITIGNORE, encoding="utf-8")

    print(f"Created {manifest_path}")
    print()
    print("Next steps:")
    print(f"  1. cp .env.example .env        # Fill in your credentials")
    print(f"  2. chatixia validate            # Check everything is correct")
    print(f"  3. chatixia pair <code>         # Pair with a mesh (get code from admin)")
    print(f"  4. chatixia run                 # Connect to the mesh")

    return manifest_path
