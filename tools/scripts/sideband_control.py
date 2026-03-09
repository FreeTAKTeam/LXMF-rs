#!/usr/bin/env python3
import argparse
import json
import os
import signal
import sys
import time
from pathlib import Path


def _load_core(sideband_root: Path):
    sys.path.insert(0, str(sideband_root))
    from sbapp.sideband.core import SidebandCore  # type: ignore

    return SidebandCore


def _json_default(value):
    if isinstance(value, bytes):
        return value.hex()
    raise TypeError(f"unsupported json value: {type(value)!r}")


def _write_json(path: Path, payload):
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_suffix(path.suffix + ".tmp")
    tmp.write_text(json.dumps(payload, indent=2, sort_keys=True, default=_json_default))
    tmp.replace(path)


def _normalise_text(value):
    if isinstance(value, bytes):
        return value.decode("utf-8", errors="replace")
    return value


def _find_message(core, context_hash: bytes, content: str, direction: str):
    messages = core._db_messages(context_hash, limit=512) or []
    for message in reversed(messages):
        source = message["source"]
        dest = message["dest"]
        message_content = _normalise_text(message["content"])
        if message_content != content:
            continue
        if direction == "inbound" and source == core.lxmf_destination.hash:
            continue
        if direction == "outbound" and source != core.lxmf_destination.hash:
            continue
        return {
            "hash": message["hash"].hex(),
            "source": source.hex(),
            "dest": dest.hex(),
            "content": message_content,
            "state": message["state"],
            "method": message["method"],
            "received": message["received"],
            "sent": message["sent"],
            "title": _normalise_text(message["title"]),
        }
    return None


def serve(args):
    sideband_root = Path(args.sideband_root).resolve()
    control_dir = Path(args.control_dir).resolve()
    commands_dir = control_dir / "commands"
    results_dir = control_dir / "results"
    state_path = control_dir / "state.json"
    commands_dir.mkdir(parents=True, exist_ok=True)
    results_dir.mkdir(parents=True, exist_ok=True)

    SidebandCore = _load_core(sideband_root)
    core = SidebandCore(
        None,
        config_path=args.config_dir,
        is_client=False,
        verbose=args.verbose,
        quiet=not args.verbose,
        is_daemon=True,
        rns_config_path=args.rns_config_dir,
    )
    core.version_str = "interop-harness"
    core.start()

    deadline = time.time() + args.start_timeout
    while time.time() < deadline:
        if core.getstate("core.started") is True and getattr(core, "lxmf_destination", None) is not None:
            break
        time.sleep(0.1)
    else:
        raise RuntimeError("sideband core did not start before timeout")

    _write_json(
        state_path,
        {
            "sideband_root": str(sideband_root),
            "config_dir": args.config_dir,
            "rns_config_dir": args.rns_config_dir,
            "db_path": core.db_path,
            "lxmf_hash": core.lxmf_destination.hash.hex(),
        },
    )

    running = True

    def stop_handler(_signum, _frame):
        nonlocal running
        running = False

    signal.signal(signal.SIGTERM, stop_handler)
    signal.signal(signal.SIGINT, stop_handler)

    while running:
        for command_path in sorted(commands_dir.glob("*.json")):
            try:
                request = json.loads(command_path.read_text())
            except json.JSONDecodeError:
                continue
            result_path = results_dir / f"{command_path.stem}.json"
            try:
                command = request["command"]
                if command == "send":
                    destination_hash = bytes.fromhex(request["destination_hash"])
                    content = request["content"]
                    propagation = bool(request.get("propagation", False))
                    core.create_conversation(destination_hash)
                    accepted = bool(
                        core.send_message(
                            content,
                            destination_hash,
                            propagation,
                            skip_fields=True,
                            no_display=True,
                        )
                    )
                    response = {"ok": accepted, "command": command}
                elif command == "find_message":
                    context_hash = bytes.fromhex(request["context_hash"])
                    content = request["content"]
                    direction = request.get("direction", "any")
                    message = _find_message(core, context_hash, content, direction)
                    response = {
                        "ok": message is not None,
                        "command": command,
                        "message": message,
                    }
                elif command == "shutdown":
                    running = False
                    response = {"ok": True, "command": command}
                else:
                    raise ValueError(f"unsupported command '{command}'")
            except Exception as exc:  # pragma: no cover - harness failure path
                response = {"ok": False, "error": str(exc)}

            _write_json(result_path, response)
            command_path.unlink(missing_ok=True)
        time.sleep(0.1)


def main():
    parser = argparse.ArgumentParser(description="Sideband interop harness control shim")
    subparsers = parser.add_subparsers(dest="subcommand", required=True)

    serve_parser = subparsers.add_parser("serve")
    serve_parser.add_argument("--sideband-root", required=True)
    serve_parser.add_argument("--config-dir", required=True)
    serve_parser.add_argument("--rns-config-dir", required=True)
    serve_parser.add_argument("--control-dir", required=True)
    serve_parser.add_argument("--start-timeout", type=float, default=30.0)
    serve_parser.add_argument("--verbose", action="store_true", default=False)

    args = parser.parse_args()

    if args.subcommand == "serve":
        serve(args)


if __name__ == "__main__":
    main()
