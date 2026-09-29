"""Two real sidecars and a real registry on localhost.

Scenarios:

1. connect and exchange messages both ways over the DataChannel;
2. re-dial (ADR-022): freeze one sidecar with SIGSTOP past the ICE consent
   timeout, resume it, and require the connection to heal on its own with
   no process restart and no signaling reconnect;
3. offer glare (ADR-021): make both sidecars register at the same instant so
   both send an offer, and require the tie-break to produce one connection.

Run from the repo root:  uvx pytest tests/integration -v
"""

from __future__ import annotations

import asyncio
import time

from harness import (
    ICE_CONSENT_FAILURE_S,
    PEER_A,
    PEER_B,
    FakeAgent,
    MeshCluster,
    is_connected,
    is_disconnected,
    run_scenario,
)

CONNECT_TIMEOUT = 30.0
MESSAGE_TIMEOUT = 10.0
# Freeze long enough that the other side's ICE agent declares consent failed.
PAUSE_S = ICE_CONSENT_FAILURE_S + 6.0


async def connect_pair(cluster: MeshCluster) -> tuple[FakeAgent, FakeAgent]:
    """Start A, wait until it has registered, then start B (B offers to A)."""
    await cluster.start_registry()
    a = await cluster.start_sidecar(PEER_A)
    assert cluster.registry is not None
    await cluster.registry.wait_for_log(rf"register from peer_id={PEER_A}\b", 20)
    b = await cluster.start_sidecar(PEER_B)
    await wait_linked(cluster, a, b, CONNECT_TIMEOUT)
    return a, b


async def wait_linked(
    cluster: MeshCluster,
    a: FakeAgent,
    b: FakeAgent,
    timeout: float,
    start_a: int = 0,
    start_b: int = 0,
) -> None:
    t = time.monotonic()
    await asyncio.gather(
        a.wait_for(is_connected(PEER_B), timeout, f"peer_connected({PEER_B})", start_a),
        b.wait_for(is_connected(PEER_A), timeout, f"peer_connected({PEER_A})", start_b),
    )
    cluster.log(f"both sides report peer_connected ({time.monotonic() - t:.1f}s)")


async def assert_delivery(sender: FakeAgent, receiver: FakeAgent, text: str) -> None:
    start = receiver.mark()
    t = time.monotonic()
    request_id = await sender.send_text(receiver.peer_id, text)
    event = await receiver.wait_for(
        lambda e: (
            e.get("type") == "message"
            and e["payload"].get("message", {}).get("request_id") == request_id
        ),
        MESSAGE_TIMEOUT,
        f"message {request_id} from {sender.peer_id}",
        start,
    )
    payload = event["payload"]
    assert payload["from_peer"] == sender.peer_id
    assert payload["message"]["type"] == "agent_prompt"
    assert payload["message"]["source_agent"] == sender.peer_id
    assert payload["message"]["payload"] == {"text": text}
    receiver_latency_ms = (event["_t"] - t) * 1000
    print(
        f"  {sender.peer_id} -> {receiver.peer_id}: delivered in {receiver_latency_ms:.1f} ms"
    )


async def assert_healthy_link(a: FakeAgent, b: FakeAgent, label: str) -> None:
    """Messages flow both ways and both sidecars list each other as connected."""
    await assert_delivery(a, b, f"{label}: hello from {a.peer_id}")
    await assert_delivery(b, a, f"{label}: hello from {b.peer_id}")
    assert PEER_B in await a.connected_peers()
    assert PEER_A in await b.connected_peers()
    # No stale peer_disconnected after the latest peer_connected.
    assert a.last_link_event(PEER_B) == "peer_connected", a.describe()
    assert b.last_link_event(PEER_A) == "peer_connected", b.describe()


# ─── 1. Connect and exchange messages ────────────────────────────────────


def test_connect_and_exchange_messages(binaries):
    async def scenario(cluster: MeshCluster) -> None:
        a, b = await connect_pair(cluster)
        await assert_healthy_link(a, b, "initial")
        # The DataChannel runs over a direct ICE pair (host here; a CI runner
        # with STUN may also offer srflx), never a TURN relay.
        for peer in (PEER_A, PEER_B):
            await cluster.sidecar(peer).wait_for_log(
                r"selected pair: local=(host|srflx|prflx) .* remote=(host|srflx|prflx) ",
                10,
            )

    run_scenario(binaries, "connect_and_exchange_messages", scenario, timeout=60)


# ─── 2. Re-dial after an ICE consent timeout (ADR-022) ───────────────────


def test_redial_after_ice_consent_timeout(binaries):
    async def scenario(cluster: MeshCluster) -> None:
        a, b = await connect_pair(cluster)
        await assert_healthy_link(a, b, "before pause")

        proc_a, proc_b = cluster.sidecar(PEER_A), cluster.sidecar(PEER_B)
        a_log = proc_a.log_offset()
        a_mark = a.mark()
        cluster.log(f"SIGSTOP {PEER_B} for {PAUSE_S:.0f}s")
        paused_at = time.monotonic()
        proc_b.pause()

        # A notices on its own: ICE goes Disconnected after ~5 s of silence.
        await a.wait_for(
            is_disconnected(PEER_B), 20, f"peer_disconnected({PEER_B})", a_mark
        )
        cluster.log(f"{PEER_A} reported peer_disconnected")
        # ADR-022: A re-registers to re-dial while signaling is still up.
        await proc_a.wait_for_log(
            rf"re-dialing {PEER_B} after connection failure", 15, a_log
        )
        cluster.log(f"{PEER_A} re-dialed while {PEER_B} is frozen")

        await asyncio.sleep(max(0.0, PAUSE_S - (time.monotonic() - paused_at)))
        # The pause really outlasted consent: A's old connection hit Failed.
        await proc_a.wait_for_log(
            rf"\[WEBRTC\] {PEER_B} connection state: failed", 1, a_log
        )

        a_mark, b_mark = a.mark(), b.mark()
        b_log = proc_b.log_offset()
        proc_b.resume()
        cluster.log(f"SIGCONT {PEER_B}")
        await wait_linked(cluster, a, b, 45, a_mark, b_mark)
        await assert_healthy_link(a, b, "after resume")

        # Healed over the mesh, not by reconnecting signaling or restarting.
        for proc, since in ((proc_a, a_log), (proc_b, b_log)):
            assert proc.running(), f"{proc.name} exited"
            text = proc.log_text(since)
            assert "[SIG] connection closed" not in text, f"{proc.name} lost signaling"
            assert "connecting (attempt" not in text, (
                f"{proc.name} reconnected signaling"
            )

    run_scenario(binaries, "redial_after_ice_consent_timeout", scenario, timeout=150)


# ─── 3. Simultaneous dial: offer glare (ADR-021) ─────────────────────────


def test_simultaneous_dial_glare(binaries):
    async def scenario(cluster: MeshCluster) -> None:
        await cluster.start_registry()
        # Both sidecars start at once; the gate holds their signaling
        # upgrades and releases them together, so each one's register
        # gets a peer_list naming the other and both send an offer.
        a, b = await asyncio.gather(
            cluster.start_sidecar(PEER_A), cluster.start_sidecar(PEER_B)
        )
        assert cluster.gate is not None
        await wait_linked(cluster, a, b, CONNECT_TIMEOUT)
        assert cluster.gate.held == 2

        proc_a, proc_b = cluster.sidecar(PEER_A), cluster.sidecar(PEER_B)
        # Both really offered ...
        await proc_a.wait_for_log(rf"initiating connection to peer: {PEER_B}", 1)
        await proc_b.wait_for_log(rf"initiating connection to peer: {PEER_A}", 1)
        # ... and the tie-break ran: the lower peer_id keeps its offer, the
        # higher one yields and answers.
        await proc_a.wait_for_log(rf"offer glare with {PEER_B}: keeping our offer", 1)
        await proc_b.wait_for_log(
            rf"offer glare with {PEER_A}: yielding to their offer", 1
        )

        await assert_healthy_link(a, b, "after glare")

    run_scenario(binaries, "simultaneous_dial_glare", scenario, timeout=60, gate_ws=2)
