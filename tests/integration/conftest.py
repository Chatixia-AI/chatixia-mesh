"""Fixtures for the two-sidecar integration test (see harness.py)."""

from __future__ import annotations

import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent))

from harness import kill_all_live, locate_binaries  # noqa: E402


@pytest.fixture(scope="session")
def binaries() -> tuple[Path, Path]:
    return locate_binaries()


@pytest.fixture(autouse=True)
def _no_leaked_processes():
    yield
    kill_all_live()
