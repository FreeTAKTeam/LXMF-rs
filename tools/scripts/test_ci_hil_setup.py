"""Exercise dependency setup without installing packages on the test host."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "tools/scripts/ci-install-hil-deps.sh"
MIRROR_SCRIPT = SCRIPT.with_name("ci-ubuntu-apt-mirror.sh")


class HilSetupTests(unittest.TestCase):
    def setup_run(self, suite, missing=(), fail=False, github_actions=False,
                  dns=("archive.ubuntu.com",), fallback_reachable=True):
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
    if sys.argv[1] == '5s':
        os.execvp(sys.argv[2], sys.argv[2:])
    with open(os.environ['CALLS'], 'a') as f:
        f.write(json.dumps(sys.argv[1:]) + '\\n')
    os.execvp(sys.argv[3], sys.argv[3:])
elif name == 'getent':
    sys.exit(0 if sys.argv[-1] in json.loads(os.environ['DNS']) else 2)
elif name == 'curl':
    sys.exit(0 if os.environ['FALLBACK_REACHABLE'] == '1' else 6)
elif name == 'apt-get':
    sys.exit(42 if os.environ['FAIL'] == '1' else 0)
"""
            for name in ["dpkg-query", "sudo", "timeout", "getent", "curl", "apt-get"]:
                tool = path / name
                tool.write_text(stub)
                tool.chmod(0o755)
            log = path / "calls.jsonl"
            mirror = path / "apt-mirrors.txt"
            mirror.write_text("http://azure.archive.ubuntu.com/ubuntu/\nhttps://security.ubuntu.com/ubuntu/\n")
            script = path / "setup.sh"
            script.write_text(SCRIPT.read_text())
            (path / MIRROR_SCRIPT.name).write_text(
                MIRROR_SCRIPT.read_text().replace("/etc/apt/apt-mirrors.txt", str(mirror)))
            env = dict(os.environ, PATH=f"{path}{os.pathsep}{os.environ['PATH']}",
                       MISSING=json.dumps(list(missing)), FAIL="1" if fail else "0", CALLS=str(log),
                       GITHUB_ACTIONS="true" if github_actions else "false",
                       DNS=json.dumps(dns), FALLBACK_REACHABLE="1" if fallback_reachable else "0")
            result = subprocess.run(["bash", str(script), suite], env=env,
                                    text=True, capture_output=True)
            calls = [json.loads(line) for line in log.read_text().splitlines()] if log.exists() else []
            return result, calls, mirror.read_text()

    def test_installed_dependencies_do_not_contact_package_servers(self):
        for suite in ["interfaces", "tools"]:
            with self.subTest(suite=suite):
                result, calls, mirror = self.setup_run(suite, github_actions=True)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(calls, [])
                self.assertIn("azure.archive.ubuntu.com", mirror)

    def test_non_media_suite_only_installs_missing_build_dependencies(self):
        result, calls, _ = self.setup_run("controller-channel", ["libdbus-1-dev", "ffmpeg"])
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
        result, calls, _ = self.setup_run("tools", media)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(calls[0][:3], ["--kill-after=10s", "120s", "apt-get"])
        self.assertEqual(calls[1][:3], ["--kill-after=10s", "120s", "apt-get"])
        self.assertEqual(calls[1][-4:], media)
        self.assertIn("--no-install-recommends", calls[1])

    def test_failed_update_stops_setup(self):
        result, calls, _ = self.setup_run("interfaces", ["libdbus-1-dev"], fail=True)
        self.assertEqual(result.returncode, 42)
        self.assertEqual(len(calls), 1)

    def test_only_github_media_setup_switches_the_slow_mirror(self):
        for suite, github_actions in [("tools", True), ("tools", False), ("interfaces", True)]:
            with self.subTest(suite=suite, github_actions=github_actions):
                result, _, mirror = self.setup_run(suite, ["libdbus-1-dev"],
                                                    github_actions=github_actions)
                self.assertEqual(result.returncode, 0, result.stderr)
                expected = ("https://archive.ubuntu.com" if suite == "tools" and github_actions
                            else "http://azure.archive.ubuntu.com")
                self.assertEqual(mirror, expected + "/ubuntu/\nhttps://security.ubuntu.com/ubuntu/\n")


    def test_dns_outage_falls_back_for_all_github_suites(self):
        for suite in ["interfaces", "tools"]:
            with self.subTest(suite=suite):
                result, calls, mirror = self.setup_run(suite, ["libdbus-1-dev"],
                                                       github_actions=True, dns=())
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(len(calls), 2)
                self.assertEqual(mirror, "https://mirrors.edge.kernel.org/ubuntu/\n")

    def test_keeps_runner_list_when_fallback_is_unavailable_or_unneeded(self):
        for actions, dns, reachable in [
            (True, (), False), (False, (), True), (True, ("azure.archive.ubuntu.com",), True),
            (True, ("security.ubuntu.com",), True),
        ]:
            with self.subTest(actions=actions, dns=dns, reachable=reachable):
                result, _, mirror = self.setup_run("tools", ["libdbus-1-dev"],
                                                    github_actions=actions, dns=dns,
                                                    fallback_reachable=reachable)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(mirror, "http://azure.archive.ubuntu.com/ubuntu/\n"
                                         "https://security.ubuntu.com/ubuntu/\n")


if __name__ == "__main__":
    unittest.main()
