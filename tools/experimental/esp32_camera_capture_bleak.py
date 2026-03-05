#!/usr/bin/env python3
import argparse
import asyncio
import os
import subprocess
import sys
import time
from dataclasses import dataclass
from typing import Optional

try:
    from bleak import BleakClient, BleakScanner
except Exception as exc:  # pragma: no cover
    print("error: bleak is required. install with: python3 -m pip install bleak", file=sys.stderr)
    raise

FRAME_CAPTURE_REQ = 0x02
FRAME_CAPTURE_ACK = 0x03
FRAME_CHUNK = 0x04
FRAME_CHUNK_ACK = 0x05
FRAME_DONE = 0x06
FRAME_ERROR = 0x07
FRAME_NACK = 0x08
LOG_LEVELS = {"debug": 10, "info": 20, "warn": 30, "error": 40}

CURRENT_LOG_LEVEL = LOG_LEVELS["info"]


def log(level: str, msg: str) -> None:
    if LOG_LEVELS[level] < CURRENT_LOG_LEVEL:
        return
    ts = time.strftime("%H:%M:%S")
    print(f"[bleak-{level} {ts}] {msg}", file=sys.stderr)


@dataclass
class CaptureResult:
    bytes_data: bytes
    device_id: str
    device_name: str


def norm(s: str) -> str:
    return "".join(ch.lower() for ch in s.strip() if ch not in ":-")


async def find_devices(
    device_id: Optional[str],
    name_hint: str,
    service_uuid: str,
    scan_secs: float,
    permissive: bool,
):
    log("info", f"scanning for {scan_secs:.1f}s")
    devices = await BleakScanner.discover(timeout=scan_secs, return_adv=True)
    device_id_n = norm(device_id) if device_id else ""
    hint_n = norm(name_hint)
    service_n = service_uuid.lower()
    log("info", f"discovered {len(devices)} candidates")

    ranked = []
    for _, (dev, adv) in devices.items():
        d_id = getattr(dev, "address", "") or ""
        d_name = getattr(dev, "name", "") or ""
        uuids = [u.lower() for u in (adv.service_uuids or [])]
        rssi = adv.rssi or -127

        score = 50
        if device_id_n and norm(d_id) == device_id_n:
            score = 0
        elif hint_n and hint_n in norm(d_name):
            score = 1
        elif service_n in uuids:
            score = 2

        ranked.append((score, -rssi, dev, adv))
        log(
            "debug",
            f"candidate id={d_id} name={d_name or '<none>'} rssi={rssi} "
            f"services={uuids} score={score}",
        )

    ranked.sort(key=lambda t: (t[0], t[1]))
    if not ranked:
        raise RuntimeError("no BLE devices discovered")

    best = ranked[0]
    if best[0] > 2 and (device_id or name_hint) and not permissive:
        raise RuntimeError("no scanned peripheral matched requested camera profile")
    if best[0] > 2 and permissive:
        log("warn", "no strict match found; will probe strongest candidates")
    return ranked


async def capture_over_ble(
    device_id: Optional[str],
    name_hint: str,
    service_uuid: str,
    write_uuid: str,
    notify_uuid: str,
    timeout_secs: float,
    scan_secs: float,
    permissive_scan: bool,
    rounds: int,
    max_probes: int,
) -> CaptureResult:
    capture_started = time.monotonic()
    rounds = max(1, rounds)
    max_probes = max(1, max_probes)
    for round_idx in range(1, rounds + 1):
        round_started = time.monotonic()
        log("info", f"scan round {round_idx}/{rounds}")
        ranked = await find_devices(
            device_id, name_hint, service_uuid, scan_secs, permissive_scan
        )
        log("info", f"scan round {round_idx} candidate_count={len(ranked)}")
        max_probe = min(len(ranked), max_probes)
        for idx, (score, _neg_rssi, device, _adv) in enumerate(ranked[:max_probe], start=1):
            dev_name = getattr(device, "name", "") or "<none>"
            dev_id = getattr(device, "address", "") or "<unknown>"
            log("info", f"probe {idx}/{max_probe} (round {round_idx}) id={dev_id} name={dev_name} score={score}")
            q: asyncio.Queue[bytes] = asyncio.Queue()
            probe_started = time.monotonic()
            try:
                async with BleakClient(device, timeout=6.0) as client:
                    connect_elapsed_ms = int((time.monotonic() - probe_started) * 1000)
                    log("info", f"connected id={dev_id} name={dev_name} connect_ms={connect_elapsed_ms}")
                    services = getattr(client, "services", None)
                    if services is None:
                        get_services = getattr(client, "get_services", None)
                        if callable(get_services):
                            service_started = time.monotonic()
                            services = await get_services()
                            service_elapsed_ms = int((time.monotonic() - service_started) * 1000)
                            log("debug", f"loaded services via get_services service_load_ms={service_elapsed_ms}")
                        else:
                            raise RuntimeError("bleak client has no services accessor")
                    service_count = len(list(services)) if services is not None else 0
                    char_count = sum(len(service.characteristics) for service in services)
                    log("debug", f"service graph id={dev_id} services={service_count} characteristics={char_count}")
                    notify_char = services.get_characteristic(notify_uuid)
                    write_char = services.get_characteristic(write_uuid)
                    if notify_char is None or write_char is None:
                        log("debug", "missing target characteristics, skipping")
                        continue
                    notify_props = ",".join(sorted(notify_char.properties))
                    write_props = ",".join(sorted(write_char.properties))
                    log(
                        "info",
                        f"matched GATT profile on id={dev_id} name={dev_name} "
                        f"notify_handle={notify_char.handle} notify_props={notify_props} "
                        f"write_handle={write_char.handle} write_props={write_props}",
                    )
                    log("debug", "starting notify")
                    notify_started = time.monotonic()
                    await client.start_notify(notify_uuid, lambda _h, data: q.put_nowait(bytes(data)))
                    log("debug", f"notify subscription ready subscribe_ms={int((time.monotonic() - notify_started) * 1000)}")
                    log("debug", "sending capture request")
                    request_started = time.monotonic()
                    await client.write_gatt_char(write_uuid, bytes([FRAME_CAPTURE_REQ]), response=False)
                    log("debug", f"capture request sent request_ms={int((time.monotonic() - request_started) * 1000)}")

                    deadline = asyncio.get_running_loop().time() + timeout_secs
                    expected_seq = 0
                    transfer_id = None
                    chunks = bytearray()
                    frame_count = 0
                    ack_count = 0
                    duplicate_ack_count = 0
                    nack_count = 0
                    last_chunk_log_at = 0
                    capture_wire_started = time.monotonic()

                    while True:
                        now = asyncio.get_running_loop().time()
                        if now >= deadline:
                            raise TimeoutError("camera capture timed out")
                        frame = await asyncio.wait_for(q.get(), timeout=deadline - now)
                        if not frame:
                            continue
                        frame_count += 1
                        t = frame[0]
                        if frame_count <= 10 or frame_count % 25 == 0:
                            log("debug", f"frame#{frame_count} type=0x{t:02x} len={len(frame)}")

                        if t == FRAME_CAPTURE_ACK:
                            log("debug", f"capture acknowledged after_ms={int((time.monotonic() - capture_wire_started) * 1000)}")
                            continue
                        if t == FRAME_ERROR:
                            msg = frame[1:].decode("utf-8", errors="replace")
                            raise RuntimeError(f"camera error: {msg}")
                        if t == FRAME_DONE:
                            log("debug", f"received done frame after_ms={int((time.monotonic() - capture_wire_started) * 1000)}")
                            break
                        if t != FRAME_CHUNK:
                            continue
                        if len(frame) < 15:
                            continue

                        fid = int.from_bytes(frame[1:5], "little")
                        seq = int.from_bytes(frame[5:7], "little")
                        payload_len = int.from_bytes(frame[9:11], "little")
                        payload = frame[15:]
                        if len(payload) != payload_len:
                            continue

                        if transfer_id is None:
                            transfer_id = fid
                        if fid != transfer_id:
                            continue

                        if seq == expected_seq:
                            chunks.extend(payload)
                            ack = bytes([FRAME_CHUNK_ACK]) + fid.to_bytes(4, "little") + seq.to_bytes(2, "little")
                            ack_started = time.monotonic()
                            await client.write_gatt_char(write_uuid, ack, response=False)
                            ack_elapsed_ms = int((time.monotonic() - ack_started) * 1000)
                            ack_count += 1
                            expected_seq += 1
                            chunk_delta = len(chunks) - last_chunk_log_at
                            if expected_seq <= 8 or expected_seq % 20 == 0 or chunk_delta >= 2048:
                                elapsed = max(time.monotonic() - capture_wire_started, 0.001)
                                rate = len(chunks) / elapsed
                                log(
                                    "info",
                                    f"accepted chunk seq={seq} payload_bytes={payload_len} total_bytes={len(chunks)} "
                                    f"ack_ms={ack_elapsed_ms} rate_Bps={int(rate)}",
                                )
                                last_chunk_log_at = len(chunks)
                        elif seq < expected_seq:
                            ack = bytes([FRAME_CHUNK_ACK]) + fid.to_bytes(4, "little") + seq.to_bytes(2, "little")
                            await client.write_gatt_char(write_uuid, ack, response=False)
                            duplicate_ack_count += 1
                            if duplicate_ack_count <= 5 or duplicate_ack_count % 20 == 0:
                                log("warn", f"duplicate chunk seq={seq} expected_seq={expected_seq} duplicate_acks={duplicate_ack_count}")
                        else:
                            nack = bytes([FRAME_NACK]) + fid.to_bytes(4, "little") + expected_seq.to_bytes(2, "little")
                            await client.write_gatt_char(write_uuid, nack, response=False)
                            nack_count += 1
                            log("warn", f"sequence gap expected={expected_seq} got={seq}, sent NACK")

                    await client.stop_notify(notify_uuid)
                    elapsed = max(time.monotonic() - capture_wire_started, 0.001)
                    total_elapsed = max(time.monotonic() - capture_started, 0.001)
                    log(
                        "info",
                        f"capture complete frames={frame_count} bytes={len(chunks)} "
                        f"acks={ack_count} dup_acks={duplicate_ack_count} nacks={nack_count} "
                        f"wire_ms={int(elapsed * 1000)} total_ms={int(total_elapsed * 1000)} rate_Bps={int(len(chunks) / elapsed)}",
                    )
                    if not chunks:
                        raise RuntimeError("camera capture returned empty payload")
                    return CaptureResult(bytes(chunks), dev_id, dev_name)
            except Exception as exc:
                probe_elapsed_ms = int((time.monotonic() - probe_started) * 1000)
                log("warn", f"probe failed id={dev_id} name={dev_name} elapsed_ms={probe_elapsed_ms}: {exc}")
                continue
        round_elapsed_ms = int((time.monotonic() - round_started) * 1000)
        log("warn", f"scan round {round_idx} exhausted probes={max_probe} round_ms={round_elapsed_ms}")

    raise RuntimeError("failed to find/connect camera with requested characteristics")


def run_upload(rnx_path: str, rpc: str, file_path: str, content_type: str, chunk_size: int) -> None:
    cmd = [
        rnx_path,
        "camera-upload",
        "--rpc",
        rpc,
        "--file",
        file_path,
        "--content-type",
        content_type,
        "--chunk-size",
        str(chunk_size),
    ]
    log("info", f"upload start rpc={rpc} file={file_path} chunk_size={chunk_size} content_type={content_type}")
    log("debug", f"upload command={' '.join(cmd)}")
    upload_started = time.monotonic()
    subprocess.run(cmd, check=True)
    log("info", f"upload complete upload_ms={int((time.monotonic() - upload_started) * 1000)}")


def main() -> int:
    p = argparse.ArgumentParser(description="ESP32 camera capture via bleak, optional rnx upload")
    p.add_argument("--device-id", default="", help="BLE device id/address (optional)")
    p.add_argument("--name-hint", default="LXMF", help="BLE name hint")
    p.add_argument("--service-uuid", required=True)
    p.add_argument("--write-char-uuid", required=True)
    p.add_argument("--notify-char-uuid", required=True)
    p.add_argument("--timeout-secs", type=float, default=25)
    p.add_argument("--scan-secs", type=float, default=12)
    p.add_argument("--rounds", type=int, default=2, help="number of scan+probe rounds")
    p.add_argument("--max-probes", type=int, default=20, help="max candidates probed per round")
    p.add_argument("--permissive-scan", action="store_true", help="fallback to strongest candidate if strict match not found")
    p.add_argument("--out", default="/tmp/lxmf-capture.jpg")
    p.add_argument("--upload", action="store_true")
    p.add_argument("--rnx", default="./target/debug/rnx")
    p.add_argument("--rpc", default="127.0.0.1:4243")
    p.add_argument("--content-type", default="image/jpeg")
    p.add_argument("--chunk-size", type=int, default=8192)
    p.add_argument("--log-level", choices=["debug", "info", "warn", "error"], default="info")
    args = p.parse_args()
    global CURRENT_LOG_LEVEL
    CURRENT_LOG_LEVEL = LOG_LEVELS[args.log_level]

    device_id = args.device_id.strip() or None
    log("info", f"params name_hint={args.name_hint} service={args.service_uuid} timeout={args.timeout_secs}s scan={args.scan_secs}s")
    result = asyncio.run(
        capture_over_ble(
            device_id=device_id,
            name_hint=args.name_hint,
            service_uuid=args.service_uuid,
            write_uuid=args.write_char_uuid,
            notify_uuid=args.notify_char_uuid,
            timeout_secs=args.timeout_secs,
            scan_secs=args.scan_secs,
            permissive_scan=args.permissive_scan,
            rounds=args.rounds,
            max_probes=args.max_probes,
        )
    )

    os.makedirs(os.path.dirname(args.out) or ".", exist_ok=True)
    write_started = time.monotonic()
    with open(args.out, "wb") as f:
        f.write(result.bytes_data)
    write_ms = int((time.monotonic() - write_started) * 1000)
    file_size = os.path.getsize(args.out)
    log("info", f"wrote capture file path={args.out} bytes={file_size} write_ms={write_ms}")

    print(f"BLE_CAPTURE ok: bytes={len(result.bytes_data)} device_id={result.device_id} device_name={result.device_name} out={args.out}")

    if args.upload:
        run_upload(args.rnx, args.rpc, args.out, args.content_type, args.chunk_size)

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
