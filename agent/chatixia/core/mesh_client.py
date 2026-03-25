"""Mesh client — IPC bridge to Rust sidecar for WebRTC mesh communication.

The Python agent communicates with its Rust sidecar via a Unix domain socket.
The sidecar handles all WebRTC/signaling complexity; this module provides a
clean async interface for sending/receiving mesh messages.

Protocol: JSON lines over Unix socket (one JSON object per newline).
"""

from __future__ import annotations

import asyncio
import json
import logging
import os
import shutil
import subprocess
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Callable

logger = logging.getLogger("chatixia.mesh")


@dataclass
class MeshMessage:
    """A message received from another agent over the mesh."""

    msg_type: str
    request_id: str = ""
    source_agent: str = ""
    target_agent: str = ""
    payload: dict[str, Any] = field(default_factory=dict)

    def to_dict(self) -> dict[str, Any]:
        return {
            "type": self.msg_type,
            "request_id": self.request_id,
            "source_agent": self.source_agent,
            "target_agent": self.target_agent,
            "payload": self.payload,
        }

    @classmethod
    def from_dict(cls, d: dict[str, Any]) -> MeshMessage:
        return cls(
            msg_type=d.get("type", ""),
            request_id=d.get("request_id", ""),
            source_agent=d.get("source_agent", ""),
            target_agent=d.get("target_agent", ""),
            payload=d.get("payload", {}),
        )


def _resolve_sidecar_binary(configured: str) -> str:
    """Resolve the sidecar binary path.

    Resolution order:
    1. Configured path (if absolute and exists)
    2. SIDECAR_BINARY env var
    3. PATH lookup
    Raises RuntimeError with install instructions if not found.
    """
    # 1. Explicit absolute or relative path that exists on disk
    p = Path(configured).expanduser()
    if p.exists() and os.access(p, os.X_OK):
        return str(p.resolve())

    # 2. Env var override
    env_binary = os.environ.get("SIDECAR_BINARY")
    if env_binary:
        ep = Path(env_binary).expanduser()
        if ep.exists() and os.access(ep, os.X_OK):
            return str(ep.resolve())

    # 3. Search PATH (handles bare name like "chatixia-sidecar")
    found = shutil.which(configured)
    if found:
        return found
    if env_binary:
        found = shutil.which(env_binary)
        if found:
            return found

    raise RuntimeError(
        f"Sidecar binary '{configured}' not found.\n"
        "Install it with one of:\n"
        "  cargo install --git https://github.com/Chatixia-AI/chatixia-mesh chatixia-sidecar\n"
        "Or build from source:\n"
        "  git clone https://github.com/Chatixia-AI/chatixia-mesh && cd chatixia-mesh\n"
        "  cargo build --release -p chatixia-sidecar\n"
        "Then either add target/release/ to PATH or set sidecar.binary in agent.yaml"
    )


class MeshClient:
    """Async client for communicating with the Rust sidecar over IPC."""

    def __init__(
        self,
        socket_path: str = "/tmp/chatixia-sidecar.sock",
        sidecar_binary: str | None = None,
    ) -> None:
        self._socket_path = socket_path
        self._sidecar_binary = sidecar_binary or os.environ.get(
            "SIDECAR_BINARY", "chatixia-sidecar"
        )
        self._reader: asyncio.StreamReader | None = None
        self._writer: asyncio.StreamWriter | None = None
        self._sidecar_proc: subprocess.Popen | None = None
        self._sidecar_log_file: Any | None = None
        self._sidecar_log_path: str = ""
        self._handlers: dict[str, list[Callable]] = {}
        self._listen_task: asyncio.Task | None = None
        self._connected = False
        self._pending_responses: dict[str, asyncio.Future] = {}
        self._peers: set[str] = set()

    async def start(self, auto_spawn_sidecar: bool = True) -> None:
        """Start the mesh client — optionally spawn the sidecar process."""
        # Remove stale socket from previous crash
        Path(self._socket_path).unlink(missing_ok=True)

        if auto_spawn_sidecar:
            await self._spawn_sidecar()

        # Wait for socket to appear
        for _ in range(50):  # 5 seconds
            if Path(self._socket_path).exists():
                break
            # Check if sidecar exited early
            if self._sidecar_proc and self._sidecar_proc.poll() is not None:
                raise RuntimeError(
                    f"Sidecar exited with code {self._sidecar_proc.returncode}"
                    f" — check {self._sidecar_log_path}"
                )
            await asyncio.sleep(0.1)

        if not Path(self._socket_path).exists():
            raise RuntimeError(
                f"Sidecar did not create socket at {self._socket_path} within 5s"
                f" — check {self._sidecar_log_path}"
            )

        # Connect to sidecar IPC socket
        self._reader, self._writer = await asyncio.open_unix_connection(
            self._socket_path
        )
        self._connected = True
        logger.info("mesh client connected to sidecar at %s", self._socket_path)

        # Start listening for incoming messages
        self._listen_task = asyncio.create_task(self._listen_loop())

        # Register internal handlers for peer lifecycle events
        self.on("peer_connected", self._on_peer_connected)
        self.on("peer_disconnected", self._on_peer_disconnected)
        self.on("peer_list", self._on_peer_list)

    async def _spawn_sidecar(self) -> None:
        """Spawn the Rust sidecar process, capturing logs to a file."""
        binary = _resolve_sidecar_binary(self._sidecar_binary)
        env = os.environ.copy()
        env["IPC_SOCKET"] = self._socket_path

        # Derive log file path from socket path:
        #   /tmp/chatixia-sidecar.sock → /tmp/chatixia-sidecar.log
        self._sidecar_log_path = str(Path(self._socket_path).with_suffix(".log"))
        self._sidecar_log_file = open(self._sidecar_log_path, "a")  # noqa: SIM115

        self._sidecar_proc = subprocess.Popen(
            [binary],
            env=env,
            stdout=self._sidecar_log_file,
            stderr=self._sidecar_log_file,
        )
        logger.info(
            "sidecar spawned (pid=%d, binary=%s, socket=%s, log=%s)",
            self._sidecar_proc.pid,
            binary,
            self._socket_path,
            self._sidecar_log_path,
        )

    async def stop(self) -> None:
        """Stop the mesh client and sidecar."""
        self._connected = False
        if self._listen_task:
            self._listen_task.cancel()
        if self._writer:
            self._writer.close()
        if self._sidecar_proc:
            self._sidecar_proc.terminate()
            self._sidecar_proc.wait(timeout=5)
        if self._sidecar_log_file:
            self._sidecar_log_file.close()

    async def _listen_loop(self) -> None:
        """Read messages from sidecar and dispatch to handlers."""
        assert self._reader is not None
        try:
            while self._connected:
                line = await self._reader.readline()
                if not line:
                    logger.warning("sidecar IPC connection closed")
                    break
                try:
                    data = json.loads(line.decode().strip())
                    await self._dispatch(data)
                except json.JSONDecodeError as e:
                    logger.warning("failed to parse IPC message: %s", e)
        except asyncio.CancelledError:
            pass
        except Exception as e:
            logger.error("IPC listen error: %s", e)

    async def _dispatch(self, data: dict[str, Any]) -> None:
        """Dispatch an incoming IPC message to registered handlers."""
        msg_type = data.get("type", "")

        # Check for pending request/response
        if msg_type == "message":
            payload = data.get("payload", {})
            inner = payload.get("message", {})
            req_id = inner.get("request_id", "")
            if req_id and req_id in self._pending_responses:
                self._pending_responses[req_id].set_result(inner)
                return

        # Dispatch to registered handlers
        handlers = self._handlers.get(msg_type, []) + self._handlers.get("*", [])
        for handler in handlers:
            try:
                result = handler(data)
                if asyncio.iscoroutine(result):
                    await result
            except Exception as e:
                logger.error("handler error for %s: %s", msg_type, e)

    def on(self, msg_type: str, handler: Callable) -> None:
        """Register a handler for a message type. Use '*' for all messages."""
        self._handlers.setdefault(msg_type, []).append(handler)

    # ─── Peer tracking ────────────────────────────────────────────────────

    def _on_peer_connected(self, data: dict[str, Any]) -> None:
        peer_id = data.get("payload", {}).get("peer_id", "")
        if peer_id:
            self._peers.add(peer_id)
            logger.info("peer connected: %s (total: %d)", peer_id, len(self._peers))

    def _on_peer_disconnected(self, data: dict[str, Any]) -> None:
        peer_id = data.get("payload", {}).get("peer_id", "")
        if peer_id:
            self._peers.discard(peer_id)
            logger.info("peer disconnected: %s (total: %d)", peer_id, len(self._peers))

    def _on_peer_list(self, data: dict[str, Any]) -> None:
        peers = data.get("payload", {}).get("peers", [])
        self._peers = set(peers)
        logger.info("peer list updated: %d peers", len(self._peers))

    def is_peer_connected(self, peer_id: str) -> bool:
        """Check if a specific peer is currently connected."""
        return peer_id in self._peers

    @property
    def peers(self) -> set[str]:
        """Currently connected peer IDs (returns a copy)."""
        return set(self._peers)

    # ─── IPC messaging ───────────────────────────────────────────────────

    async def _send_ipc(self, msg: dict[str, Any]) -> None:
        """Send a JSON-line message to the sidecar."""
        assert self._writer is not None
        line = json.dumps(msg) + "\n"
        self._writer.write(line.encode())
        await self._writer.drain()

    async def send(self, target_peer: str, message: MeshMessage) -> None:
        """Send a message to a specific peer via the mesh."""
        await self._send_ipc(
            {
                "type": "send",
                "payload": {
                    "target_peer": target_peer,
                    "message": message.to_dict(),
                },
            }
        )

    async def broadcast(self, message: MeshMessage) -> None:
        """Broadcast a message to all connected mesh peers."""
        await self._send_ipc(
            {
                "type": "broadcast",
                "payload": {"message": message.to_dict()},
            }
        )

    async def request(
        self,
        target_peer: str,
        message: MeshMessage,
        timeout: float = 30.0,
    ) -> dict[str, Any]:
        """Send a request and wait for a response (matched by request_id)."""
        import uuid

        if not message.request_id:
            message.request_id = uuid.uuid4().hex[:12]

        loop = asyncio.get_event_loop()
        future = loop.create_future()
        self._pending_responses[message.request_id] = future

        await self.send(target_peer, message)

        try:
            return await asyncio.wait_for(future, timeout=timeout)
        finally:
            self._pending_responses.pop(message.request_id, None)

    async def list_peers(self) -> list[str]:
        """Get the list of connected mesh peers."""
        await self._send_ipc({"type": "list_peers", "payload": {}})
        # Wait for peer_list response (arrives via _on_peer_list handler)
        await asyncio.sleep(0.2)
        return list(self._peers)

    @property
    def connected(self) -> bool:
        return self._connected

    @property
    def sidecar_log_path(self) -> str:
        """Path to the sidecar log file (populated after start)."""
        return self._sidecar_log_path
