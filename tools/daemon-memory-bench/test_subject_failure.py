import tempfile
import unittest
from pathlib import Path
from unittest.mock import Mock, patch

from run import SubjectFailure, capture_subject_sample


class SubjectFailureTest(unittest.TestCase):
    def test_subject_exit_during_proc_read_preserves_crash_and_oom_evidence(self):
        with tempfile.TemporaryDirectory() as directory:
            group = Path(directory)
            (group / 'memory.events').write_text('oom 1\noom_kill 1\n')
            daemon = Mock(pid=123, returncode=137)
            daemon.poll.return_value = 137
            manifest = {'collector_errors': []}
            with patch('run.sample', side_effect=FileNotFoundError(2, 'process disappeared', '/proc/123/status')):
                with self.assertRaises(SubjectFailure):
                    capture_subject_sample(group, None, False, daemon, manifest)
            self.assertTrue(manifest['subject_unexpected_exit'])
            self.assertEqual(manifest['subject_final_memory_events']['oom_kill'], 1)
            self.assertEqual(manifest['collector_errors'], [])

    def test_unrelated_collector_error_is_retained_alongside_crash(self):
        daemon = Mock(returncode=1)
        daemon.poll.return_value = 1
        manifest = {'collector_errors': []}
        with patch('run.sample', side_effect=ValueError('bad counter')):
            with self.assertRaises(SubjectFailure):
                capture_subject_sample(Path('/does-not-exist'), None, False, daemon, manifest)
        self.assertIn('ValueError: bad counter', manifest['collector_errors'])

    def test_unrelated_missing_file_is_also_a_capture_error(self):
        daemon = Mock(pid=123, returncode=1)
        daemon.poll.return_value = 1
        manifest = {'collector_errors': []}
        with patch('run.sample', side_effect=FileNotFoundError(2, 'missing', '/proc/pressure/io')):
            with self.assertRaises(SubjectFailure):
                capture_subject_sample(Path('/does-not-exist'), None, False, daemon, manifest)
        self.assertTrue(manifest['collector_errors'])


if __name__ == '__main__':
    unittest.main()
