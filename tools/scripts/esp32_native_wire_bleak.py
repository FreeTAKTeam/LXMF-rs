#!/usr/bin/env python3
import argparse
import pathlib
import sys

REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]
if str(REPO_ROOT) not in sys.path:
    sys.path.insert(0, str(REPO_ROOT))

from tools.scripts.esp32_native_runtime_probe_bleak import main as probe_main


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Send and observe pure 0x23 native runtime wire frames over BLE"
    )
    parser.add_argument("--device-id", default="", help="BLE device id/address (optional)")
    parser.add_argument("--name-hint", default="LXMF", help="BLE name hint")
    parser.add_argument("--service-uuid", required=True)
    parser.add_argument("--write-char-uuid", required=True)
    parser.add_argument("--notify-char-uuid", required=True)
    parser.add_argument("--scan-secs", type=float, default=10)
    parser.add_argument("--timeout-secs", type=float, default=8)
    parser.add_argument("--rounds", type=int, default=2)
    parser.add_argument("--max-probes", type=int, default=10)
    parser.add_argument("--permissive-scan", action="store_true")
    parser.add_argument("--runtime-kind", type=lambda v: int(v, 0), required=True)
    parser.add_argument("--runtime-seq", type=int, default=1)
    parser.add_argument("--runtime-payload", default="ping")
    parser.add_argument("--log-level", choices=["debug", "info", "warn", "error"], default="info")
    args = parser.parse_args()

    sys.argv = [
        "esp32_native_runtime_probe_bleak.py",
        "--device-id",
        args.device_id,
        "--name-hint",
        args.name_hint,
        "--service-uuid",
        args.service_uuid,
        "--write-char-uuid",
        args.write_char_uuid,
        "--notify-char-uuid",
        args.notify_char_uuid,
        "--scan-secs",
        str(args.scan_secs),
        "--timeout-secs",
        str(args.timeout_secs),
        "--rounds",
        str(args.rounds),
        "--max-probes",
        str(args.max_probes),
        "--send-runtime-kind",
        hex(args.runtime_kind),
        "--send-runtime-seq",
        str(args.runtime_seq),
        "--send-runtime-payload",
        args.runtime_payload,
        "--log-level",
        args.log_level,
    ]
    if args.permissive_scan:
        sys.argv.append("--permissive-scan")
    return probe_main()


if __name__ == "__main__":
    raise SystemExit(main())
