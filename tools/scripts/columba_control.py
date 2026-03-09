#!/usr/bin/env python3
import argparse
import atexit
import json
import signal
import sys
import time
from pathlib import Path


def _load_wrapper(columba_root: Path):
    python_dir = columba_root / "python"
    sys.path.insert(0, str(python_dir))
    from reticulum_wrapper import ReticulumWrapper  # type: ignore

    return ReticulumWrapper


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


def _normalise_message(message):
    fields = message.get("fields") or {}
    return {
        "message_hash": _normalise_text(message.get("message_hash")),
        "content": _normalise_text(message.get("content")),
        "source_hash": _json_default(message["source_hash"]) if isinstance(message.get("source_hash"), bytes) else message.get("source_hash"),
        "destination_hash": _json_default(message["destination_hash"]) if isinstance(message.get("destination_hash"), bytes) else message.get("destination_hash"),
        "timestamp": message.get("timestamp"),
        "hops": message.get("hops"),
        "receiving_interface": message.get("receiving_interface"),
        "public_key": _json_default(message["public_key"]) if isinstance(message.get("public_key"), bytes) else message.get("public_key"),
        "fields": {key: _normalise_text(value) for key, value in fields.items()},
    }


def _find_message(messages, context_hash: str, content: str, direction: str):
    for message in reversed(messages):
        if message["content"] != content:
            continue
        if context_hash and context_hash not in (message["source_hash"], message["destination_hash"]):
            continue
        if direction == "inbound" and message["source_hash"] == context_hash:
            return message
        if direction == "outbound" and message["destination_hash"] == context_hash:
            return message
        if direction == "any":
            return message
    return None


def serve(args):
    columba_root = Path(args.columba_root).resolve()
    storage_dir = Path(args.storage_dir).resolve()
    control_dir = Path(args.control_dir).resolve()
    commands_dir = control_dir / "commands"
    results_dir = control_dir / "results"
    state_path = control_dir / "state.json"
    commands_dir.mkdir(parents=True, exist_ok=True)
    results_dir.mkdir(parents=True, exist_ok=True)
    storage_dir.mkdir(parents=True, exist_ok=True)

    ReticulumWrapper = _load_wrapper(columba_root)
    wrapper = ReticulumWrapper(str(storage_dir))
    config = {
        "storagePath": str(storage_dir),
        "enabledInterfaces": [
            {
                "type": "TCPClient",
                "target_host": args.transport_host,
                "target_port": args.transport_port,
            }
        ],
        "logLevel": args.log_level,
        "allowAnonymous": True,
        "display_name": args.display_name,
        "enable_transport": False,
    }
    result = wrapper.initialize(json.dumps(config))
    if not result.get("success"):
        raise RuntimeError(f"columba initialize failed: {result}")

    deadline = time.time() + args.start_timeout
    while time.time() < deadline:
        destination = wrapper.get_lxmf_destination()
        identity = wrapper.get_lxmf_identity()
        if "error" not in destination and "error" not in identity:
            break
        time.sleep(0.1)
    else:
        raise RuntimeError("columba wrapper did not expose identity before timeout")

    destination = wrapper.get_lxmf_destination()
    identity = wrapper.get_lxmf_identity()
    _write_json(
        state_path,
        {
            "columba_root": str(columba_root),
            "storage_dir": str(storage_dir),
            "lxmf_hash": destination["hex_hash"],
            "identity_hash": identity["hash"].hex(),
        },
    )

    running = True
    seen_hashes = set()
    received_messages = []

    def stop_handler(_signum, _frame):
        nonlocal running
        running = False

    signal.signal(signal.SIGTERM, stop_handler)
    signal.signal(signal.SIGINT, stop_handler)

    try:
        while running:
            for message in wrapper.poll_received_messages():
                normalised = _normalise_message(message)
                msg_hash = normalised["message_hash"]
                if msg_hash in seen_hashes:
                    continue
                seen_hashes.add(msg_hash)
                received_messages.append(normalised)

            for command_path in sorted(commands_dir.glob("*.json")):
                try:
                    request = json.loads(command_path.read_text())
                except json.JSONDecodeError:
                    continue
                result_path = results_dir / f"{command_path.stem}.json"
                try:
                    command = request["command"]
                    if command == "send":
                        identity = wrapper.get_lxmf_identity()
                        response = wrapper.send_lxmf_message(
                            bytes.fromhex(request["destination_hash"]),
                            request["content"],
                            identity["private_key"],
                        )
                        response["ok"] = bool(response.get("success"))
                    elif command == "find_message":
                        message = _find_message(
                            received_messages,
                            request.get("context_hash", ""),
                            request["content"],
                            request.get("direction", "any"),
                        )
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
    finally:
        try:
            wrapper.shutdown()
        finally:
            try:
                import RNS  # type: ignore

                atexit.unregister(RNS.Reticulum.exit_handler)
            except Exception:
                pass


def main():
    parser = argparse.ArgumentParser(description="Columba interop harness control shim")
    subparsers = parser.add_subparsers(dest="subcommand", required=True)

    serve_parser = subparsers.add_parser("serve")
    serve_parser.add_argument("--columba-root", required=True)
    serve_parser.add_argument("--storage-dir", required=True)
    serve_parser.add_argument("--control-dir", required=True)
    serve_parser.add_argument("--transport-host", default="127.0.0.1")
    serve_parser.add_argument("--transport-port", type=int, required=True)
    serve_parser.add_argument("--display-name", default="Columba Interop")
    serve_parser.add_argument("--log-level", default="INFO")
    serve_parser.add_argument("--start-timeout", type=float, default=30.0)

    args = parser.parse_args()

    if args.subcommand == "serve":
        serve(args)


if __name__ == "__main__":
    main()
