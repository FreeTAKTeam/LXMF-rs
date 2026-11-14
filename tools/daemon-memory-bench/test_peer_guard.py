import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import peer_guard


class FakePeer:
    pid = 1234567
    def __init__(self, exit_code=None):
        self.returncode = exit_code
    def poll(self):
        return self.returncode
    def terminate(self):
        self.returncode = -15
    def wait(self, timeout=None):
        return self.returncode


class PeerGuardTest(unittest.TestCase):
    def test_guard_records_peer_exit_and_retains_scope_until_ack(self):
        self.check_peer(FakePeer(0), False)

    def test_guard_records_controller_cancellation(self):
        self.check_peer(FakePeer(), True)

    def check_peer(self, peer, cancel):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'experiment.json').write_text('{}')
            (root / 'sender-ack').touch()
            if cancel:
                (root / 'sender-cancel').touch()
            original = Path.read_text
            def read(path, *args, **kwargs):
                if str(path) == f'/proc/{peer.pid}/stat':
                    return f'{peer.pid} (peer) ' + ' '.join(['S'] + ['1'] * 30)
                return original(path, *args, **kwargs)
            with patch.object(peer_guard.sys, 'argv', ['peer_guard.py', str(root), 'sender', '9']), \
                 patch.object(peer_guard.os, 'getppid', return_value=9), \
                 patch.object(peer_guard.ctypes, 'CDLL') as library, \
                 patch.object(peer_guard.subprocess, 'Popen', return_value=peer), \
                 patch.object(peer_guard, 'cgroup_members', side_effect=lambda: [peer_guard.os.getpid()]), \
                 patch.object(Path, 'read_text', read):
                library.return_value.prctl.return_value = 0
                self.assertEqual(peer_guard.main(), 0)
            status = json.loads((root / 'sender-peer-result.json').read_text())
            self.assertEqual(status['exit_code'], -15 if cancel else 0)
            self.assertEqual(status['cancelled'], cancel)
            self.assertEqual(status['remaining_pids'], [])
            self.assertEqual(json.loads((root / 'sender-peer-start.json').read_text())['pid'], peer.pid)


if __name__ == '__main__':
    unittest.main()
