#!/usr/bin/env python3
import argparse
import asyncio
import pathlib
import sys
import time

REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]
if str(REPO_ROOT) not in sys.path:
    sys.path.insert(0, str(REPO_ROOT))

try:
    from bleak import BleakClient
except Exception:
    print("error: bleak is required. install with: python3 -m pip install bleak", file=sys.stderr)
    raise

from tools.experimental.esp32_camera_capture_bleak import find_devices

FRAME_PROVISION_CMD = 0x24
FRAME_PROVISION_RESP = 0x25

LOG_LEVELS = {"debug": 10, "info": 20, "warn": 30, "error": 40}
CURRENT_LOG_LEVEL = LOG_LEVELS["info"]


def log(level: str, msg: str) -> None:
    if LOG_LEVELS[level] < CURRENT_LOG_LEVEL:
        return
    ts = time.strftime("%H:%M:%S")
    print(f"[ble-provision-{level} {ts}] {msg}", file=sys.stderr)


def build_set_command(args: argparse.Namespace) -> str:
    fields: list[str] = []
    if args.mode:
        fields.append(f"mode={args.mode}")
    if args.wifi_ssid is not None:
        fields.append(f"wifi_ssid={args.wifi_ssid}")
    if args.wifi_password is not None:
        fields.append(f"wifi_password={args.wifi_password}")
    if args.tcp_host is not None:
        fields.append(f"tcp_host={args.tcp_host}")
    if args.tcp_port is not None:
        fields.append(f"tcp_port={args.tcp_port}")
    if args.capture_profile:
        fields.append(f"capture_profile={args.capture_profile}")
    if args.ble_recovery is not None:
        fields.append(f"ble_recovery={1 if args.ble_recovery else 0}")
    if not fields:
        raise ValueError("set command requires at least one field")
    return "set\n" + "\n".join(fields)


async def run_command(args: argparse.Namespace, command_text: str) -> str:
    ranked = await find_devices(
        args.device_id.strip() or None,
        args.name_hint,
        args.service_uuid,
        args.scan_secs,
        args.permissive_scan,
    )
    max_probe = min(len(ranked), max(1, args.max_probes))
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
                notify_char = services.get_characteristic(args.notify_char_uuid)
                write_char = services.get_characteristic(args.write_char_uuid)
                if notify_char is None or write_char is None:
                    log("debug", "missing target characteristics, skipping")
                    continue

                await client.start_notify(args.notify_char_uuid, lambda _h, data: q.put_nowait(bytes(data)))
                payload = bytes([FRAME_PROVISION_CMD]) + command_text.encode("utf-8")
                log("info", f"sending provisioning command bytes={len(payload)-1}")
                await client.write_gatt_char(args.write_char_uuid, payload, response=False)

                deadline = asyncio.get_running_loop().time() + args.timeout_secs
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
                    if frame[0] != FRAME_PROVISION_RESP:
                        log("debug", f"ignoring notify type=0x{frame[0]:02x} len={len(frame)}")
                        continue
                    response = frame[1:].decode("utf-8", errors="replace")
                    await client.stop_notify(args.notify_char_uuid)
                    return response

                await client.stop_notify(args.notify_char_uuid)
                raise RuntimeError("no provisioning response received before timeout")
        except Exception as exc:
            log("warn", f"probe failed id={dev_id} name={dev_name}: {exc}")
            continue
    raise RuntimeError("failed to connect to ESP32 provisioning service over BLE")


def main() -> int:
    parser = argparse.ArgumentParser(description="Provision ESP32 node config over BLE")
    parser.add_argument("--device-id", default="", help="BLE device id/address (optional)")
    parser.add_argument("--name-hint", default="LXMF", help="BLE name hint")
    parser.add_argument("--service-uuid", required=True)
    parser.add_argument("--write-char-uuid", required=True)
    parser.add_argument("--notify-char-uuid", required=True)
    parser.add_argument("--scan-secs", type=float, default=10)
    parser.add_argument("--timeout-secs", type=float, default=8)
    parser.add_argument("--max-probes", type=int, default=10)
    parser.add_argument("--permissive-scan", action="store_true")
    parser.add_argument("--log-level", choices=["debug", "info", "warn", "error"], default="info")

    subparsers = parser.add_subparsers(dest="command", required=True)
    subparsers.add_parser("status")
    subparsers.add_parser("reboot")
    set_parser = subparsers.add_parser("set")
    set_parser.add_argument("--mode", choices=["ble_only", "tcp_client", "tcp_server"])
    set_parser.add_argument("--wifi-ssid")
    set_parser.add_argument("--wifi-password")
    set_parser.add_argument("--tcp-host")
    set_parser.add_argument("--tcp-port", type=int)
    set_parser.add_argument("--capture-profile", choices=["thumbnail", "balanced", "high", "very_high"])
    set_parser.add_argument("--ble-recovery", choices=["0", "1"])

    args = parser.parse_args()
    global CURRENT_LOG_LEVEL
    CURRENT_LOG_LEVEL = LOG_LEVELS[args.log_level]

    if args.command == "status":
        command_text = "status"
    elif args.command == "reboot":
        command_text = "reboot"
    else:
        args.ble_recovery = None if args.ble_recovery is None else args.ble_recovery == "1"
        command_text = build_set_command(args)

    response = asyncio.run(run_command(args, command_text))
    print(f"BLE_PROVISION ok: response={response}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
