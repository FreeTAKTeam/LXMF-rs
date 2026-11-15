#!/usr/bin/env python3
"""Keep a normal-traffic peer scope alive until final counters are captured."""
import ctypes
import json
import os
import signal
import subprocess
import sys
import time
from pathlib import Path


def cgroup_members():
    relative = Path('/proc/self/cgroup').read_text().strip().split(':', 2)[2]
    return [int(value) for value in (Path('/sys/fs/cgroup') / relative.lstrip('/') / 'cgroup.procs').read_text().split()]


def atomic_json(path, value):
    pending = path.with_suffix(path.suffix + '.tmp')
    pending.write_text(json.dumps(value, sort_keys=True) + '\n')
    pending.replace(path)


def main():
    root, role, parent = Path(sys.argv[1]), sys.argv[2], int(sys.argv[3])
    if role not in ('sender', 'receiver'):
        raise ValueError('invalid peer role')
    if ctypes.CDLL(None, use_errno=True).prctl(1, signal.SIGTERM) != 0 or os.getppid() != parent:
        raise RuntimeError('peer guard could not bind to its runner')
    spec = json.loads((root / 'experiment.json').read_text())
    program = 'packet_sender.py' if role == 'sender' and spec.get('generator_mode') == 'prepared_packets' else 'peer.py'
    peer = subprocess.Popen([sys.executable, '-u', str(Path(__file__).with_name(program)), str(root), role, str(os.getpid())])
    stat = Path(f'/proc/{peer.pid}/stat').read_text()
    start_ticks = int(stat[stat.rfind(')') + 2:].split()[19])
    atomic_json(root / f'{role}-peer-start.json', {'pid': peer.pid, 'start_ticks': start_ticks})
    cancel = root / f'{role}-cancel'
    cancelled = False
    while peer.poll() is None:
        if cancel.exists():
            cancelled = True
            peer.terminate()
            try:
                peer.wait(timeout=5)
            except subprocess.TimeoutExpired:
                peer.kill()
                peer.wait()
            break
        time.sleep(0.05)
    deadline = time.monotonic() + 15
    remaining = [pid for pid in cgroup_members() if pid != os.getpid()]
    while remaining and time.monotonic() < deadline:
        time.sleep(0.1)
        remaining = [pid for pid in cgroup_members() if pid != os.getpid()]
    atomic_json(root / f'{role}-peer-result.json', {'pid': peer.pid, 'start_ticks': start_ticks,
                'exit_code': peer.returncode, 'cancelled': cancelled, 'remaining_pids': remaining})
    ack = root / f'{role}-ack'
    while not ack.exists():
        time.sleep(0.05)
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
