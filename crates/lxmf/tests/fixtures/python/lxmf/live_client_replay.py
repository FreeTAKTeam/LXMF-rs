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
import LXMF  # noqa: E402
from LXMF.LXMessage import LXMessage  # noqa: E402


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
                    "  instance_name = replay-live",
                    "",
                    "[interfaces]",
                    "  [[Default Interface]]",
                    "    type = AutoInterface",
                    "    enabled = No",
                    "",
                ]
            )
        )


def _decode_text(value):
    if isinstance(value, bytes):
        try:
            return value.decode("utf-8")
        except Exception:
            return base64.b64encode(value).decode("ascii")
    if isinstance(value, str):
        return value
    return None


def _field_key_ints(fields):
    keys = []
    if not isinstance(fields, dict):
        return keys
    for key in fields.keys():
        if isinstance(key, int):
            keys.append(key)
            continue
        if isinstance(key, str):
            try:
                keys.append(int(key, 0))
                continue
            except Exception:
                pass
    return sorted(set(keys))


def _attachment_names(fields):
    if not isinstance(fields, dict):
        return []
    attachments = fields.get(LXMF.FIELD_FILE_ATTACHMENTS, [])
    if not isinstance(attachments, list):
        return []

    names = []
    for attachment in attachments:
        if not isinstance(attachment, (list, tuple)) or len(attachment) < 1:
            continue
        name = _decode_text(attachment[0])
        if isinstance(name, str):
            names.append(name)
    return names


def _extract_metadata(message):
    fields = message.fields if isinstance(message.fields, dict) else {}
    commands = fields.get(LXMF.FIELD_COMMANDS, [])
    return {
        "title": message.title_as_string(),
        "content": message.content_as_string(),
        "signature_validated": bool(message.signature_validated),
        "field_keys": _field_key_ints(fields),
        "attachment_names": _attachment_names(fields),
        "has_embedded_lxms": LXMF.FIELD_EMBEDDED_LXMS in fields,
        "has_image": LXMF.FIELD_IMAGE in fields,
        "has_audio": LXMF.FIELD_AUDIO in fields,
        "has_telemetry_stream": LXMF.FIELD_TELEMETRY_STREAM in fields,
        "has_thread": LXMF.FIELD_THREAD in fields,
        "has_results": LXMF.FIELD_RESULTS in fields,
        "has_group": LXMF.FIELD_GROUP in fields,
        "has_event": LXMF.FIELD_EVENT in fields,
        "has_rnr_refs": LXMF.FIELD_RNR_REFS in fields,
        "renderer": fields.get(LXMF.FIELD_RENDERER),
        "commands_count": len(commands) if isinstance(commands, list) else 0,
        "has_telemetry": LXMF.FIELD_TELEMETRY in fields,
        "has_ticket": LXMF.FIELD_TICKET in fields,
        "has_custom_type": LXMF.FIELD_CUSTOM_TYPE in fields,
        "has_custom_data": LXMF.FIELD_CUSTOM_DATA in fields,
        "has_custom_meta": LXMF.FIELD_CUSTOM_META in fields,
        "has_non_specific": LXMF.FIELD_NON_SPECIFIC in fields,
        "has_debug": LXMF.FIELD_DEBUG in fields,
    }


def _decode_wire(wire_bytes):
    message = LXMessage.unpack_from_bytes(wire_bytes)
    return _extract_metadata(message)


def _decode_paper(paper_bytes, destination_identity):
    destination_hash = paper_bytes[:16]
    encrypted = paper_bytes[16:]
    decrypted = destination_identity.decrypt(encrypted)
    if decrypted is None:
        raise RuntimeError("paper decrypt failed")
    return _decode_wire(destination_hash + decrypted)


def _decode_propagation(propagation_bytes, destination_identity):
    envelope = msgpack.unpackb(propagation_bytes)
    if not isinstance(envelope, (list, tuple)) or len(envelope) < 2:
        raise RuntimeError("invalid propagation envelope")
    messages = envelope[1]
    if not isinstance(messages, (list, tuple)) or len(messages) == 0:
        raise RuntimeError("propagation envelope has no messages")
    lxm_data = messages[0]
    if not isinstance(lxm_data, bytes) or len(lxm_data) <= 16:
        raise RuntimeError("invalid propagated payload")
    destination_hash = lxm_data[:16]
    encrypted = lxm_data[16:]
    decrypted = destination_identity.decrypt(encrypted)
    if decrypted is None:
        raise RuntimeError("propagation decrypt failed")
    return _decode_wire(destination_hash + decrypted)


def _build_vectors(source, destination):
    return [
        {
            "id": "sideband_file_markdown",
            "title": "Sideband File",
            "content": "Hello **Sideband**",
            "fields": {
                LXMF.FIELD_FILE_ATTACHMENTS: [
                    ["notes.txt", b"hello sideband"],
                    ["map.geojson", b'{"type":"FeatureCollection","features":[]}'],
                ],
                LXMF.FIELD_RENDERER: LXMF.RENDERER_MARKDOWN,
            },
        },
        {
            "id": "meshchat_media_icon",
            "title": "Mesh Media",
            "content": "media packet",
            "fields": {
                LXMF.FIELD_IMAGE: [b"image/png", b"\x89PNG\r\n\x1a\n\x00"],
                LXMF.FIELD_AUDIO: [LXMF.AM_CODEC2_1200, b"\x01\x02\x03\x04"],
                LXMF.FIELD_ICON_APPEARANCE: [
                    b"map-marker",
                    bytes([255, 204, 0]),
                    bytes([17, 34, 51]),
                ],
            },
        },
        {
            "id": "commands_ticket",
            "title": "Ops",
            "content": "cmd set",
            "fields": {
                LXMF.FIELD_COMMANDS: [{0x01: b"ping"}, {0x02: b"echo hi"}],
                LXMF.FIELD_TICKET: bytes([0xAA] * (RNS.Identity.TRUNCATED_HASHLENGTH // 8)),
                LXMF.FIELD_NON_SPECIFIC: b"note",
            },
        },
        {
            "id": "telemetry_custom",
            "title": "Telemetry",
            "content": "stats",
            "fields": {
                LXMF.FIELD_TELEMETRY: msgpack.packb(
                    {"temp_c": 24.5, "battery": 88, "ok": True},
                    use_bin_type=True,
                ),
                LXMF.FIELD_CUSTOM_TYPE: b"meshchatx/location",
                LXMF.FIELD_CUSTOM_DATA: b"\x10\x20\x30",
            },
        },
        {
            "id": "thread_group_event_refs",
            "title": "Context",
            "content": "threaded",
            "fields": {
                LXMF.FIELD_THREAD: b"thread-001",
                LXMF.FIELD_RESULTS: [{0x01: b"ok"}, {0x02: b"accepted"}],
                LXMF.FIELD_GROUP: b"group-alpha",
                LXMF.FIELD_EVENT: b"event-join",
                LXMF.FIELD_RNR_REFS: [b"ref-1", b"ref-2"],
                LXMF.FIELD_RENDERER: LXMF.RENDERER_MICRON,
            },
        },
        {
            "id": "embedded_stream_debug",
            "title": "Embedded",
            "content": "capsule",
            "fields": {
                LXMF.FIELD_EMBEDDED_LXMS: [b"embedded-lxm-1", b"embedded-lxm-2"],
                LXMF.FIELD_TELEMETRY_STREAM: [
                    [
                        bytes([0x22] * 16),
                        1_700_001_999,
                        msgpack.packb({"alt": 120, "ok": True}, use_bin_type=True),
                        [b"person", bytes([0, 0, 0]), bytes([255, 255, 255])],
                    ]
                ],
                LXMF.FIELD_CUSTOM_TYPE: b"meshchatx/blob",
                LXMF.FIELD_CUSTOM_DATA: b"\xaa\xbb\xcc\xdd",
                LXMF.FIELD_CUSTOM_META: {b"scope": b"debug", b"v": 1},
                LXMF.FIELD_NON_SPECIFIC: b"nonspecific",
                LXMF.FIELD_DEBUG: {b"trace_id": b"abc123"},
            },
        },
    ]


def _generate():
    source_identity = RNS.Identity()
    destination_identity = RNS.Identity()
    source = RNS.Destination(
        source_identity, RNS.Destination.OUT, RNS.Destination.SINGLE, "lxmf", "interop"
    )
    destination = RNS.Destination(
        destination_identity, RNS.Destination.IN, RNS.Destination.SINGLE, "lxmf", "interop"
    )

    vectors = []
    base_timestamp = 1_700_001_000.0
    for index, template in enumerate(_build_vectors(source, destination)):
        ts = base_timestamp + index
        common_kwargs = dict(
            destination=destination,
            source=source,
            title=template["title"],
            content=template["content"],
            fields=template["fields"],
        )

        wire = LXMessage(**common_kwargs)
        wire.timestamp = ts
        wire.pack()

        paper = LXMessage(desired_method=LXMessage.PAPER, **common_kwargs)
        paper.timestamp = ts
        paper.pack()

        propagation = LXMessage(desired_method=LXMessage.PROPAGATED, **common_kwargs)
        propagation.timestamp = ts
        propagation.pack()

        vectors.append(
            {
                "id": template["id"],
                "title": template["title"],
                "content": template["content"],
                "wire_b64": base64.b64encode(wire.packed).decode("ascii"),
                "paper_b64": base64.b64encode(paper.paper_packed).decode("ascii"),
                "propagation_b64": base64.b64encode(propagation.propagation_packed).decode(
                    "ascii"
                ),
                "expected": _extract_metadata(wire),
            }
        )

    return {
        "source_public_b64": base64.b64encode(source_identity.get_public_key()).decode("ascii"),
        "source_hash_hex": source.hash.hex(),
        "destination_private_b64": base64.b64encode(destination_identity.get_private_key()).decode(
            "ascii"
        ),
        "vectors": vectors,
    }


def _verify(payload):
    source_hash = bytes.fromhex(payload["source_hash_hex"])
    source_public = base64.b64decode(payload["source_public_b64"])
    RNS.Identity.remember(b"\x00" * 16, source_hash, source_public)

    destination_private = base64.b64decode(payload["destination_private_b64"])
    destination_identity = RNS.Identity.from_bytes(destination_private)
    if destination_identity is None:
        raise RuntimeError("invalid destination private identity bytes")

    outputs = []
    for vector in payload.get("vectors", []):
        wire = _decode_wire(base64.b64decode(vector["wire_b64"]))
        paper = _decode_paper(base64.b64decode(vector["paper_b64"]), destination_identity)
        propagation = _decode_propagation(
            base64.b64decode(vector["propagation_b64"]), destination_identity
        )
        outputs.append(
            {
                "id": vector.get("id"),
                "wire": wire,
                "paper": paper,
                "propagation": propagation,
            }
        )

    return {"vectors": outputs}


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
