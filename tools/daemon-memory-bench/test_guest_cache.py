import unittest
import json
import tempfile
import threading
import time
from pathlib import Path
from unittest.mock import patch

from run import request_guest_cache, validate_guest_cache


class GuestCacheTest(unittest.TestCase):
    def test_requires_different_guest_boot_and_cold_nonempty_inventory(self):
        spec = {"host_boot_id": "host", "subject_runtime_trees_sha256": {"/runtime": "hash"},
                "subject_runtime_files_sha256": {"/config": "hash"}, "rust_executable": "/rust"}
        evidence = {"guest_boot_id": "guest", "host_boot_id": "host", "guest_marker": "marker", "virtualization": "kvm",
                    "residency": {"resident_pages": 0, "skipped_symlinks": [],
                                  "roots": {path: {"files": 1, "resident_pages": 0}
                                            for path in ("/runtime", "/config", "/rust")}}}
        validate_guest_cache(evidence, spec, "guest", "marker")
        for changed in ({"guest_boot_id": "host"}, {"guest_marker": "other"}, {"host_boot_id": "other"}, {"virtualization": "docker"},
                        {"residency": {"resident_pages": 1}},
                        {"residency": {"resident_pages": 0, "roots": {}}}):
            with self.assertRaises(ValueError):
                validate_guest_cache({**evidence, **changed}, spec, "guest", "marker")

    def test_invalid_controller_response_is_preserved_before_rejection(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            control, run = root / 'control', root / 'run'
            run.mkdir()
            spec = {"host_boot_id": "host", "subject_runtime_trees_sha256": {"/runtime": "hash"},
                    "subject_runtime_files_sha256": {"/config": "hash"}, "rust_executable": "/rust"}

            def controller():
                deadline = time.monotonic() + 3
                while not (control / 'request').exists() and time.monotonic() < deadline:
                    time.sleep(0.01)
                self.assertEqual((control / 'request').read_text(), 'test-run\n')
                pending = control / 'response.tmp'
                pending.write_text(json.dumps({"control_error": "failed"}))
                pending.replace(control / 'response.json')

            thread = threading.Thread(target=controller)
            thread.start()
            try:
                real_path = Path
                def paths(value):
                    if value == '/home/ubuntu/rch-page-audit/cache-control':
                        return control
                    if value in ('/proc/sys/kernel/random/boot_id', '/etc/rch-disposable-memory-vm'):
                        path = root / ('boot' if value.startswith('/proc') else 'marker')
                        path.write_text('guest' if value.startswith('/proc') else 'marker')
                        return path
                    return real_path(value)
                with patch('run.Path', side_effect=paths):
                    with self.assertRaises(ValueError):
                        request_guest_cache('test-run', run, spec)
            finally:
                thread.join(timeout=4)
            self.assertFalse(thread.is_alive())
            self.assertEqual(json.loads((run / 'guest-cache-control.json').read_text()), {"control_error": "failed"})


if __name__ == "__main__":
    unittest.main()
