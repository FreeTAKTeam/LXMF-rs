#!/usr/bin/env python3
import argparse
import asyncio
import pathlib
import sys
import time
from dataclasses import dataclass
from typing import Optional

REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]
if str(REPO_ROOT) not in sys.path:
    sys.path.insert(0, str(REPO_ROOT))

try:
    from bleak import BleakClient
except Exception:  # pragma: no cover
    print("error: bleak is required. install with: python3 -m pip install bleak", file=sys.stderr)
    raise

from tools.experimental.esp32_camera_capture_bleak import find_devices

FRAME_NATIVE_ANNOUNCE_REQ = 0x21
FRAME_NATIVE_MESSAGE_TX_REQ = 0x22
FRAME_NATIVE_WIRE = 0x23

LOG_LEVELS = {"debug": 10, "info": 20, "warn": 30, "error": 40}
CURRENT_LOG_LEVEL = LOG_LEVELS["info"]


def log(level: str, msg: str) -> None:
    if LOG_LEVELS[level] < CURRENT_LOG_LEVEL:
        return
    ts = time.strftime("%H:%M:%S")
    print(f"[native-probe-{level} {ts}] {msg}", file=sys.stderr)


@dataclass
class ProbeResult:
    device_id: str
    device_name: str
    native_outbound_count: int
    native_inbound_count: int


def encode_runtime_frame(kind: int, sequence: int, payload: bytes) -> bytes:
    if not payload:
        raise ValueError("payload must not be empty")
    if len(payload) > 1024 * 1024:
        raise ValueError("payload too large")
    out = bytearray()
    out.extend(b"RNE1")
    out.append(0x01)
    out.append(kind & 0xFF)
    out.extend(int(sequence).to_bytes(4, "little"))
    out.extend(len(payload).to_bytes(4, "little"))
    out.extend(payload)
    return bytes(out)


def decode_runtime_frame(frame: bytes) -> tuple[int, int, bytes]:
    if len(frame) < 14:
        raise ValueError("frame too short")
    if frame[0:4] != b"RNE1":
        raise ValueError("bad magic")
    if frame[4] != 0x01:
        raise ValueError("unsupported version")
    kind = frame[5]
    sequence = int.from_bytes(frame[6:10], "little")
    payload_len = int.from_bytes(frame[10:14], "little")
    if payload_len == 0 or len(frame) != 14 + payload_len:
        raise ValueError("bad payload length")
    return kind, sequence, frame[14:]


async def probe_runtime(
    device_id: Optional[str],
    name_hint: str,
    service_uuid: str,
    write_uuid: str,
    notify_uuid: str,
    scan_secs: float,
    timeout_secs: float,
    permissive_scan: bool,
    rounds: int,
    max_probes: int,
    queue_message_body: Optional[bytes],
    send_raw_runtime: Optional[bytes],
    trigger_announce: bool,
) -> ProbeResult:
    for round_idx in range(1, max(1, rounds) + 1):
        log("info", f"scan round {round_idx}/{max(1, rounds)}")
        ranked = await find_devices(device_id, name_hint, service_uuid, scan_secs, permissive_scan)
        max_probe = min(len(ranked), max(1, max_probes))
        for idx, (score, _neg_rssi, device, _adv) in enumerate(ranked[:max_probe], start=1):
            dev_name = getattr(device, "name", "") or "<none>"
            dev_id = getattr(device, "address", "") or "<unknown>"
            log("info", f"probe {idx}/{max_probe} id={dev_id} name={dev_name} score={score}")
            q: asyncio.Queue[bytes] = asyncio.Queue()
            try:
                async with BleakClient(device, timeout=6.0) as client:
                    log("info", f"connected id={dev_id} name={dev_name}")
                    services = getattr(client, "services", None)
                    if services is None:
                        get_services = getattr(client, "get_services", None)
                        if callable(get_services):
                            services = await get_services()
                    notify_char = services.get_characteristic(notify_uuid)
                    write_char = services.get_characteristic(write_uuid)
                    if notify_char is None or write_char is None:
                        log("debug", "missing target characteristics, skipping")
                        continue
                    await client.start_notify(notify_uuid, lambda _h, data: q.put_nowait(bytes(data)))
                    native_outbound = 0
                    native_inbound = 0
                    saw_valid_native_outbound = False

                    if trigger_announce:
                        log("info", "sending native announce request")
                        await client.write_gatt_char(write_uuid, bytes([FRAME_NATIVE_ANNOUNCE_REQ]), response=False)

                    if queue_message_body is not None:
                        log("info", f"sending native message request body_bytes={len(queue_message_body)}")
                        await client.write_gatt_char(
                            write_uuid,
                            bytes([FRAME_NATIVE_MESSAGE_TX_REQ]) + queue_message_body,
                            response=False,
                        )

                    if send_raw_runtime is not None:
                        log("info", f"sending raw native wire bytes={len(send_raw_runtime)}")
                        await client.write_gatt_char(
                            write_uuid,
                            bytes([FRAME_NATIVE_WIRE]) + send_raw_runtime,
                            response=False,
                        )
                        native_inbound += 1

                    deadline = asyncio.get_running_loop().time() + timeout_secs
                    while True:
                        now = asyncio.get_running_loop().time()
                        if now >= deadline:
                            break
                        try:
                            frame = await asyncio.wait_for(q.get(), timeout=deadline - now)
                        except asyncio.TimeoutError:
                            break
                        if not frame:
                            continue
                        frame_type = frame[0]
                        if frame_type != FRAME_NATIVE_WIRE:
                            log("debug", f"notify type=0x{frame_type:02x} len={len(frame)}")
                            continue
                        native_outbound += 1
                        try:
                            kind, sequence, payload = decode_runtime_frame(frame[1:])
                            payload_preview = payload[:32].hex()
                            log(
                                "info",
                                f"native outbound frame kind=0x{kind:02x} seq={sequence} "
                                f"payload_bytes={len(payload)} payload_hex={payload_preview}",
                            )
                            saw_valid_native_outbound = True
                            if queue_message_body is not None or trigger_announce:
                                break
                        except Exception as exc:
                            log("warn", f"native outbound decode failed len={len(frame)-1}: {exc}")
                            continue

                    await client.stop_notify(notify_uuid)
                    if (queue_message_body is not None or trigger_announce) and not saw_valid_native_outbound:
                        raise RuntimeError("no native outbound frame received before timeout")
                    return ProbeResult(dev_id, dev_name, native_outbound, native_inbound)
            except Exception as exc:
                log("warn", f"probe failed id={dev_id} name={dev_name}: {exc}")
                continue
    raise RuntimeError("failed to connect to runtime-capable ESP32 camera over BLE")


def main() -> int:
    p = argparse.ArgumentParser(description="Probe ESP32 native runtime frames over BLE")
    p.add_argument("--device-id", default="", help="BLE device id/address (optional)")
    p.add_argument("--name-hint", default="LXMF", help="BLE name hint")
    p.add_argument("--service-uuid", required=True)
    p.add_argument("--write-char-uuid", required=True)
    p.add_argument("--notify-char-uuid", required=True)
    p.add_argument("--scan-secs", type=float, default=10)
    p.add_argument("--timeout-secs", type=float, default=8)
    p.add_argument("--rounds", type=int, default=1)
    p.add_argument("--max-probes", type=int, default=10)
    p.add_argument("--permissive-scan", action="store_true")
    p.add_argument("--trigger-announce", action="store_true")
    p.add_argument("--queue-message", default="", help="queue a native runtime message body")
    p.add_argument("--send-runtime-kind", type=lambda v: int(v, 0), default=None)
    p.add_argument("--send-runtime-seq", type=int, default=1)
    p.add_argument("--send-runtime-payload", default="", help="ASCII payload for raw runtime frame")
    p.add_argument("--log-level", choices=["debug", "info", "warn", "error"], default="info")
    args = p.parse_args()

    global CURRENT_LOG_LEVEL
    CURRENT_LOG_LEVEL = LOG_LEVELS[args.log_level]

    queue_message_body = args.queue_message.encode("utf-8") if args.queue_message else None
    send_raw_runtime = None
    if args.send_runtime_kind is not None:
        payload = args.send_runtime_payload.encode("utf-8") if args.send_runtime_payload else b"ping"
        send_raw_runtime = encode_runtime_frame(args.send_runtime_kind, args.send_runtime_seq, payload)

    result = asyncio.run(
        probe_runtime(
            device_id=args.device_id.strip() or None,
            name_hint=args.name_hint,
            service_uuid=args.service_uuid,
            write_uuid=args.write_char_uuid,
            notify_uuid=args.notify_char_uuid,
            scan_secs=args.scan_secs,
            timeout_secs=args.timeout_secs,
            permissive_scan=args.permissive_scan,
            rounds=args.rounds,
            max_probes=args.max_probes,
            queue_message_body=queue_message_body,
            send_raw_runtime=send_raw_runtime,
            trigger_announce=args.trigger_announce,
        )
    )
    print(
        "NATIVE_RUNTIME_PROBE ok: "
        f"device_id={result.device_id} "
        f"device_name={result.device_name} "
        f"native_outbound={result.native_outbound_count} "
        f"native_inbound={result.native_inbound_count}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
