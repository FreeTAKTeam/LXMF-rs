import errno
import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import run


class GeneratorSampleTest(unittest.TestCase):
    def test_vanished_worker_is_recorded_but_live_worker_error_is_not_hidden(self):
        with tempfile.TemporaryDirectory() as directory:
            scope = Path(directory)
            (scope / "memory.current").write_text("1")
            (scope / "memory.events").write_text("oom 0\noom_kill 0\n")
            (scope / "cpu.stat").write_text("usage_usec 1\nnr_throttled 0\n")
            missing = 99999999
            (scope / "cgroup.procs").write_text(str(missing))
            with patch.object(run, "group_for", return_value=scope), \
                 patch.object(run, "process_stats", side_effect=OSError(errno.ESRCH, "vanished")):
                result = run.sample_generator(os.getpid(), scope / "subject", True)
                self.assertEqual(result["disappeared_pids"], [missing])
                (scope / "cgroup.procs").write_text(str(os.getpid()))
                with self.assertRaises(OSError):
                    run.sample_generator(os.getpid(), scope / "subject", True)


if __name__ == "__main__":
    unittest.main()
