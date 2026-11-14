#!/usr/bin/env python3
"""Start a subject in its final cgroup and restore its disposable identity."""
import ctypes
import os
import shutil
import signal
import sys


def main():
    expected_parent, fixture, identity, *command = sys.argv[1:]
    if ctypes.CDLL(None, use_errno=True).prctl(1, signal.SIGTERM) != 0:
        raise OSError(ctypes.get_errno(), "failed to register parent-death signal")
    if os.getppid() != int(expected_parent):
        raise RuntimeError("runner exited before subject startup")
    shutil.copyfile(fixture, identity)
    os.chmod(identity, 0o600)
    os.execvp(command[0], command)


if __name__ == "__main__":
    main()
