import unittest
import hashlib
import json
import os
import py_compile
import tempfile
from pathlib import Path
from unittest.mock import patch

from report import guest_pair_errors, load_frozen_validator, process_io_delta, recovery_metrics, seconds_above_high
from validate import guest_status_matches, jsonl


class ReportTest(unittest.TestCase):
    def test_loads_only_the_selected_hash_checked_validator(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = root / 'harness' / 'validate.py'
            path.parent.mkdir()
            path.write_text('marker = "first"\n')
            spec = {'harness_sha256': {'validate.py': hashlib.sha256(path.read_bytes()).hexdigest()}}
            self.assertEqual(load_frozen_validator(root, spec).marker, 'first')
            self.assertFalse((path.parent / '__pycache__').exists())
            path.write_text('marker = "other"\n')
            with self.assertRaises(ValueError):
                load_frozen_validator(root, spec)
            spec['harness_sha256']['validate.py'] = hashlib.sha256(path.read_bytes()).hexdigest()
            self.assertEqual(load_frozen_validator(root, spec).marker, 'other')

    def test_frozen_validator_ignores_poisoned_timestamp_bytecode(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = root / 'harness' / 'validate.py'
            path.parent.mkdir()
            path.write_text('marker = "evil"\n')
            stat = path.stat()
            py_compile.compile(str(path), doraise=True)
            path.write_text('marker = "safe"\n')
            os.utime(path, ns=(stat.st_atime_ns, stat.st_mtime_ns))
            spec = {'harness_sha256': {'validate.py': hashlib.sha256(path.read_bytes()).hexdigest()}}
            self.assertEqual(load_frozen_validator(root, spec).marker, 'safe')

    def test_guest_status_requires_a_finalized_valid_capture(self):
        manifest = {"finalized": True}
        passing = {"capture_valid": True, "functional_pass": True}
        failing = {"capture_valid": True, "functional_pass": False}
        self.assertTrue(guest_status_matches("guest_script_exit=0", "runner_process_exit=0", manifest, passing))
        self.assertTrue(guest_status_matches("guest_script_exit=1", "runner_process_exit=1", manifest, failing))
        self.assertFalse(guest_status_matches("guest_script_exit=1", "runner_process_exit=0", manifest, failing))
        self.assertFalse(guest_status_matches("guest_script_exit=1", "runner_process_exit=1", manifest, passing))
        self.assertFalse(guest_status_matches("guest_script_exit=1", "runner_process_exit=1", manifest, {"capture_valid": False, "functional_pass": False}))
        self.assertFalse(guest_status_matches("guest_script_exit=1", "runner_process_exit=1", {}, failing))

    def test_process_io_delta_and_restart(self):
        def sample(pid, start, reads, writes):
            return {"processes": [{"pid": pid, "start_ticks": start,
                                   "io": f"read_bytes: {reads}\nwrite_bytes: {writes}\n"}]}

        first = sample(10, 20, 100, 200)
        last = sample(10, 20, 150, 350)
        self.assertEqual(process_io_delta([first, last], "write_bytes"), 150)
        self.assertEqual(process_io_delta([first, last], "read_bytes"), 50)
        self.assertIsNone(process_io_delta([first, sample(10, 21, 150, 350)], "write_bytes"))
        self.assertIsNone(process_io_delta([first], "write_bytes"))

    def test_duration_uses_actual_sample_intervals_and_stays_in_window(self):
        samples = [{"monotonic_ns": t * 1_000_000_000, "cgroup": {"memory.current": n}}
                   for t, n in [(0, 11), (2, 11), (5, 9), (9, 11)]]
        self.assertEqual(seconds_above_high(samples, 10_000_000_000, 10), 6)

    def test_recovery_memory_changes_use_recovery_boundaries(self):
        def sample(phase, timestamp, anon, file):
            return {'phase': phase, 'monotonic_ns': timestamp * 10**9,
                    'cgroup': {'memory.stat': {'anon': anon * 2**20, 'file': file * 2**20}}}
        samples = [sample('sender', 0, 1, 2), sample('recovery', 10, 4, 8), sample('recovery', 70, 3, 10)]
        self.assertEqual(recovery_metrics(samples, 10 * 10**9, 60),
                         {'recovery_anon_change_mib': -1, 'recovery_file_change_mib': 2})
        with self.assertRaises(ValueError):
            recovery_metrics(samples, 10 * 10**9, 61)
        with self.assertRaises(ValueError):
            recovery_metrics(samples, 11 * 10**9, 59)
        with self.assertRaises(ValueError):
            recovery_metrics([sample('recovery', 9, 9, 9), *samples], 10 * 10**9, 60)

    @patch('report.load_frozen_validator')
    def test_guest_report_requires_host_and_cache_evidence(self, load_validator):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            root, vm = base / 'experiment', base / 'vm-image'
            root.mkdir()
            (vm / 'controls').mkdir(parents=True)
            spec = {'subject_runtime_trees_sha256': {'/runtime': 'hash'}, 'subject_runtime_files_sha256': {'/config': 'hash'},
                    'rust_executable': '/rust', 'host_boot_id': 'host', 'guest_image_sha256': 'image',
                    'host_minimum_available_memory_bytes': 100, 'host_minimum_free_disk_bytes': 200,
                    'harness_sha256': {'validate.py': 'hash'}, 'fixture_sha256': {'node.key': 'hash'}}
            names = ('experiment.json', 'host-boot-id', 'guest/guest-run-frozen-profile.sh',
                     'guest/private-runtime-setup-v3.py', 'guest/guest-cold-cache-control.py',
                     'guest/cache-residency-audit-v2.py', 'harness/validate.py', 'fixture/node.key')
            inventory = {}
            for name in names:
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(name)
                inventory[name] = hashlib.sha256(path.read_bytes()).hexdigest()
            inventory_path = root / 'frozen-input-sha256.json'
            inventory_path.write_text(json.dumps(inventory))
            spec['harness_sha256']['validate.py'] = inventory['harness/validate.py']
            spec['fixture_sha256']['node.key'] = inventory['fixture/node.key']
            outcome = load_validator.return_value.verify_guest_outcome
            load_validator.return_value.jsonl.side_effect = jsonl
            inventory_hash = hashlib.sha256(inventory_path.read_bytes()).hexdigest()
            run_ids = ['guest-python', 'guest-rust']
            for run_id in run_ids:
                run, output = root / 'runs' / run_id, vm / 'output' / run_id
                run.mkdir(parents=True)
                output.mkdir(parents=True)
                cache = {'host_boot_id': 'host', 'guest_boot_id': 'boot-' + run_id, 'guest_marker': 'marker-' + run_id,
                         'virtualization': 'kvm', 'residency': {'resident_pages': 0, 'skipped_symlinks': [],
                         'roots': {p: {'files': 1, 'resident_pages': 0} for p in ('/runtime', '/config', '/rust')}}}
                (run / 'manifest.json').write_text(json.dumps({'guest_cache_control': cache, 'finalized': True}))
                (run / 'summary.json').write_text(json.dumps({'capture_valid': True, 'functional_pass': True}))
                (run / 'guest-cache-control.json').write_text(json.dumps(cache))
                (output / 'cache-control.output').write_text(json.dumps(cache))
                (vm / 'controls' / f'{run_id}.json').write_text(json.dumps({'host_boot_id': 'host', 'guest_marker': cache['guest_marker'],
                    'guest_image_sha256': 'image', 'frozen_inventory_sha256': inventory_hash}))
                (output / 'guest-status.txt').write_text('guest_script_exit=0\n')
                (output / 'runner-status.txt').write_text('runner_process_exit=0\n')
                samples = [{'run_id': run_id, 'guest_marker': cache['guest_marker'],
                            'monotonic_ns': i * 5_000_000_000, 'utc': 'time', 'qemu_pid': pid,
                            'available_memory_bytes': 1000, 'free_disk_bytes': 2000, 'swap_counters': {'pswpin': 0, 'pswpout': 0},
                            **{kind + '_pressure': 'some total=0\nfull total=0' for kind in ('cpu', 'memory', 'io')}}
                           for i, pid in enumerate((None, 123, None))]
                (vm / f'{run_id}-host-preflight.json').write_text(json.dumps(samples[0]))
                (vm / f'{run_id}-host-samples.jsonl').write_text(''.join(json.dumps(row) + '\n' for row in samples))
                (vm / f'{run_id}-host-monitor-summary.json').write_text(json.dumps({'capture_valid': True, 'coverage': 1.0,
                    'max_gap_seconds': 5.0, 'final_safety_ok': True, 'run_id': run_id, 'guest_marker': cache['guest_marker'],
                    'qemu_exit_code': 0, 'termination_reason': 'normal'}))
            self.assertEqual(guest_pair_errors(root, run_ids, spec), [])

            samples_path = vm / 'guest-python-host-samples.jsonl'
            original = samples_path.read_text()
            bad = [json.loads(line) for line in original.splitlines()]
            bad[1]['free_disk_bytes'] = 0
            samples_path.write_text(''.join(json.dumps(row) + '\n' for row in bad))
            self.assertTrue(guest_pair_errors(root, run_ids, spec))
            samples_path.unlink()
            self.assertTrue(guest_pair_errors(root, run_ids, spec))
            bad = [json.loads(line) for line in original.splitlines()]
            bad[1]['cpu_pressure'] = 'some'
            samples_path.write_text(''.join(json.dumps(row) + '\n' for row in bad))
            self.assertTrue(guest_pair_errors(root, run_ids, spec))
            samples_path.write_text(original)
            summary = vm / 'guest-python-host-monitor-summary.json'
            good_summary = summary.read_text()
            data = json.loads(good_summary)
            for key, value in (('capture_valid', 'false'), ('coverage', 0.5), ('qemu_exit_code', 1)):
                summary.write_text(json.dumps({**data, key: value}))
                self.assertTrue(guest_pair_errors(root, run_ids, spec))
            summary.write_text(good_summary)
            status = vm / 'output' / 'guest-python' / 'runner-status.txt'
            outcome.side_effect = ValueError('status mismatch')
            status.write_text('runner_process_exit=1\n')
            self.assertTrue(guest_pair_errors(root, run_ids, spec))
            outcome.side_effect = None
            status.write_text('runner_process_exit=0\n')
            control = vm / 'controls' / 'guest-python.json'
            good_control = control.read_text()
            control.write_text(json.dumps({**json.loads(good_control), 'frozen_inventory_sha256': 'wrong'}))
            self.assertTrue(guest_pair_errors(root, run_ids, spec))
            control.write_text(good_control)
            manifest = root / 'runs' / 'guest-python' / 'manifest.json'
            good_manifest = manifest.read_text()
            manifest.write_text('[]')
            self.assertTrue(guest_pair_errors(root, run_ids, spec))
            manifest.write_text(good_manifest)
            rust_run, rust_output = root / 'runs' / 'guest-rust', vm / 'output' / 'guest-rust'
            rust_cache = json.loads((rust_run / 'guest-cache-control.json').read_text())
            duplicate = {**rust_cache, 'guest_boot_id': 'boot-guest-python'}
            (rust_run / 'guest-cache-control.json').write_text(json.dumps(duplicate))
            (rust_run / 'manifest.json').write_text(json.dumps({'guest_cache_control': duplicate, 'finalized': True}))
            (rust_output / 'cache-control.output').write_text(json.dumps(duplicate))
            self.assertTrue(guest_pair_errors(root, run_ids, spec))
            (rust_run / 'guest-cache-control.json').write_text(json.dumps(rust_cache))
            (rust_run / 'manifest.json').write_text(json.dumps({'guest_cache_control': rust_cache, 'finalized': True}))
            (rust_output / 'cache-control.output').write_text(json.dumps(rust_cache))
            self.assertEqual(guest_pair_errors(root, run_ids, spec), [])
            (rust_output / 'cache-control.output').write_text('{}')
            self.assertTrue(guest_pair_errors(root, run_ids, spec))
            (rust_output / 'cache-control.output').write_text(json.dumps(rust_cache))
            helper = root / 'guest' / 'copy-frozen-inputs.py'
            helper.write_text('staging helper')
            spec['guest_staging_sha256'] = hashlib.sha256(helper.read_bytes()).hexdigest()
            inventory['guest/copy-frozen-inputs.py'] = spec['guest_staging_sha256']
            inventory_path.write_text(json.dumps(inventory))
            inventory_hash = hashlib.sha256(inventory_path.read_bytes()).hexdigest()
            staged = {name: value for name, value in inventory.items()
                      if name == 'experiment.json' or name.startswith(('fixture/', 'harness/'))}
            for run_id in run_ids:
                path = vm / 'controls' / f'{run_id}.json'
                path.write_text(json.dumps({**json.loads(path.read_text()), 'frozen_inventory_sha256': inventory_hash}))
                (vm / 'output' / run_id / 'staging-evidence.json').write_text(json.dumps({'staged_sha256': staged}))
            self.assertEqual(guest_pair_errors(root, run_ids, spec), [])
            (rust_output / 'staging-evidence.json').write_text('{}')
            self.assertTrue(guest_pair_errors(root, run_ids, spec))
            (root / 'fixture' / 'node.key').write_text('changed')
            self.assertTrue(guest_pair_errors(root, run_ids, spec))

    def test_failed_report_removes_stale_comparison(self):
        from report import failed_report
        with tempfile.TemporaryDirectory() as directory:
            out = Path(directory)
            (out / 'summary.csv').write_text('stale')
            (out / 'memory.png').write_text('stale')
            self.assertEqual(failed_report(out, ['invalid']), 1)
            self.assertFalse((out / 'summary.csv').exists())
            self.assertFalse((out / 'memory.png').exists())
            self.assertIn('No comparative memory conclusion', (out / 'report.md').read_text())


if __name__ == "__main__":
    unittest.main()
