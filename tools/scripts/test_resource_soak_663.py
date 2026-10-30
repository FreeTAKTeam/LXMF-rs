"""Parser and report tests for the issue #663 resource soak harness (no daemon needed)."""
import json
import sys
import tempfile
import unittest
from types import SimpleNamespace
from unittest.mock import patch
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import resource_soak_663 as soak  # noqa: E402


class ParserTests(unittest.TestCase):
    def test_resident_capture_follows_http_metrics(self):
        order = []
        process = SimpleNamespace(poll=lambda: None)
        with tempfile.TemporaryDirectory() as directory:
            with patch.object(soak.time, "monotonic", side_effect=[0, 0, 0, 6]), \
                 patch.object(soak.time, "sleep"), \
                 patch.object(soak, "sample_process", return_value={}), \
                 patch.object(soak, "sample_cgroup", return_value={}), \
                 patch.object(soak, "fetch_metrics", side_effect=lambda _: order.append("http") or {}), \
                 patch("resource_soak_663_resident.capture_resident", side_effect=lambda *a: order.append("capture") or {}):
                soak.sample_loop(Path(directory), process, process, 123, None, 0, Path(directory),
                                 SimpleNamespace(duration_secs=5, sample_secs=5, resident_memory=True),
                                 lambda _: None, 42)
        self.assertEqual(order, ["http", "capture"])

    def test_release_binary_respects_shared_target(self):
        with patch.dict(soak.os.environ, {"CARGO_TARGET_DIR": "/tmp/shared-target"}):
            self.assertEqual(
                soak.release_binary("client", example=True),
                Path("/tmp/shared-target/release/examples/client"),
            )

    def test_daemon_socket_path_stays_short_for_nested_run_roots(self):
        command = soak.daemon_command(
            Path("/bin/reticulumd"), Path("/nested" + "/very-long-run-name" * 12),
            1234, 1235, None, None, 0,
        )
        self.assertEqual(command[command.index("--rpc-unix") + 1], "rpc.sock")

    def test_sampler_rejects_premature_process_exit(self):
        for failed in ("daemon", "poll client"):
            processes = [SimpleNamespace(poll=lambda: None), SimpleNamespace(poll=lambda: None)]
            processes[0 if failed == "daemon" else 1] = SimpleNamespace(poll=lambda: 1, returncode=1)
            with tempfile.TemporaryDirectory() as directory:
                with self.assertRaisesRegex(RuntimeError, f"{failed} exited early"):
                    soak.sample_loop(
                        Path(directory), *processes, 0, None, 0, Path(directory),
                        SimpleNamespace(duration_secs=5), lambda _: None,
                    )

    def test_kv_file(self):
        self.assertEqual(soak.parse_kv_file("anon 10\nfile 20\njunk x\n"), {"anon": 10, "file": 20})

    def test_proc_status_like_converts_kb(self):
        text = "Rss:    1024 kB\nAnonymous:  512 kB\nSwap:  0 kB\n"
        self.assertEqual(
            soak.parse_proc_status_like(text),
            {"Rss": 1024 * 1024, "Anonymous": 512 * 1024, "Swap": 0},
        )

    def test_proc_io(self):
        self.assertEqual(soak.parse_proc_io("rchar: 5\nsyscr: 7\n"), {"rchar": 5, "syscr": 7})

    def test_proc_stat_cpu_ticks_handles_parenthesised_comm(self):
        stat = "123 (tokio (rt) x) S 1 1 1 0 -1 4194560 0 0 0 0 40 2 0 0 20 0 1 0 1 1 1 1"
        self.assertEqual(soak.parse_proc_stat_cpu_ticks(stat), 42)

    def test_pressure(self):
        parsed = soak.parse_pressure("some avg10=0.10 avg60=0.20 avg300=0.30 total=5\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n")
        self.assertEqual(parsed["some"]["avg10"], 0.10)
        self.assertEqual(parsed["full"]["total"], 0.0)

    def test_cgroup_path_v2(self):
        self.assertEqual(soak.parse_cgroup_path("0::/user.slice/x.scope\n"), "/user.slice/x.scope")
        self.assertIsNone(soak.parse_cgroup_path("1:memory:/legacy\n"))

    def test_parse_size(self):
        self.assertEqual(soak.parse_size("768M"), 768 * 1024**2)
        self.assertEqual(soak.parse_size("1G"), 1024**3)
        self.assertEqual(soak.parse_size("4096"), 4096)


def sample(index, rss, high, wal, rchar, ticks, metrics=None):
    return {
        "index": index,
        "monotonic": float(index * 5),
        "unix_time": 0.0,
        "process": {"rss_bytes": rss, "anon_bytes": rss, "swap_bytes": 0, "cpu_ticks": ticks, "io": {"rchar": rchar, "syscr": index, "read_bytes": 0}},
        "cgroup": {"available": True, "memory_current": rss, "memory_swap_current": 0, "memory_stat": {"file": 1, "anon": rss}, "memory_events": {"high": high, "max": 0, "oom": 0, "oom_kill": 0}},
        "wal_bytes": wal,
        "metrics": metrics,
    }


class ReportTests(unittest.TestCase):
    def test_summary_and_report_render(self):
        samples = [sample(i, 100 * 2**20, i // 10, 500 * 2**20, i * 1000, i * 10, {"storage": {"write_lock_wait_ns_total": i, "write_ops_total": i}}) for i in range(30)]
        summary = soak.summarize_samples(samples, 5)
        self.assertEqual(summary["samples"], 30)
        self.assertEqual(summary["memory_events_delta"]["high"], 2)
        self.assertTrue(summary["metrics_available"])
        self.assertAlmostEqual(summary["rchar_per_sec"], 200.0)
        with tempfile.TemporaryDirectory() as directory:
            out = Path(directory)
            (out / "run.json").write_text(json.dumps({"label": "unit", "profile": "idle", "duration_secs": 150, "sample_secs": 5, "constrained": True, "fixture": {}}))
            (out / "samples.jsonl").write_text("\n".join(json.dumps(s) for s in samples) + "\n")
            (out / "client.jsonl").write_text(json.dumps({"window": {"polls": 10, "errors": 1, "events": 0, "latency_us": {"p50": 1, "p95": 9, "max": 20}}, "total": {"polls": 10, "errors": 1, "events": 0, "errors_by_code": {"X": 1}}}) + "\n")
            report = soak.render_report(out)
            self.assertIn("fail (+2)", report)
            self.assertIn("1 (10.0000%)", report)
            self.assertIn("not met (145 s measured)", report)
            self.assertTrue((out / "summary.json").exists())

    def test_requested_duration_cannot_certify_an_interrupted_soak(self):
        summary = soak.summarize_samples([sample(i, 100, 0, 0, 0, 0) for i in range(3)], 5)
        rows = soak.acceptance_rows(
            summary, {}, {"duration_secs": 14400, "completed": False},
        )
        self.assertIn("| multi-hour duration | not met (10 s measured) |", rows)

    def test_report_with_too_few_samples(self):
        with tempfile.TemporaryDirectory() as directory:
            out = Path(directory)
            (out / "run.json").write_text("{}")
            report = soak.render_report(out)
            self.assertIn("No usable samples", report)


if __name__ == "__main__":
    unittest.main()
