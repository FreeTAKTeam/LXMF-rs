#!/usr/bin/env python3
"""Stop one test scope if its runner disappears, including after SIGKILL."""
import os
import select
import signal
import sys
import time
from pathlib import Path


def main():
    parent = int(sys.argv[1])
    group = Path(sys.argv[2])
    if not group.name.startswith("run-p") or not group.name.endswith(".scope"):
        raise SystemExit("refusing to watch a non-test scope")
    try:
        parent_fd = os.pidfd_open(parent)
    except ProcessLookupError:
        parent_fd = None
    if os.getppid() != parent:
        parent_fd = None
    if parent_fd is not None:
        poller = select.poll()
        poller.register(parent_fd, select.POLLIN)
        while group.exists() and not poller.poll(1000):
            pass
        if not group.exists():
            return
    for sig in (signal.SIGTERM, signal.SIGKILL):
        if not group.exists():
            return
        try:
            pids = (group / "cgroup.procs").read_text().split()
        except FileNotFoundError:
            return
        for pid in pids:
            try:
                os.kill(int(pid), sig)
            except ProcessLookupError:
                pass
        time.sleep(3)


if __name__ == "__main__":
    main()
