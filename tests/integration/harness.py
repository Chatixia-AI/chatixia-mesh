"""Process harness for the two-sidecar integration test.

Runs a real ``chatixia-registry`` and two real ``chatixia-sidecar`` processes
on localhost and plays a minimal agent on each sidecar's IPC socket (JSON
lines over a Unix socket, the same protocol ``agent/chatixia/core/mesh_client.py``
speaks). WebRTC runs natively with host ICE candidates, so no Docker, STUN
reachability or TURN is needed.

Only the standard library is used, so the test runs with a bare
``uvx pytest``.
"""

from __future__ import annotations

import asyncio
import contextlib
import json
import os
import re
import shutil
import signal
import socket
import subprocess
import tempfile
import time
import uuid
from collections.abc import Awaitable, Callable
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]

# The lower peer_id keeps its offer on glare (ADR-021), so A always wins.
PEER_A = "itest-a"
PEER_B = "itest-b"
API_KEYS = {PEER_A: "ak_itest_a", PEER_B: "ak_itest_b"}

# webrtc-ice 0.17 defaults: Disconnected after 5 s without traffic from the
# remote, Failed after a further 25 s. ICE consent therefore fails after 30 s.
ICE_CONSENT_FAILURE_S = 30.0

DEFAULT_RUST_LOG = "info"
LOG_TAIL_LINES = 120

_ANSI = re.compile(r"\x1b\[[0-9;]*m")

# Every process ever started, so a crashed test run cannot leak any.
_LIVE: list[Proc] = []


# ─── Binaries ────────────────────────────────────────────────────────────


def locate_binaries() -> tuple[Path, Path]:
    """Return (registry, sidecar) binary paths.

    ``CHATIXIA_BIN_DIR`` points at prebuilt binaries (CI builds them in a
    separate step). Otherwise ``cargo build`` (debug) runs first, which is a
    no-op when nothing changed, so the test never runs stale binaries.
    """
    bin_dir = os.environ.get("CHATIXIA_BIN_DIR")
    if bin_dir:
        directory = Path(bin_dir).resolve()
    else:
        subprocess.run(
            ["cargo", "build", "-p", "chatixia-registry", "-p", "chatixia-sidecar"],
            cwd=REPO_ROOT,
            check=True,
        )
        target = Path(os.environ.get("CARGO_TARGET_DIR", REPO_ROOT / "target"))
        if not target.is_absolute():
            target = REPO_ROOT / target
        directory = target / "debug"
    registry = directory / "chatixia-registry"
    sidecar = directory / "chatixia-sidecar"
    for path in (registry, sidecar):
        if not path.is_file():
            raise FileNotFoundError(f"binary not found: {path}")
    return registry, sidecar


def free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def child_env(extra: dict[str, str]) -> dict[str, str]:
    """A minimal environment: no proxy variables (reqwest would route the
    token exchange through them), no TURN settings, no secrets."""
    env = {
        k: os.environ[k] for k in ("PATH", "HOME", "LANG", "TMPDIR") if k in os.environ
    }
    env["NO_COLOR"] = "1"
    env["RUST_BACKTRACE"] = "1"
    env["RUST_LOG"] = os.environ.get("CHATIXIA_ITEST_RUST_LOG", DEFAULT_RUST_LOG)
    env.update(extra)
    return env


# ─── Processes ───────────────────────────────────────────────────────────


class Proc:
    """A child process whose combined output goes to a log file."""

    def __init__(
        self, name: str, argv: list[str], env: dict[str, str], cwd: Path, log_path: Path
    ):
        self.name = name
        self.argv = argv
        self.env = env
        self.cwd = cwd
        self.log_path = log_path
        self.popen: subprocess.Popen[bytes] | None = None
        self.paused = False
        self._log = None

    def start(self) -> None:
        self._log = open(self.log_path, "wb")  # closed in stop()
        self.popen = subprocess.Popen(
            self.argv,
            env=self.env,
            cwd=self.cwd,
            stdin=subprocess.DEVNULL,
            stdout=self._log,
            stderr=subprocess.STDOUT,
            start_new_session=True,
        )
        _LIVE.append(self)

    @property
    def pid(self) -> int:
        assert self.popen is not None
        return self.popen.pid

    def running(self) -> bool:
        return self.popen is not None and self.popen.poll() is None

    def pause(self) -> None:
        os.kill(self.pid, signal.SIGSTOP)
        self.paused = True

    def resume(self) -> None:
        os.kill(self.pid, signal.SIGCONT)
        self.paused = False

    def stop(self) -> None:
        if self.popen is None:
            return
        if self.popen.poll() is None:
            with contextlib.suppress(ProcessLookupError):
                if self.paused:
                    os.kill(self.pid, signal.SIGCONT)
                self.popen.terminate()
            try:
                self.popen.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.popen.kill()
                self.popen.wait(timeout=5)
        if self._log is not None:
            self._log.close()
            self._log = None
        if self in _LIVE:
            _LIVE.remove(self)

    def log_offset(self) -> int:
        try:
            return self.log_path.stat().st_size
        except FileNotFoundError:
            return 0

    def log_text(self, offset: int = 0) -> str:
        try:
            with open(self.log_path, "rb") as f:
                f.seek(offset)
                data = f.read()
        except FileNotFoundError:
            return ""
        return _ANSI.sub("", data.decode("utf-8", errors="replace"))

    async def wait_for_log(
        self, pattern: str, timeout: float, offset: int = 0
    ) -> re.Match[str]:
        """Wait until ``pattern`` (a regex) appears in the log after ``offset``."""
        regex = re.compile(pattern)
        deadline = time.monotonic() + timeout
        while True:
            match = regex.search(self.log_text(offset))
            if match:
                return match
            if not self.running():
                raise AssertionError(
                    f"{self.name} exited while waiting for log /{pattern}/"
                )
            if time.monotonic() > deadline:
                raise AssertionError(
                    f"{self.name}: no log line /{pattern}/ within {timeout:.0f}s"
                )
            await asyncio.sleep(0.1)


def kill_all_live() -> None:
    for proc in list(_LIVE):
        proc.stop()


# ─── Fake agent on the IPC socket ────────────────────────────────────────


Event = dict
Predicate = Callable[[Event], bool]


def is_connected(peer: str) -> Predicate:
    return lambda e: (
        e.get("type") == "peer_connected" and e["payload"].get("peer_id") == peer
    )


def is_disconnected(peer: str) -> Predicate:
    return lambda e: (
        e.get("type") == "peer_disconnected" and e["payload"].get("peer_id") == peer
    )


class FakeAgent:
    """Minimal agent: records every sidecar→agent event, sends commands."""

    def __init__(self, peer_id: str, socket_path: Path):
        self.peer_id = peer_id
        self.socket_path = socket_path
        self.events: list[Event] = []
        self._cond = asyncio.Condition()
        self._reader: asyncio.StreamReader | None = None
        self._writer: asyncio.StreamWriter | None = None
        self._task: asyncio.Task[None] | None = None

    async def connect(self, timeout: float, proc: Proc) -> None:
        deadline = time.monotonic() + timeout
        while True:
            try:
                self._reader, self._writer = await asyncio.open_unix_connection(
                    str(self.socket_path)
                )
                break
            except (FileNotFoundError, ConnectionRefusedError):
                if not proc.running():
                    raise AssertionError(
                        f"{proc.name} exited before opening its IPC socket"
                    )
                if time.monotonic() > deadline:
                    raise AssertionError(
                        f"{proc.name}: IPC socket not ready in {timeout:.0f}s"
                    )
                await asyncio.sleep(0.05)
        self._task = asyncio.create_task(self._read_loop())

    async def _read_loop(self) -> None:
        assert self._reader is not None
        while True:
            line = await self._reader.readline()
            if not line:
                return
            try:
                event = json.loads(line)
            except json.JSONDecodeError:
                continue
            event["_t"] = time.monotonic()
            async with self._cond:
                self.events.append(event)
                self._cond.notify_all()

    async def close(self) -> None:
        if self._task is not None:
            self._task.cancel()
            with contextlib.suppress(asyncio.CancelledError, Exception):
                await self._task
        if self._writer is not None:
            self._writer.close()
            with contextlib.suppress(Exception):
                await self._writer.wait_closed()

    def mark(self) -> int:
        """Index of the next event; pass as ``start`` to only see later ones."""
        return len(self.events)

    def _find(self, predicate: Predicate, start: int) -> Event | None:
        for event in self.events[start:]:
            if predicate(event):
                return event
        return None

    async def wait_for(
        self, predicate: Predicate, timeout: float, what: str, start: int = 0
    ) -> Event:
        async with self._cond:
            try:
                await asyncio.wait_for(
                    self._cond.wait_for(
                        lambda: self._find(predicate, start) is not None
                    ),
                    timeout,
                )
            except asyncio.TimeoutError:
                raise AssertionError(
                    f"agent {self.peer_id}: no {what} within {timeout:.0f}s; "
                    f"events since #{start}: {self.describe(start)}"
                ) from None
            found = self._find(predicate, start)
            assert found is not None
            return found

    def describe(self, start: int = 0) -> str:
        return json.dumps(
            [
                {
                    "type": e.get("type"),
                    **{k: v for k, v in e.get("payload", {}).items() if k != "message"},
                }
                for e in self.events[start:]
            ]
        )

    def last_link_event(self, peer: str) -> str | None:
        """Type of the most recent peer_connected/peer_disconnected for ``peer``."""
        for event in reversed(self.events):
            if event.get("type") in ("peer_connected", "peer_disconnected") and (
                event["payload"].get("peer_id") == peer
            ):
                return event["type"]
        return None

    async def command(self, msg_type: str, payload: dict) -> None:
        assert self._writer is not None
        self._writer.write(
            (json.dumps({"type": msg_type, "payload": payload}) + "\n").encode()
        )
        await self._writer.drain()

    async def send_text(self, target: str, text: str) -> str:
        """Send an ``agent_prompt`` MeshMessage over the DataChannel; returns its request_id."""
        request_id = uuid.uuid4().hex
        await self.command(
            "send",
            {
                "target_peer": target,
                "message": {
                    "type": "agent_prompt",
                    "request_id": request_id,
                    "source_agent": self.peer_id,
                    "target_agent": target,
                    "payload": {"text": text},
                },
            },
        )
        return request_id

    async def connected_peers(self, timeout: float = 5.0) -> list[str]:
        start = self.mark()
        await self.command("list_peers", {})
        event = await self.wait_for(
            lambda e: e.get("type") == "peer_list", timeout, "peer_list reply", start
        )
        return list(event["payload"].get("peers", []))


# ─── Signaling gate (TCP proxy in front of the registry) ─────────────────


class WsGate:
    """TCP proxy in front of the registry that makes two sidecars register at
    the same instant, the offer-glare setup from ADR-021.

    The first ``hold`` ``GET /ws`` upgrades are held until all of them have
    arrived, then forwarded together. Their ``101`` replies are held until
    every upgrade has completed at the registry (plus ``settle`` seconds, so
    each peer is in the registry's signaling map), then released together.
    Each sidecar's ``register`` therefore gets a ``peer_list`` naming the
    other, and both send an offer. Everything else is passed through.
    """

    def __init__(self, upstream_port: int, hold: int, settle: float = 0.3):
        self.upstream_port = upstream_port
        self.hold = hold
        self.settle = settle
        self.port = 0
        self._arrived = 0
        self._upgraded = 0
        self._all_arrived = asyncio.Event()
        self._all_upgraded = asyncio.Event()
        self._server: asyncio.AbstractServer | None = None
        self._writers: set[asyncio.StreamWriter] = set()

    @property
    def held(self) -> int:
        return self._arrived

    async def start(self) -> None:
        self._server = await asyncio.start_server(self._handle, "127.0.0.1", 0)
        self.port = self._server.sockets[0].getsockname()[1]

    async def close(self) -> None:
        if self._server is not None:
            self._server.close()
        for w in list(self._writers):
            w.close()
        if self._server is not None:
            with contextlib.suppress(Exception):
                await asyncio.wait_for(self._server.wait_closed(), 5)

    async def _handle(self, cr: asyncio.StreamReader, cw: asyncio.StreamWriter) -> None:
        uw: asyncio.StreamWriter | None = None
        self._writers.add(cw)
        try:
            head = await cr.readuntil(b"\r\n\r\n")
            held = head.startswith(b"GET /ws") and self._arrived < self.hold
            if held:
                self._arrived += 1
                if self._arrived == self.hold:
                    self._all_arrived.set()
                await self._all_arrived.wait()
            ur, uw = await asyncio.open_connection("127.0.0.1", self.upstream_port)
            self._writers.add(uw)
            uw.write(head)
            await uw.drain()
            if held:
                reply = await ur.readuntil(b"\r\n\r\n")
                self._upgraded += 1
                if self._upgraded == self.hold:
                    self._all_upgraded.set()
                await self._all_upgraded.wait()
                await asyncio.sleep(self.settle)
                cw.write(reply)
                await cw.drain()
            await asyncio.gather(_pipe(cr, uw), _pipe(ur, cw))
        except (
            ConnectionError,
            asyncio.IncompleteReadError,
            asyncio.LimitOverrunError,
        ):
            pass
        finally:
            for w in (cw, uw):
                if w is not None:
                    self._writers.discard(w)
                    w.close()


async def _pipe(src: asyncio.StreamReader, dst: asyncio.StreamWriter) -> None:
    try:
        while data := await src.read(65536):
            dst.write(data)
            await dst.drain()
    except ConnectionError:
        pass
    finally:
        dst.close()


# ─── The cluster: registry + sidecars + fake agents ──────────────────────


class MeshCluster:
    """Registry plus sidecars, torn down (processes and temp files) on exit.

    On an exception the tail of every process log and every agent's event
    list is printed; pytest shows it with the failure.
    """

    def __init__(self, binaries: tuple[Path, Path], name: str, gate_ws: int = 0):
        self.registry_bin, self.sidecar_bin = binaries
        self.name = name
        self.gate_ws = gate_ws
        # Short path: Unix socket paths are limited to ~104-108 bytes.
        self.workdir = Path(tempfile.mkdtemp(prefix="cxm-itest-"))
        self.port = free_port()
        self.registry: Proc | None = None
        self.gate: WsGate | None = None
        self.sidecars: dict[str, Proc] = {}
        self.agents: dict[str, FakeAgent] = {}
        self.t0 = time.monotonic()

    async def __aenter__(self) -> MeshCluster:
        return self

    async def __aexit__(self, exc_type, exc, tb) -> None:
        try:
            if exc_type is not None:
                self.dump(exc)
        finally:
            await self.close()

    def log(self, msg: str) -> None:
        print(f"[itest {time.monotonic() - self.t0:6.1f}s] {msg}", flush=True)

    async def start_registry(self) -> None:
        keys = {
            "keys": {
                key: {"peer_id": peer, "role": "agent"}
                for peer, key in API_KEYS.items()
            }
        }
        keys_file = self.workdir / "api_keys.json"
        keys_file.write_text(json.dumps(keys))
        env = child_env(
            {
                "PORT": str(self.port),
                "API_KEYS_FILE": str(keys_file),
                "SIGNALING_SECRET": "itest-signaling-secret",
                # Pairing admin endpoints are being put behind this token;
                # the test never calls them but keeps the registry happy.
                "REGISTRY_ADMIN_TOKEN": "itest-admin-token",
                "HUB_DIST_DIR": str(self.workdir / "no-hub"),
            }
        )
        self.registry = Proc(
            "registry",
            [str(self.registry_bin)],
            env,
            self.workdir,
            self.workdir / "registry.log",
        )
        self.registry.start()
        deadline = time.monotonic() + 20
        while True:
            try:
                _, w = await asyncio.open_connection("127.0.0.1", self.port)
                w.close()
                break
            except OSError:
                if not self.registry.running():
                    raise AssertionError("registry exited during startup") from None
                if time.monotonic() > deadline:
                    raise AssertionError("registry did not listen within 20s") from None
                await asyncio.sleep(0.05)
        if self.gate_ws:
            self.gate = WsGate(self.port, hold=self.gate_ws)
            await self.gate.start()
        self.log(
            f"registry up on :{self.port}"
            + (f" behind ws gate :{self.gate.port}" if self.gate else "")
        )

    async def start_sidecar(self, peer: str) -> FakeAgent:
        port = self.gate.port if self.gate else self.port
        sock = self.workdir / f"{peer}.sock"
        env = child_env(
            {
                "SIGNALING_URL": f"ws://127.0.0.1:{port}/ws",
                "TOKEN_URL": f"http://127.0.0.1:{port}/api/token",
                "API_KEY": API_KEYS[peer],
                "IPC_SOCKET": str(sock),
            }
        )
        proc = Proc(
            peer,
            [str(self.sidecar_bin)],
            env,
            self.workdir,
            self.workdir / f"{peer}.log",
        )
        proc.start()
        self.sidecars[peer] = proc
        agent = FakeAgent(peer, sock)
        await agent.connect(30, proc)
        self.agents[peer] = agent
        self.log(f"sidecar {peer} up (pid {proc.pid}), fake agent attached")
        return agent

    def sidecar(self, peer: str) -> Proc:
        return self.sidecars[peer]

    def dump(self, exc: BaseException | None) -> None:
        print(f"\n===== {self.name} failed: {exc!r} =====")
        procs = ([self.registry] if self.registry else []) + list(
            self.sidecars.values()
        )
        for proc in procs:
            lines = proc.log_text().splitlines()
            print(
                f"\n----- {proc.name} log (last {LOG_TAIL_LINES} of {len(lines)} lines) -----"
            )
            print("\n".join(lines[-LOG_TAIL_LINES:]))
        for agent in self.agents.values():
            print(f"\n----- agent {agent.peer_id} events -----\n{agent.describe()}")

    async def close(self) -> None:
        for agent in self.agents.values():
            await agent.close()
        for proc in self.sidecars.values():
            proc.stop()
        if self.gate is not None:
            await self.gate.close()
        if self.registry is not None:
            self.registry.stop()
        keep = os.environ.get("CHATIXIA_ITEST_LOG_DIR")
        if keep:
            dest = Path(keep) / self.name
            dest.mkdir(parents=True, exist_ok=True)
            for log in self.workdir.glob("*.log"):
                shutil.copy(log, dest / log.name)
        shutil.rmtree(self.workdir, ignore_errors=True)


def run_scenario(
    binaries: tuple[Path, Path],
    name: str,
    scenario: Callable[[MeshCluster], Awaitable[None]],
    *,
    timeout: float,
    gate_ws: int = 0,
) -> None:
    """Run one async scenario against a fresh cluster with an overall timeout."""

    async def main() -> None:
        async with MeshCluster(binaries, name, gate_ws=gate_ws) as cluster:
            try:
                await asyncio.wait_for(scenario(cluster), timeout)
            except asyncio.TimeoutError:
                raise AssertionError(
                    f"scenario exceeded its {timeout:.0f}s budget"
                ) from None

    asyncio.run(main())
