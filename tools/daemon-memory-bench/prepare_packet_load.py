#!/usr/bin/env python3
"""Prepare fresh signed, encrypted, stamped propagation packets before launch."""
import base64
import hashlib
import json
import os
import sys
import time
from pathlib import Path

import LXMF
import RNS
from LXMF import LXStamper
from RNS.vendor import umsgpack
from peer import payload


def stamp_for(transient_id, cost):
    workblock = LXStamper.stamp_workblock(transient_id, expand_rounds=LXStamper.WORKBLOCK_EXPAND_ROUNDS_PN)
    prefix = hashlib.sha256(workblock)
    target = 1 << (256 - cost)
    attempts = 0
    while True:
        stamp = os.urandom(LXStamper.STAMP_SIZE)
        candidate = prefix.copy()
        candidate.update(stamp)
        attempts += 1
        if int.from_bytes(candidate.digest(), 'big') <= target:
            if not LXStamper.stamp_valid(stamp, cost, workblock):
                raise RuntimeError('prepared propagation stamp failed reference validation')
            return stamp, attempts


def prepare(root):
    spec = json.loads((root / 'experiment.json').read_text())
    fixture = root / 'fixture'
    sender = RNS.Identity.from_file(str(fixture / 'sender.key'))
    recipient = RNS.Identity.from_file(str(fixture / 'receiver.key'))
    if sender is None or recipient is None:
        raise ValueError('prepared message identities unavailable')
    source = RNS.Destination(sender, RNS.Destination.OUT, RNS.Destination.SINGLE, 'lxmf', 'delivery')
    destination = RNS.Destination(recipient, RNS.Destination.OUT, RNS.Destination.SINGLE, 'lxmf', 'delivery')
    if source.hash.hex() != spec['sender_delivery_hash'] or destination.hash.hex() != spec['receiver_delivery_hash']:
        raise ValueError('prepared message identity hashes differ from the frozen experiment')
    output = root / 'prepared-packets.jsonl'
    started = time.monotonic_ns()
    cpu_started = time.process_time_ns()
    rounds = 0
    with output.open('x', buffering=1) as stream:
        for sequence in range(spec['message_count']):
            content = payload(sequence, spec['payload_bytes'], spec['seed'])
            message = LXMF.LXMessage(destination, source, content=content, desired_method=LXMF.LXMessage.PROPAGATED)
            message.defer_stamp = False
            message.pack()
            transient = message.transient_id
            stamp, attempts = stamp_for(transient, spec['propagation_stamp_target'])
            rounds += attempts
            message.propagation_stamp = stamp
            message.defer_propagation_stamp = False
            message.packed = None
            message.pack()
            packet = message.propagation_packed
            decoded = umsgpack.unpackb(packet)
            if (message.transient_id != transient or message.representation != LXMF.LXMessage.PACKET
                    or len(packet) > RNS.Link.MDU or len(decoded) != 2 or len(decoded[1]) != 1
                    or hashlib.sha256(decoded[1][0][:-LXStamper.STAMP_SIZE]).digest() != transient
                    or decoded[1][0][-LXStamper.STAMP_SIZE:] != stamp):
                raise RuntimeError('prepared packet does not match its validated envelope')
            row = {'sequence': sequence, 'sha256': hashlib.sha256(content).hexdigest(),
                   'message_hash': message.hash.hex(), 'transient_id': transient.hex(),
                   'packet_base64': base64.b64encode(packet).decode('ascii')}
            stream.write(json.dumps(row, sort_keys=True) + '\n')
    metadata = {'message_count': spec['message_count'], 'stamp_target': spec['propagation_stamp_target'],
                'started_ns': started, 'finished_ns': time.monotonic_ns(), 'hash_attempts': rounds,
                'cpu_ns': time.process_time_ns() - cpu_started,
                'packets_sha256': hashlib.sha256(output.read_bytes()).hexdigest()}
    (root / 'prepared-packets-metadata.json').write_text(json.dumps(metadata, indent=2, sort_keys=True) + '\n')
    return metadata


if __name__ == '__main__':
    if len(sys.argv) != 2:
        raise SystemExit('usage: prepare_packet_load.py RUN_DIRECTORY')
    print(json.dumps(prepare(Path(sys.argv[1])), sort_keys=True))
