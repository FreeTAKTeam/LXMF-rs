#!/usr/bin/env python3
"""Send pre-stamped propagation packets on a fixed, observed schedule."""
import base64
import ctypes
import json
import os
import signal
import sys
import threading
import time
from pathlib import Path

import LXMF
import RNS
from RNS.vendor import umsgpack
from peer import CaptureError, record_inbound_queue, wait_normal_start
from validate import expected_payload_hash


def submit_packet(link, row, record, finish, proof_timeout_seconds):
    data = base64.b64decode(row['packet_base64'], validate=True)
    receipt = RNS.Packet(link, data).send()
    submitted = receipt is not None and receipt is not False
    if submitted:
        receipt.set_timeout(proof_timeout_seconds)
    record('protocol_send', sequence=row['sequence'], packet_bytes=len(data), submitted=submitted)
    if not submitted:
        finish(row, LXMF.LXMessage.FAILED, None)
    else:
        receipt.set_delivery_callback(lambda value: finish(row, LXMF.LXMessage.SENT, value))
        receipt.set_timeout_callback(lambda value: finish(row, LXMF.LXMessage.FAILED, value))
        if receipt.status == RNS.PacketReceipt.DELIVERED:
            finish(row, LXMF.LXMessage.SENT, receipt)
        elif receipt.status in (RNS.PacketReceipt.FAILED, RNS.PacketReceipt.CULLED):
            finish(row, LXMF.LXMessage.FAILED, receipt)


def main():
    root, role, parent = Path(sys.argv[1]), sys.argv[2], int(sys.argv[3])
    if role != 'sender' or ctypes.CDLL(None, use_errno=True).prctl(1, signal.SIGTERM) != 0 or os.getppid() != parent:
        raise RuntimeError('packet sender could not bind to its guard')
    spec = json.loads((root / 'experiment.json').read_text())
    packets = [json.loads(line) for line in (root / 'prepared-packets.jsonl').read_text().splitlines()]
    if (len(packets) != spec['message_count'] or [row['sequence'] for row in packets] != list(range(spec['message_count']))
            or any(row['sha256'] != expected_payload_hash(row['sequence'], spec) for row in packets)):
        raise CaptureError('prepared packet inventory differs from the frozen schedule')
    node = RNS.Identity.from_file(str(root / 'fixture/node.key'))
    if node is None:
        raise CaptureError('propagation identity unavailable')
    node_hash = RNS.Destination.hash_from_name_and_identity('lxmf.propagation', node)
    RNS.Reticulum(configdir=str(root / 'sender/rns'), loglevel=0)
    sender_identity = RNS.Identity.from_file(str(root / 'fixture/sender.key'))
    if sender_identity is None:
        raise CaptureError('sender identity unavailable')
    router = LXMF.LXMRouter(storagepath=str(root / 'sender/store'), enforce_stamps=False)
    router.register_delivery_identity(sender_identity, display_name='Benchmark peer')
    lock = threading.RLock()
    completed, errors, closed = set(), [], [False]
    ledger = (root / 'sender.jsonl').open('a', buffering=1)

    def record(event, **values):
        with lock:
            try:
                ledger.write(json.dumps({'event': event, 'monotonic_ns': time.monotonic_ns(), **values}, sort_keys=True) + '\n')
            except (OSError, ValueError) as error:
                raise CaptureError(f'could not write sender ledger: {error}') from error

    def finish(row, state, receipt):
        with lock:
            if closed[0] or row['sequence'] in completed:
                return
            try:
                record('sender_final', sequence=row['sequence'], state=state,
                       message_hash=row['message_hash'], transient_id=row['transient_id'],
                       receipt_status=receipt.status if receipt is not None else None,
                       proof_elapsed_seconds=(receipt.concluded_at - receipt.sent_at) if receipt is not None and receipt.concluded_at is not None else None)
                completed.add(row['sequence'])
            except Exception as error:
                errors.append(f'{type(error).__name__}: {error}')

    record_inbound_queue(record, 'baseline')

    record('node_discovery_start', node=node_hash.hex())
    deadline = time.monotonic() + spec['route_deadline_seconds']
    while time.monotonic() < deadline:
        if RNS.Transport.has_path(node_hash) and RNS.Identity.recall(node_hash) is not None and RNS.Identity.recall_app_data(node_hash):
            break
        RNS.Transport.request_path(node_hash)
        time.sleep(1)
    else:
        record('node_timeout', node=node_hash.hex(), phase='route')
        raise SystemExit(71)
    app_data = umsgpack.unpackb(RNS.Identity.recall_app_data(node_hash))
    if (len(app_data) < 6 or app_data[2] is not True or app_data[3] != spec['propagation_transfer_kb']
            or app_data[4] != spec['propagation_sync_kb'] or app_data[5][0] != spec['propagation_stamp_target']
            or app_data[5][1] != spec['propagation_stamp_flexibility']):
        raise CaptureError('advertised propagation settings differ from the frozen profile')
    destination = RNS.Destination(RNS.Identity.recall(node_hash), RNS.Destination.OUT, RNS.Destination.SINGLE, 'lxmf', 'propagation')
    link = RNS.Link(destination)
    while time.monotonic() < deadline and link.status != RNS.Link.ACTIVE:
        time.sleep(0.05)
    if link.status != RNS.Link.ACTIVE:
        record('node_timeout', node=node_hash.hex(), phase='link')
        raise SystemExit(71)
    record('node_ready', node=node_hash.hex(), stamp_target=app_data[5][0], stamp_flexibility=app_data[5][1],
           transfer_kb=app_data[3], sync_kb=app_data[4])
    start = wait_normal_start(root, spec['route_deadline_seconds'] * 2 + 30)
    next_pending = start
    for row in packets:
        scheduled = start + row['sequence'] * spec['interval_seconds']
        while time.monotonic() < scheduled:
            with lock:
                if errors:
                    raise CaptureError(errors[0])
            time.sleep(min(0.01, max(0, scheduled - time.monotonic())))
        with RNS.Transport.receipts_lock:
            tracked_receipts = len(RNS.Transport.receipts)
        if tracked_receipts >= spec.get('max_generator_receipts', 900):
            record('generator_capacity', tracked_receipts=tracked_receipts)
            raise CaptureError('generator receipt tracking approached its capacity')
        record('attempted', sequence=row['sequence'], scheduled_monotonic_ns=int(scheduled * 1e9), sha256=row['sha256'])
        submit_packet(link, row, record, finish, spec['acceptance_latency_target_seconds'])
        if time.monotonic() >= next_pending:
            with lock:
                record('sender_pending', pending=row['sequence'] + 1 - len(completed), tracked_receipts=tracked_receipts)
                record_inbound_queue(record, 'periodic')
            next_pending += 5
    end = start + spec['duration_seconds']
    while time.monotonic() < end:
        with lock:
            if errors:
                raise CaptureError(errors[0])
        time.sleep(min(0.05, max(0, end - time.monotonic())))
    deadline = time.monotonic() + spec['sender_drain_seconds']
    while time.monotonic() < deadline:
        with lock:
            if errors:
                raise CaptureError(errors[0])
            if len(completed) == len(packets):
                break
        time.sleep(0.05)
    with lock:
        if errors:
            raise CaptureError(errors[0])
        record_inbound_queue(record, 'final')
        record('sender_complete', attempted=len(packets), final=len(completed))
        closed[0] = True
        marker = root / 'sender-complete.json.tmp'
        marker.write_text(json.dumps({'monotonic_ns': time.monotonic_ns()}) + '\n')
        marker.replace(root / 'sender-complete.json')


if __name__ == '__main__':
    try:
        main()
    except CaptureError as error:
        print(f'BENCH_CAPTURE_ERROR: {error}', file=sys.stderr, flush=True)
        raise SystemExit(70)
