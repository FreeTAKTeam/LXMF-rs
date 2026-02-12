#!/usr/bin/env python3

import base64
import json
import os
import sys
import tempfile

ROOT = os.path.abspath(
    os.path.join(os.path.dirname(__file__), "..", "..", "..", "..", "..", "..")
)
RETICULUM = os.path.abspath(os.path.join(ROOT, "..", "Reticulum"))
sys.path.insert(0, ROOT)
sys.path.insert(0, RETICULUM)

import RNS  # noqa: E402
import RNS.vendor.umsgpack as msgpack  # noqa: E402
from LXMF.LXMessage import LXMessage  # noqa: E402


EXPECTED_TITLE = "interop-title"
EXPECTED_CONTENT = "interop-content"
EXPECTED_FIELDS = {
    "attachments": [
        {
            "name": "note.txt",
            "size": 5,
            "media_type": "text/plain",
            "hash": "sha256:deadbeef",
        }
    ],
    "_lxmf": {"scope": "chat", "app": "weft"},
    "announce": {"name": "node-alpha", "stamp_cost": 20},
}
FIXED_TIMESTAMP = 1_700_000_123.0


def _write_minimal_config(config_dir: str) -> None:
    os.makedirs(config_dir, exist_ok=True)
    config_path = os.path.join(config_dir, "config")
    with open(config_path, "w", encoding="utf-8") as handle:
        handle.write(
            "\n".join(
                [
                    "[reticulum]",
                    "  enable_transport = False",
                    "  share_instance = No",
                    "  instance_name = interop-live",
                    "",
                    "[interfaces]",
                    "  [[Default Interface]]",
                    "    type = AutoInterface",
                    "    enabled = No",
                    "",
                ]
            )
        )


def _normalise(value):
    if isinstance(value, bytes):
        try:
            return value.decode("utf-8")
        except Exception:
            return base64.b64encode(value).decode("ascii")
    if isinstance(value, dict):
        return {_normalise(key): _normalise(item) for key, item in value.items()}
    if isinstance(value, (list, tuple)):
        return [_normalise(item) for item in value]
    return value


def _attachment_names(fields):
    normalised = _normalise(fields or {})
    attachments = normalised.get("attachments", [])
    names = []
    for attachment in attachments:
        if isinstance(attachment, dict):
            name = attachment.get("name")
            if isinstance(name, str):
                names.append(name)
    return names


def _decode_wire(lxmf_bytes: bytes):
    message = LXMessage.unpack_from_bytes(lxmf_bytes)
    return {
        "title": message.title_as_string(),
        "content": message.content_as_string(),
        "signature_validated": bool(message.signature_validated),
        "attachment_names": _attachment_names(message.fields),
        "scope": _normalise((message.fields or {}).get("_lxmf", {})).get("scope"),
    }


def _decode_paper(paper_bytes: bytes, destination_identity):
    destination_hash = paper_bytes[:16]
    encrypted = paper_bytes[16:]
    decrypted = destination_identity.decrypt(encrypted)
    if decrypted is None:
        raise RuntimeError("paper decrypt failed")
    return _decode_wire(destination_hash + decrypted)


def _decode_propagation(propagation_bytes: bytes, destination_identity):
    envelope = msgpack.unpackb(propagation_bytes)
    if not isinstance(envelope, (list, tuple)) or len(envelope) < 2:
        raise RuntimeError("invalid propagation envelope")
    messages = envelope[1]
    if not isinstance(messages, (list, tuple)) or len(messages) == 0:
        raise RuntimeError("propagation envelope has no messages")
    lxmf_data = messages[0]
    if not isinstance(lxmf_data, bytes) or len(lxmf_data) <= 16:
        raise RuntimeError("invalid propagated lxmf payload")
    destination_hash = lxmf_data[:16]
    encrypted = lxmf_data[16:]
    decrypted = destination_identity.decrypt(encrypted)
    if decrypted is None:
        raise RuntimeError("propagation decrypt failed")
    return _decode_wire(destination_hash + decrypted)


def _generate():
    source_identity = RNS.Identity()
    destination_identity = RNS.Identity()

    source = RNS.Destination(
        source_identity, RNS.Destination.OUT, RNS.Destination.SINGLE, "lxmf", "interop"
    )
    destination = RNS.Destination(
        destination_identity, RNS.Destination.IN, RNS.Destination.SINGLE, "lxmf", "interop"
    )

    wire_message = LXMessage(
        destination,
        source,
        content=EXPECTED_CONTENT,
        title=EXPECTED_TITLE,
        fields=EXPECTED_FIELDS,
    )
    wire_message.timestamp = FIXED_TIMESTAMP
    wire_message.pack()

    paper_message = LXMessage(
        destination,
        source,
        content=EXPECTED_CONTENT,
        title=EXPECTED_TITLE,
        fields=EXPECTED_FIELDS,
        desired_method=LXMessage.PAPER,
    )
    paper_message.timestamp = FIXED_TIMESTAMP
    paper_message.pack()

    propagation_message = LXMessage(
        destination,
        source,
        content=EXPECTED_CONTENT,
        title=EXPECTED_TITLE,
        fields=EXPECTED_FIELDS,
        desired_method=LXMessage.PROPAGATED,
    )
    propagation_message.timestamp = FIXED_TIMESTAMP
    propagation_message.pack()

    return {
        "source_private_b64": base64.b64encode(source_identity.get_private_key()).decode("ascii"),
        "source_public_b64": base64.b64encode(source_identity.get_public_key()).decode("ascii"),
        "source_hash_hex": source.hash.hex(),
        "destination_private_b64": base64.b64encode(destination_identity.get_private_key()).decode(
            "ascii"
        ),
        "destination_public_b64": base64.b64encode(destination_identity.get_public_key()).decode(
            "ascii"
        ),
        "destination_hash_hex": destination.hash.hex(),
        "wire_b64": base64.b64encode(wire_message.packed).decode("ascii"),
        "paper_b64": base64.b64encode(paper_message.paper_packed).decode("ascii"),
        "propagation_b64": base64.b64encode(propagation_message.propagation_packed).decode(
            "ascii"
        ),
        "expected": {
            "title": EXPECTED_TITLE,
            "content": EXPECTED_CONTENT,
            "attachment_names": ["note.txt"],
            "scope": "chat",
        },
    }


def _verify(payload):
    source_hash = bytes.fromhex(payload["source_hash_hex"])
    source_public = base64.b64decode(payload["source_public_b64"])
    # Register source identity so signature validation works in unpack_from_bytes.
    RNS.Identity.remember(b"\x00" * 16, source_hash, source_public)

    destination_private = base64.b64decode(payload["destination_private_b64"])
    destination_identity = RNS.Identity.from_bytes(destination_private)
    if destination_identity is None:
        raise RuntimeError("invalid destination private identity bytes")

    wire = _decode_wire(base64.b64decode(payload["wire_b64"]))
    paper = _decode_paper(base64.b64decode(payload["paper_b64"]), destination_identity)
    propagation = _decode_propagation(
        base64.b64decode(payload["propagation_b64"]), destination_identity
    )
    return {"wire": wire, "paper": paper, "propagation": propagation}


def main():
    mode = sys.argv[1] if len(sys.argv) > 1 else "generate"

    with tempfile.TemporaryDirectory() as tmp:
        config_dir = os.path.join(tmp, ".reticulum")
        _write_minimal_config(config_dir)
        RNS.Reticulum(configdir=config_dir, loglevel=RNS.LOG_ERROR)

        if mode == "generate":
            print(json.dumps(_generate()))
            return
        if mode == "verify":
            payload = json.loads(sys.stdin.read())
            print(json.dumps(_verify(payload)))
            return
        raise SystemExit(f"Unsupported mode: {mode}")


if __name__ == "__main__":
    main()
