"""Chatixia CLI — deploy and run AI agents on the mesh network."""

from __future__ import annotations

import argparse
import sys

from chatixia import __version__


def main(argv: list[str] | None = None) -> int:
    """CLI entry point (registered as ``chatixia`` console script)."""
    parser = argparse.ArgumentParser(
        prog="chatixia",
        description="Deploy and run AI agents on the Chatixia mesh network.",
    )
    parser.add_argument(
        "-V",
        "--version",
        action="version",
        version=f"chatixia {__version__}",
    )

    subparsers = parser.add_subparsers(dest="command")

    # -- chatixia init ------------------------------------------------------
    init_parser = subparsers.add_parser(
        "init",
        help="Scaffold a new agent (agent.yaml + .env.example)",
    )
    init_parser.add_argument(
        "name",
        nargs="?",
        default="my-agent",
        help="Agent name (default: my-agent)",
    )
    init_parser.add_argument(
        "-d",
        "--directory",
        default=".",
        help="Directory to create the manifest in (default: current)",
    )

    # -- chatixia run -------------------------------------------------------
    run_parser = subparsers.add_parser(
        "run",
        help="Run an agent and connect to the mesh",
    )
    run_parser.add_argument(
        "manifest",
        nargs="?",
        default="agent.yaml",
        help="Path to agent.yaml (default: ./agent.yaml)",
    )

    # -- chatixia validate --------------------------------------------------
    validate_parser = subparsers.add_parser(
        "validate",
        help="Validate an agent manifest",
    )
    validate_parser.add_argument(
        "manifest",
        nargs="?",
        default="agent.yaml",
        help="Path to agent.yaml (default: ./agent.yaml)",
    )

    # -- chatixia pair ------------------------------------------------------
    pair_parser = subparsers.add_parser(
        "pair",
        help="Pair this agent with a mesh network using an invite code",
    )
    pair_parser.add_argument(
        "code",
        help="6-digit invite code",
    )
    pair_parser.add_argument(
        "manifest",
        nargs="?",
        default="agent.yaml",
        help="Path to agent.yaml (default: ./agent.yaml)",
    )

    args = parser.parse_args(argv)

    if args.command is None:
        parser.print_help()
        return 0

    if args.command == "init":
        return _cmd_init(args)
    elif args.command == "run":
        return _cmd_run(args)
    elif args.command == "validate":
        return _cmd_validate(args)
    elif args.command == "pair":
        return _cmd_pair(args)

    return 0


# ---------------------------------------------------------------------------
# Subcommand handlers
# ---------------------------------------------------------------------------


def _cmd_init(args: argparse.Namespace) -> int:
    """Execute the ``init`` subcommand."""
    from chatixia.scaffold import write_scaffold

    # If no explicit -d given, create a subdirectory named after the agent
    directory = args.directory if args.directory != "." else args.name

    try:
        write_scaffold(args.name, directory)
        return 0
    except FileExistsError as exc:
        print(f"Error: {exc}", file=sys.stderr)
        return 1
    except Exception as exc:
        print(f"Error: {exc}", file=sys.stderr)
        return 1


def _cmd_run(args: argparse.Namespace) -> int:
    """Execute the ``run`` subcommand."""
    import asyncio

    from chatixia.config import load_config
    from chatixia.runner import run_agent

    try:
        config = load_config(args.manifest)
    except FileNotFoundError as exc:
        print(f"Error: {exc}", file=sys.stderr)
        return 1
    except Exception as exc:
        print(f"Error loading manifest: {exc}", file=sys.stderr)
        return 1

    errors = config.validate()
    if errors:
        for err in errors:
            print(f"  Error: {err}", file=sys.stderr)
        return 1

    try:
        asyncio.run(run_agent(config))
    except KeyboardInterrupt:
        print("\nGoodbye!")
    except RuntimeError as exc:
        print(f"Error: {exc}", file=sys.stderr)
        return 1

    return 0


def _cmd_validate(args: argparse.Namespace) -> int:
    """Execute the ``validate`` subcommand."""
    from chatixia.config import load_config

    try:
        config = load_config(args.manifest)
    except FileNotFoundError as exc:
        print(f"Error: {exc}", file=sys.stderr)
        return 1
    except Exception as exc:
        print(f"Error parsing manifest: {exc}", file=sys.stderr)
        return 1

    errors = config.validate()

    if errors:
        print(f"{len(errors)} error(s) in {args.manifest}:")
        for err in errors:
            print(f"  - {err}")
        return 1

    print(f"OK: {args.manifest}")
    print(f"  Agent:    {config.name}")
    print(f"  Registry: {config.registry}")
    print(f"  Provider: {config.provider}")
    skills_count = len(config.skills_builtin)
    print(f"  Skills:   {skills_count} builtin")
    if config.agent_md:
        lines = len(config.agent_md.strip().splitlines())
        print(f"  AGENT.md: loaded ({lines} lines)")
    else:
        print("  AGENT.md: not found (optional)")

    return 0


def _cmd_pair(args: argparse.Namespace) -> int:
    """Execute the ``pair`` subcommand — redeem invite code to join the mesh."""
    import json

    import requests

    from chatixia.config import load_config

    try:
        config = load_config(args.manifest)
    except FileNotFoundError as exc:
        print(f"Error: {exc}", file=sys.stderr)
        return 1
    except Exception as exc:
        print(f"Error loading manifest: {exc}", file=sys.stderr)
        return 1

    code = args.code.strip()
    if len(code) != 6 or not code.isdigit():
        print("Error: invite code must be exactly 6 digits", file=sys.stderr)
        return 1

    registry = config.registry.rstrip("/")
    print(f"Pairing '{config.name}' with {registry} ...")

    try:
        resp = requests.post(
            f"{registry}/api/pairing/pair",
            json={"code": code, "agent_name": config.name},
            timeout=10,
        )
    except requests.ConnectionError:
        print(f"Error: cannot reach registry at {registry}", file=sys.stderr)
        return 1

    if resp.status_code != 200:
        error = (
            resp.json().get("error", resp.text)
            if resp.headers.get("content-type", "").startswith("application/json")
            else resp.text
        )
        print(f"Error: {error}", file=sys.stderr)
        return 1

    data = resp.json()
    status = data.get("status", "unknown")
    peer_id = data.get("peer_id", "")
    entry_id = data.get("id", "")

    print(f"Paired! Status: {status}")
    print(f"  Entry ID: {entry_id}")
    print(f"  Peer ID:  {peer_id}")

    if status == "pending_approval":
        print("\nWaiting for admin approval in the hub dashboard.")
        print("Once approved, your device token will be shown here.")
        print(
            f"Check status: curl {registry}/api/pairing/all | jq '.[] | select(.id==\"{entry_id}\")'"
        )

    # Save pairing result to .chatixia/pairing.json for later use
    from pathlib import Path

    data_dir = Path(config.data_dir)
    data_dir.mkdir(parents=True, exist_ok=True)
    pairing_file = data_dir / "pairing.json"
    pairing_data = {
        "entry_id": entry_id,
        "peer_id": peer_id,
        "status": status,
        "registry": registry,
    }
    pairing_file.write_text(json.dumps(pairing_data, indent=2), encoding="utf-8")
    print(f"\nPairing info saved to {pairing_file}")

    return 0


if __name__ == "__main__":
    sys.exit(main())
