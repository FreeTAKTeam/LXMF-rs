"""Exercise dependency setup without installing packages on the test host."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "tools/scripts/ci-install-hil-deps.sh"


class HilSetupTests(unittest.TestCase):
    def setup_run(self, suite, missing=(), fail=False):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp)
            stub = """#!/usr/bin/env python3
import json, os, pathlib, sys
name = pathlib.Path(sys.argv[0]).name
if name == 'dpkg-query':
    if sys.argv[-1] in json.loads(os.environ['MISSING']):
        sys.exit(1)
    print('install ok installed')
elif name == 'sudo':
    os.execvp(sys.argv[1], sys.argv[1:])
elif name == 'timeout':
    with open(os.environ['CALLS'], 'a') as f:
        f.write(json.dumps(sys.argv[1:]) + '\\n')
    os.execvp(sys.argv[3], sys.argv[3:])
elif name == 'apt-get':
    sys.exit(42 if os.environ['FAIL'] == '1' else 0)
"""
            for name in ["dpkg-query", "sudo", "timeout", "apt-get"]:
                tool = path / name
                tool.write_text(stub)
                tool.chmod(0o755)
            log = path / "calls.jsonl"
            env = dict(os.environ, PATH=f"{path}{os.pathsep}{os.environ['PATH']}",
                       MISSING=json.dumps(list(missing)), FAIL="1" if fail else "0", CALLS=str(log))
            result = subprocess.run(["bash", str(SCRIPT), suite], env=env,
                                    text=True, capture_output=True)
            calls = [json.loads(line) for line in log.read_text().splitlines()] if log.exists() else []
            return result, calls

    def test_installed_dependencies_do_not_contact_package_servers(self):
        result, calls = self.setup_run("interfaces")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(calls, [])

    def test_non_media_suite_only_installs_missing_build_dependencies(self):
        result, calls = self.setup_run("controller-channel", ["libdbus-1-dev", "ffmpeg"])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(len(calls), 2)
        self.assertEqual(calls[1][-1], "libdbus-1-dev")
        self.assertNotIn("ffmpeg", calls[1])
        for call in calls:
            self.assertEqual(call[:3], ["--kill-after=10s", "120s", "apt-get"])
            for option in ["Acquire::Retries=2", "Acquire::http::Timeout=20",
                           "Acquire::https::Timeout=20", "DPkg::Lock::Timeout=30"]:
                self.assertIn(option, call)

    def test_tools_suite_retains_all_media_dependencies(self):
        media = ["ffmpeg", "imagemagick", "graphicsmagick", "squashfs-tools"]
        result, calls = self.setup_run("tools", media)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(calls[1][-4:], media)

    def test_failed_update_stops_setup(self):
        result, calls = self.setup_run("interfaces", ["libdbus-1-dev"], fail=True)
        self.assertEqual(result.returncode, 42)
        self.assertEqual(len(calls), 1)


if __name__ == "__main__":
    unittest.main()
