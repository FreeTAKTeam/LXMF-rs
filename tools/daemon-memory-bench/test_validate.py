import json
import hashlib
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import validate as validator_module
from validate import expected_payload_hash, load_inputs, validate


class ValidationTest(unittest.TestCase):
    def test_prepared_packet_inventory_is_checked_before_validation(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            spec = {'generator_mode': 'prepared_packets', 'message_count': 1, 'propagation_stamp_target': 16,
                    'payload_bytes': 64, 'seed': 'test'}
            packet = {'sequence': 0, 'sha256': expected_payload_hash(0, spec), 'message_hash': 'ab'*32,
                      'transient_id': 'cd'*32, 'packet_base64': 'YQ=='}
            content = json.dumps(packet) + '\n'
            metadata = {'message_count': 1, 'stamp_target': 16, 'started_ns': 1, 'finished_ns': 2,
                        'packets_sha256': hashlib.sha256(content.encode()).hexdigest()}
            manifest = {'sender_started': False, 'receiver_started': False, 'packet_preparation': metadata,
                        'phase_times_ns': {'launch_requested': 3}}
            (root / 'experiment.json').write_text(json.dumps(spec))
            (root / 'prepared-packets.jsonl').write_text(content)
            (root / 'prepared-packets-metadata.json').write_text(json.dumps(metadata))
            (root / 'samples.jsonl').write_text('')
            self.assertEqual(load_inputs(root, manifest)[-1]['sender'], [])
            (root / 'prepared-packets.jsonl').write_text(content.replace('"sequence": 0', '"sequence": 1'))
            self.assertTrue(load_inputs(root, manifest)[-1]['sender'])

    def test_guest_outcome_requires_fresh_matching_evidence(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            spec = {"profile": "normal"}
            manifest = {"finalized": True}
            result = {"capture_valid": True, "functional_pass": False}
            for name, value in (("experiment.json", spec), ("manifest.json", manifest), ("summary.json", result)):
                (root / name).write_text(json.dumps(value))
            frozen = (root / "experiment.json").read_bytes()
            with patch.object(validator_module, "load_inputs", return_value=([], [], [], {})), \
                 patch.object(validator_module, "validate", return_value=result) as fresh:
                self.assertEqual(validator_module.verify_guest_outcome(root, spec, "guest_script_exit=1", "runner_process_exit=1", frozen), result)
                with self.assertRaises(ValueError):
                    validator_module.verify_guest_outcome(root, spec, "guest_script_exit=1", "runner_process_exit=0", frozen)
                (root / "experiment.json").write_bytes(frozen + b"\n")
                with self.assertRaises(ValueError):
                    validator_module.verify_guest_outcome(root, spec, "guest_script_exit=1", "runner_process_exit=1", frozen)
                (root / "experiment.json").write_bytes(frozen)
                fresh.return_value = {"capture_valid": False, "functional_pass": False}
                with self.assertRaises(ValueError):
                    validator_module.verify_guest_outcome(root, spec, "guest_script_exit=1", "runner_process_exit=1", frozen)
                (root / "summary.json").unlink()
                with self.assertRaises(FileNotFoundError):
                    validator_module.verify_guest_outcome(root, spec, "guest_script_exit=1", "runner_process_exit=1", frozen)

    def setUp(self):
        self.spec = {"message_count": 1, "sample_interval_seconds": 5,
                     "maximum_schedule_lateness_seconds": 2,
                     "interval_seconds": 30, "payload_bytes": 256, "seed": "test",
                     "sender_delivery_hash": "sender", "receiver_delivery_hash": "receiver"}
        self.manifest = {"isolation_verified": True, "limits_verified": True, "finalized": True,
                         "sampling_start_ns": 0, "sampling_end_ns": 10_000_000_000,
                         "sender_exit": 0, "receiver_exit": 0}
        events = {"oom": 0, "oom_kill": 0}
        self.samples = [{"monotonic_ns": t, "cgroup": {"memory.events": events}}
                        for t in (0, 5_000_000_000, 10_000_000_000)]
        payload_hash = expected_payload_hash(0, self.spec)
        self.sender = [{"event": "attempted", "sequence": 0, "sha256": payload_hash,
                        "monotonic_ns": 1, "scheduled_monotonic_ns": 0},
                       {"event": "sender_final", "sequence": 0, "state": 4, "message_hash": "message", "monotonic_ns": 2},
                       {"event": "sender_complete", "attempted": 1, "final": 1, "monotonic_ns": 3}]
        self.receiver = [{"event": "received", "sha256": payload_hash, "message_hash": "message",
                          "source": "sender", "destination": "receiver", "monotonic_ns": 4},
                         {"event": "receiver_complete", "count": 1, "quiet_interval_completed": True, "sync_state": 7, "monotonic_ns": 5}]

    def result(self):
        return validate(self.spec, self.manifest, self.samples, self.sender, self.receiver)

    def test_guest_outcome_revalidates_real_failed_delivery_evidence(self):
        self.receiver[0]["source"] = "wrong"
        result = self.result()
        self.assertTrue(result["capture_valid"])
        self.assertFalse(result["functional_pass"])
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name, value in (("experiment.json", self.spec), ("manifest.json", self.manifest), ("summary.json", result)):
                (root / name).write_text(json.dumps(value))
            for name, rows in (("samples", self.samples), ("sender", self.sender), ("receiver", self.receiver)):
                (root / f"{name}.jsonl").write_text(''.join(json.dumps(row) + '\n' for row in rows))
            frozen = (root / "experiment.json").read_bytes()
            self.assertEqual(validator_module.verify_guest_outcome(root, self.spec, "guest_script_exit=1", "runner_process_exit=1", frozen), result)
            (root / "summary.json").write_text(json.dumps({**result, "received": True}))
            with self.assertRaises(ValueError):
                validator_module.verify_guest_outcome(root, self.spec, "guest_script_exit=1", "runner_process_exit=1", frozen)
            (root / "summary.json").write_text(json.dumps(result))
            receiver = root / "receiver.jsonl"
            receiver.write_text(receiver.read_text().replace('"wrong"', '"sender"'))
            with self.assertRaises(ValueError):
                validator_module.verify_guest_outcome(root, self.spec, "guest_script_exit=1", "runner_process_exit=1", frozen)

    def test_good_run(self):
        self.assertTrue(self.result()["capture_valid"])
        self.assertTrue(self.result()["functional_pass"])

    def test_missing_sample_is_detected(self):
        self.samples.pop(1)
        self.samples.insert(1, {**self.samples[0], "monotonic_ns": 200_000_000})
        self.assertFalse(self.result()["capture_valid"])

    def test_unmatched_delivery_is_detected(self):
        self.receiver[0]["sha256"] = "different"
        self.assertFalse(self.result()["functional_pass"])

    def test_wrong_sender_is_not_a_valid_delivery(self):
        self.receiver[0]["source"] = "other"
        result = self.result()
        self.assertEqual(result["missing"], 1)
        self.assertEqual(result["duplicate_or_unexpected"], 1)

    def test_swapped_protocol_hashes_do_not_match_payloads(self):
        self.spec["message_count"] = 2
        second_hash = expected_payload_hash(1, self.spec)
        self.sender.insert(1, {"event": "attempted", "sequence": 1, "sha256": second_hash,
                               "monotonic_ns": 30_000_000_001, "scheduled_monotonic_ns": 30_000_000_000})
        self.sender[2]["monotonic_ns"] = 30_000_000_002
        self.sender.insert(3, {"event": "sender_final", "sequence": 1, "state": 4,
                               "message_hash": "other", "monotonic_ns": 30_000_000_003})
        self.sender[-1].update(attempted=2, final=2, monotonic_ns=30_000_000_004)
        self.receiver[0].update(message_hash="other", monotonic_ns=30_000_000_005)
        self.receiver.insert(1, {"event": "received", "sha256": second_hash, "message_hash": "message",
                                 "source": "sender", "destination": "receiver", "monotonic_ns": 30_000_000_006})
        self.receiver[-1].update(count=2, monotonic_ns=30_000_000_007)
        result = self.result()
        self.assertFalse(result["functional_pass"])
        self.assertIn("receiver payload and protocol message hash pairing differs from the sender", result["functional_errors"])

    def test_nonfinite_or_reversed_timestamps_invalidate_capture(self):
        self.sender[0]["scheduled_monotonic_ns"] = float('nan')
        self.assertFalse(self.result()["capture_valid"])
        self.sender[0]["scheduled_monotonic_ns"] = 2
        self.assertFalse(self.result()["capture_valid"])

    def test_delivery_cannot_precede_the_corresponding_attempt(self):
        self.sender[0]["monotonic_ns"] = 50
        self.sender[1]["monotonic_ns"] = 60
        self.sender[2]["monotonic_ns"] = 70
        result = self.result()
        self.assertFalse(result["capture_valid"])
        self.assertIn("receiver delivery preceded its sender attempt", result["capture_errors"])

    def test_never_attempted_messages_are_in_expected_delivery_count(self):
        self.spec["message_count"] = 2
        result = self.result()
        self.assertEqual(result["planned"], 2)
        self.assertEqual(result["never_attempted"], 1)
        self.assertEqual(result["missing"], 1)
        self.assertFalse(result["functional_pass"])

    def test_nonzero_exit_and_early_oom_are_detected(self):
        self.manifest["sender_exit"] = 1
        self.samples[0]["cgroup"]["memory.events"]["oom"] = 1
        self.assertFalse(self.result()["functional_pass"])

    def test_frozen_payload_is_checked(self):
        self.sender[0]["sha256"] = "different"
        self.assertFalse(self.result()["functional_pass"])

    def test_latency_target_is_checked(self):
        self.spec["acceptance_latency_target_seconds"] = 1
        self.sender[1]["monotonic_ns"] = 2_000_000_002
        self.assertFalse(self.result()["functional_pass"])

    def test_log_error_is_not_mislabeled_as_a_crash(self):
        self.manifest["subject_log_error_count"] = 1
        result = self.result()
        self.assertFalse(result["functional_pass"])
        self.assertTrue(any("subject log" in error for error in result["functional_errors"]))
        self.assertFalse(any("exited unexpectedly" in error for error in result["functional_errors"]))

    def test_receiver_ledger_failure_is_capture_invalid_not_missing_delivery(self):
        self.manifest["receiver_exit"] = 70
        self.receiver.clear()
        result = self.result()
        self.assertFalse(result["capture_valid"])
        self.assertFalse(result["delivery_evaluated"])
        self.assertFalse(result["functional_pass"])
        self.assertTrue(any("receiver" in error and "capture" in error for error in result["capture_errors"]))
        self.assertFalse(any("receiver" in error for error in result["functional_errors"]))

    def test_sender_completion_counter_disagreement_invalidates_capture(self):
        self.sender[-1]["attempted"] = 2
        result = self.result()
        self.assertFalse(result["capture_valid"])
        self.assertFalse(result["delivery_evaluated"])
        self.assertIsNone(result["never_attempted"])
        self.assertTrue(any("completion counters" in error for error in result["capture_errors"]))

    def test_receiver_completion_counter_uses_only_preceding_deliveries(self):
        self.receiver[-1]["count"] = 2
        result = self.result()
        self.assertFalse(result["capture_valid"])
        self.receiver[-1]["count"] = 1
        self.receiver.append({**self.receiver[0], "monotonic_ns": 6})
        result = self.result()
        self.assertTrue(result["capture_valid"])
        self.assertIn("receiver recorded a delivery after its quiet completion", result["functional_errors"])

    def test_multiple_receiver_completion_records_invalidate_capture(self):
        self.receiver.append(dict(self.receiver[-1]))
        result = self.result()
        self.assertFalse(result["capture_valid"])
        self.assertFalse(result["delivery_evaluated"])

    def test_malformed_record_returns_a_capture_error(self):
        self.receiver[0] = {"event": "received"}
        result = self.result()
        self.assertFalse(result["capture_valid"])
        self.assertFalse(result["delivery_evaluated"])
        self.assertEqual(result["received"], 1)
        self.assertTrue(any("could not interpret evidence" in error for error in result["capture_errors"]))

    def test_malformed_nested_sample_returns_a_capture_error(self):
        self.samples[0] = {"monotonic_ns": 0, "cgroup": {"memory.events": None}}
        result = self.result()
        self.assertFalse(result["capture_valid"])
        self.assertTrue(any("AttributeError" in error for error in result["capture_errors"]))

    def test_malformed_evidence_preserves_known_crash_and_oom(self):
        self.receiver[0] = {"event": "received"}
        self.manifest.update(subject_unexpected_exit=True, subject_final_memory_events={"oom": 1})
        result = self.result()
        self.assertFalse(result["capture_valid"])
        self.assertEqual(result["oom_at_exit"], "recorded")
        self.assertIn("subject exited unexpectedly", result["functional_errors"])
        self.assertIn("subject cgroup recorded an OOM event", result["functional_errors"])

    def test_receiver_completion_requires_a_boolean_quiet_flag(self):
        self.receiver[-1]["quiet_interval_completed"] = "false"
        self.assertFalse(self.result()["capture_valid"])

    def test_crash_after_peer_completion_preserves_delivery_evidence(self):
        self.manifest["subject_unexpected_exit"] = True
        result = self.result()
        self.assertTrue(result["delivery_evaluated"])
        self.assertEqual(result["missing"], 0)
        self.assertEqual(result["never_attempted"], 0)
        self.assertIn("subject exited unexpectedly", result["functional_errors"])

    def test_sender_ledger_failure_does_not_claim_incomplete_work(self):
        self.manifest["sender_exit"] = 70
        self.sender.clear()
        self.receiver.clear()
        self.manifest["receiver_exit"] = 0
        result = self.result()
        self.assertFalse(result["capture_valid"])
        self.assertFalse(result["delivery_evaluated"])
        self.assertFalse(result["functional_errors"])
        self.assertIsNone(result["never_attempted"])

    def test_cli_preserves_complete_records_and_reports_partial_ledger(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.manifest["receiver_exit"] = 70
            (root / "experiment.json").write_text(json.dumps(self.spec))
            (root / "manifest.json").write_text(json.dumps(self.manifest))
            (root / "samples.jsonl").write_text(''.join(json.dumps(x) + '\n' for x in self.samples))
            (root / "sender.jsonl").write_text(''.join(json.dumps(x) + '\n' for x in self.sender))
            (root / "receiver.jsonl").write_text(json.dumps(self.receiver[0]) + '\n{"event":')
            completed = subprocess.run([sys.executable, str(Path(__file__).with_name('validate.py')), str(root)],
                                       text=True, capture_output=True)
            self.assertEqual(completed.returncode, 1, completed.stderr)
            result = json.loads((root / "summary.json").read_text())
            self.assertEqual(result["received"], 1)
            self.assertFalse(result["capture_valid"])
            self.assertFalse(result["delivery_evaluated"])
            self.assertTrue(any('receiver.jsonl:2' in error for error in result["capture_errors"]))

    def test_subject_crash_before_peer_start_is_a_functional_result(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.manifest.update(subject_unexpected_exit=True, sender_started=False, receiver_started=False)
            self.manifest.pop("sender_exit")
            self.manifest.pop("receiver_exit")
            (root / "experiment.json").write_text(json.dumps(self.spec))
            (root / "manifest.json").write_text(json.dumps(self.manifest))
            (root / "samples.jsonl").write_text(''.join(json.dumps(x) + '\n' for x in self.samples))
            completed = subprocess.run([sys.executable, str(Path(__file__).with_name('validate.py')), str(root)],
                                       text=True, capture_output=True)
            self.assertEqual(completed.returncode, 1, completed.stderr)
            result = json.loads((root / "summary.json").read_text())
            self.assertTrue(result["capture_valid"])
            self.assertFalse(result["delivery_evaluated"])
            self.assertEqual(result["functional_errors"], ["subject exited unexpectedly"])

    def test_subject_crash_still_requires_completed_peer_ledger(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.manifest.update(subject_unexpected_exit=True, sender_started=True, sender_exit=0,
                                 receiver_started=False)
            self.manifest.pop("receiver_exit")
            (root / "samples.jsonl").write_text(''.join(json.dumps(x) + '\n' for x in self.samples))
            samples, sender, receiver, errors = load_inputs(root, self.manifest)
            result = validate(self.spec, self.manifest, samples, sender, receiver, errors)
            self.assertFalse(result["capture_valid"])
            self.assertTrue(any("sender.jsonl" in error for error in result["capture_errors"]))
            self.assertFalse(any("receiver.jsonl" in error for error in result["capture_errors"]))
            self.assertIn("subject exited unexpectedly", result["functional_errors"])

    def test_idle_requires_full_window_and_no_loopback_traffic(self):
        self.spec.update(profile="preflight-only-idle-dry-run", message_count=0, duration_seconds=10)
        self.manifest.update(sender_started=False, receiver_started=False, phase_times_ns={"idle": 0})
        self.manifest.pop("sender_exit")
        self.manifest.pop("receiver_exit")
        network = 'lo: 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0'
        for row in self.samples:
            row.update(phase="idle", namespace_network=network)
        result = validate(self.spec, self.manifest, self.samples, [], [])
        self.assertTrue(result["capture_valid"])
        self.assertTrue(result["functional_pass"])
        self.assertFalse(result["delivery_evaluated"])
        self.samples[-1]["namespace_network"] = 'lo: 1 0 0 0 0 0 0 0 1 0 0 0 0 0 0 0'
        self.assertFalse(validate(self.spec, self.manifest, self.samples, [], [])["functional_pass"])
        self.samples.pop()
        result = validate(self.spec, self.manifest, self.samples, [], [])
        self.assertFalse(result["capture_valid"])
        self.assertFalse(result["functional_pass"])

    def test_idle_requires_initial_boundary_and_rejects_any_peer_record(self):
        self.spec.update(profile="preflight-only-idle-dry-run", message_count=0, duration_seconds=10)
        self.manifest.update(sender_started=False, receiver_started=False, phase_times_ns={"idle": 0})
        self.manifest.pop("sender_exit")
        self.manifest.pop("receiver_exit")
        for row in self.samples:
            row.update(phase="idle", namespace_network='lo: 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0')
        result = validate(self.spec, self.manifest, self.samples[1:], [], [])
        self.assertFalse(result["capture_valid"])
        self.assertFalse(result["functional_pass"])
        result = validate(self.spec, self.manifest, self.samples, [{"event": "node_ready"}], [])
        self.assertFalse(result["functional_pass"])

    def test_recovery_requires_both_boundaries_and_full_duration(self):
        self.spec["recovery_seconds"] = 10
        self.manifest["phase_times_ns"] = {"recovery": 10_000_000_000}
        self.manifest["sampling_end_ns"] = 20_000_000_000
        for row in self.samples:
            row["phase"] = "sender"
        events = {"oom": 0, "oom_kill": 0}
        self.samples[-1]["phase"] = "recovery"
        self.samples.extend({"monotonic_ns": t, "phase": "recovery", "cgroup": {"memory.events": events}}
                            for t in (15_000_000_000, 20_000_000_000))
        for row in self.samples[2:]:
            row.update(ledger_counts={"accepted_minus_received": 0}, storage={"subject_file_bytes": 1})
        self.assertTrue(self.result()["functional_pass"])
        self.samples[2]["phase"] = "receiver"
        self.assertFalse(self.result()["capture_valid"])
        self.assertFalse(self.result()["functional_pass"])
        self.samples[2]["phase"] = "recovery"
        self.samples.pop()
        self.assertFalse(self.result()["capture_valid"])
        self.assertFalse(self.result()["functional_pass"])

    def test_offline_sender_must_observe_whole_offered_window(self):
        self.spec.update(profile="preflight-only-offline-recovery-dry-run", duration_seconds=10,
                         recovery_seconds=10)
        self.sender[-1]["monotonic_ns"] = 2_000_000_000
        self.manifest["phase_times_ns"] = {"sender_end": 10_000_000_000, "recovery": 10_000_000_000}
        self.manifest["sampling_end_ns"] = 20_000_000_000
        self.samples[-1].update(phase="sender")
        events = {"oom": 0, "oom_kill": 0}
        self.samples.extend({"monotonic_ns": t, "phase": "recovery", "cgroup": {"memory.events": events},
                             "ledger_counts": {"accepted_minus_received": 0}, "storage": {"subject_file_bytes": 1}}
                            for t in (10_000_000_000, 15_000_000_000, 20_000_000_000))
        self.assertFalse(self.result()["capture_valid"])
        self.sender[-1]["monotonic_ns"] = 10_000_000_000
        self.assertTrue(self.result()["capture_valid"])

    def test_subject_crash_during_offline_sender_is_a_functional_failure(self):
        self.spec.update(profile="preflight-only-offline-recovery-dry-run", duration_seconds=10,
                         recovery_seconds=10)
        self.manifest.update(subject_unexpected_exit=True, sender_started=True, receiver_started=False, sender_exit=None)
        self.manifest.pop("receiver_exit")
        self.sender[:] = self.sender[:1]
        self.receiver.clear()
        result = self.result()
        self.assertTrue(result["capture_valid"])
        self.assertFalse(result["functional_pass"])
        self.assertIn("subject exited unexpectedly", result["functional_errors"])
        self.assertEqual(result["oom_at_exit"], "unknown")
        self.manifest["subject_final_memory_events"] = {"oom": 1, "oom_kill": 1}
        result = self.result()
        self.assertEqual(result["oom_at_exit"], "recorded")
        self.assertIn("subject cgroup recorded an OOM event", result["functional_errors"])

    def test_generator_scope_coverage_and_exit_boundary(self):
        self.manifest.update(generator_telemetry_contract=1, sender_started=True, receiver_started=False,
                             sender_scope_verified_ns=0, sender_exit_observed_ns=10_000_000_000)
        for row in self.samples:
            row.update(sample_kind="periodic", phase="sender", generator_active_at_start=True,
                       generator_active_at_end=True,
                       generator={"memory_current": 100, "memory_events": {"oom": 0, "oom_kill": 0},
                                  "cpu_stat": {"usage_usec": 1, "nr_throttled": 0}})
        self.assertTrue(self.result()["capture_valid"])
        self.samples[-1].pop("generator")
        self.assertFalse(self.result()["capture_valid"])
        self.samples[-1]["generator_active_at_end"] = False
        self.assertTrue(self.result()["capture_valid"])
        self.samples[1]["generator"]["cpu_stat"]["nr_throttled"] = 1
        self.assertFalse(self.result()["capture_valid"])

    def test_generator_telemetry_preserves_receiver_completion_validation(self):
        self.manifest.update(generator_telemetry_contract=1, sender_started=True, receiver_started=True,
                             sender_scope_verified_ns=0, sender_exit_observed_ns=10_000_000_000,
                             receiver_scope_verified_ns=10_000_000_000, receiver_exit_observed_ns=16_000_000_000,
                             sampling_end_ns=15_000_000_000)
        for row in self.samples:
            row.update(sample_kind="periodic", phase="sender", generator_active_at_start=True,
                       generator_active_at_end=True,
                       generator={"memory_current": 100, "memory_events": {"oom": 0, "oom_kill": 0},
                                  "cpu_stat": {"usage_usec": 1, "nr_throttled": 0}})
        self.samples.append({**self.samples[-1], "monotonic_ns": 15_000_000_000, "phase": "receiver"})
        result = self.result()
        self.assertTrue(result["capture_valid"])
        self.assertTrue(result["functional_pass"])

    def test_generator_long_gap_fails_even_with_99_percent_coverage(self):
        self.manifest.update(generator_telemetry_contract=1, sender_started=True, receiver_started=False,
                             sender_scope_verified_ns=0, sender_exit_observed_ns=7_200_000_000_000,
                             sampling_end_ns=7_200_000_000_000)
        events = {"oom": 0, "oom_kill": 0}
        self.samples = [{"monotonic_ns": t * 1_000_000_000, "cgroup": {"memory.events": events},
                         "sample_kind": "periodic", "phase": "sender", "generator_active_at_start": True,
                         "generator_active_at_end": True,
                         **({} if 50 <= t <= 85 else {"generator": {"memory_current": 100,
                            "memory_events": events, "cpu_stat": {"usage_usec": 1, "nr_throttled": 0}}})}
                        for t in range(0, 7201, 5)]
        result = self.result()
        self.assertFalse(result["capture_valid"])
        self.assertTrue(any("gap over 30 seconds" in error for error in result["capture_errors"]))
        self.manifest.update(subject_unexpected_exit=True)
        self.manifest.pop("sender_exit_observed_ns")
        result = self.result()
        self.assertFalse(result["capture_valid"])
        self.assertIn("subject exited unexpectedly", result["functional_errors"])

    def test_normal_peers_are_monitored_and_polled_on_the_shared_schedule(self):
        second = 1_000_000_000
        self.spec.update(profile="preflight-only-normal-dry-run", duration_seconds=10, recovery_seconds=10,
                         receiver_poll_interval_seconds=5, normal_delivery_target_seconds=5,
                         max_guest_cpu_psi_some_fraction=0.10)
        self.manifest.update(generator_telemetry_contract=2, cgroup="subject", sender_started=True, receiver_started=True,
                             sender_cgroup="sender-scope", receiver_cgroup="receiver-scope", sender_pid=11, receiver_pid=12,
                             sender_start_ticks=101, receiver_start_ticks=102, sender_scope_verified_ns=0,
                             receiver_scope_verified_ns=0, sender_exit_observed_ns=25*second,
                             receiver_exit_observed_ns=25*second, sampling_end_ns=35*second,
                             phase_times_ns={"normal": 10*second, "normal_end": 25*second, "recovery": 25*second})
        events = {"oom": 0, "oom_kill": 0}
        for role in ("sender", "receiver"):
            self.manifest[role + "_final_memory_events"] = dict(events)
            self.manifest[role + "_final_cpu_stat"] = {"usage_usec": 1, "nr_throttled": 0}
        def scope(role, pid, ticks):
            return {"active_at_start": True, "active_at_end": True,
                    "scope": {"cgroup": role + "-scope", "memory_current": 1, "memory_events": dict(events),
                              "cpu_stat": {"usage_usec": 1, "nr_throttled": 0},
                              "processes": [{"pid": pid, "start_ticks": ticks}]}}
        self.samples = []
        for t in range(0, 36, 5):
            row = {"monotonic_ns": t*second, "sample_kind": "periodic", "phase": "normal" if t <= 25 else "recovery",
                   "host_cpu_pressure": f"some avg10=0.00 total={t * 1000}",
                   "host_cpu_pressure_started_ns": t*second, "host_cpu_pressure_completed_ns": t*second,
                   "cgroup": {"memory.events": dict(events)},
                   "ledger_counts": {"accepted_minus_received": 0}, "storage": {"subject_file_bytes": 1}}
            if t <= 25:
                row["generators"] = {"sender": scope("sender", 11, 101), "receiver": scope("receiver", 12, 102)}
            self.samples.append(row)
        self.samples.insert(6, {**self.samples[5], "phase": "recovery", "sample_kind": "boundary"})
        payload = expected_payload_hash(0, self.spec)
        self.sender = [{"event": "node_ready", "monotonic_ns": second},
                       {"event": "attempted", "sequence": 0, "sha256": payload, "monotonic_ns": 10*second+1,
                        "scheduled_monotonic_ns": 10*second},
                       {"event": "sender_final", "sequence": 0, "state": 4, "message_hash": "message", "monotonic_ns": 11*second},
                       {"event": "sender_complete", "attempted": 1, "final": 1, "monotonic_ns": 21*second}]
        self.receiver = [{"event": "node_ready", "monotonic_ns": 2*second}]
        for slot, t in enumerate((10, 15, 20)):
            self.receiver.extend([{"event": "poll_attempt", "slot": slot, "monotonic_ns": t*second,
                                   "scheduled_monotonic_ns": t*second},
                                  {"event": "poll_complete", "slot": slot, "monotonic_ns": t*second+1,
                                   "sync_state": 7, "link_status": 2, "pending_requests": 0}])
            if slot == 0:
                self.receiver.append({"event": "received", "sha256": payload, "message_hash": "message",
                                      "source": "sender", "destination": "receiver", "monotonic_ns": 12*second})
        self.receiver.append({"event": "receiver_complete", "count": 1, "quiet_interval_completed": True,
                              "sync_state": 7, "monotonic_ns": 21*second})
        self.assertTrue(self.result()["capture_valid"])
        self.assertTrue(self.result()["functional_pass"])
        self.assertAlmostEqual(self.result()["guest_cpu_psi_some_fraction"], 0.001)
        self.spec["receiver_poll_timeout_seconds"] = 10
        for row in self.receiver:
            if row["event"] == "poll_complete":
                row["request_statuses"] = [4]
            if row["event"] == "poll_attempt":
                row["poll_started_monotonic_ns"] = row["monotonic_ns"]
        first_complete = next(row for row in self.receiver if row["event"] == "poll_complete")
        saved_complete = dict(first_complete)
        first_complete.update(event="poll_failed", reason="terminal", sync_state=242, link_status=None, pending_requests=0,
                              request_statuses=[0], submission_failures=0)
        retried = self.result()
        self.assertTrue(retried["capture_valid"], retried)
        self.assertIn("normal propagation poll failed or timed out", retried["functional_errors"])
        self.receiver.remove(first_complete)
        self.assertFalse(self.result()["capture_valid"])
        self.receiver.insert(2, saved_complete)
        del self.spec["receiver_poll_timeout_seconds"]
        self.manifest["sender_exit"] = 1
        self.assertIn("sender peer exited unexpectedly", self.result()["capture_errors"])
        self.manifest["sender_exit"] = 0
        self.samples[2]["host_cpu_pressure_completed_ns"] += 1
        self.assertAlmostEqual(self.result()["guest_cpu_psi_some_fraction"], 0.0015)
        self.samples[2]["host_cpu_pressure_completed_ns"] -= 1
        self.samples[5]["host_cpu_pressure"] = "some avg10=0.00 total=9000000"
        self.assertFalse(self.result()["capture_valid"])
        self.samples[5]["host_cpu_pressure"] = "some avg10=0.00 total=25000"
        self.manifest["sender_final_cpu_stat"]["nr_throttled"] = 1
        self.assertFalse(self.result()["capture_valid"])
        self.manifest["sender_final_cpu_stat"]["nr_throttled"] = 0
        del self.manifest["receiver_final_memory_events"]
        self.assertFalse(self.result()["capture_valid"])
        self.manifest["receiver_final_memory_events"] = dict(events)
        self.samples[4]["host_cpu_pressure"] = "some avg10=0.00 total=9000000"
        self.assertFalse(self.result()["capture_valid"])
        self.samples[4]["host_cpu_pressure"] = "some avg10=0.00 total=20000"
        self.manifest.update(subject_unexpected_exit=True, subject_final_memory_events=dict(events),
                             sender_cancelled=True, receiver_cancelled=True, sender_exit=-15, receiver_exit=-15)
        crashed = self.result()
        self.assertTrue(crashed["capture_valid"])
        self.assertIn("subject exited unexpectedly", crashed["functional_errors"])
        self.assertFalse(crashed["delivery_evaluated"])
        self.manifest.update(subject_unexpected_exit=False, sender_cancelled=False, receiver_cancelled=False,
                             sender_exit=0, receiver_exit=0)
        saved = self.samples[2]["generators"].pop("receiver")
        self.assertFalse(self.result()["capture_valid"])
        self.samples[2]["generators"]["receiver"] = saved
        saved["scope"]["cpu_stat"]["nr_throttled"] = 1
        self.assertFalse(self.result()["capture_valid"])
        saved["scope"]["cpu_stat"]["nr_throttled"] = 0
        completed_poll = next(row for row in self.receiver if row["event"] == "poll_complete")
        completed_poll["pending_requests"] = 1
        self.assertFalse(self.result()["capture_valid"])
        completed_poll["pending_requests"] = 0
        self.spec["normal_delivery_target_seconds"] = 1
        self.assertIn("normal delivery exceeded its latency target", self.result()["functional_errors"])
        self.spec["route_deadline_seconds"] = 5
        self.manifest.update(normal_readiness_failed=True, normal_readiness_failed_roles=["sender"],
                             phase_times_ns={}, sender_exit=71, receiver_exit=-15,
                             sender_cancelled=False, receiver_cancelled=True,
                             receiver_stopped_after_readiness_failure=True)
        self.sender = [{"event": "node_discovery_start", "node": "test", "monotonic_ns": second},
                       {"event": "node_timeout", "node": "test", "monotonic_ns": 6*second}]
        self.receiver = [{"event": "node_ready", "node": "test", "monotonic_ns": 2*second}]
        failure = self.result()
        self.assertTrue(failure["capture_valid"], failure)
        self.assertFalse(failure["functional_pass"])
        self.assertFalse(failure["delivery_evaluated"])
        self.sender[-1]["monotonic_ns"] = 2*second
        self.assertFalse(self.result()["capture_valid"])

    def test_normal_subject_failure_before_peers_is_a_functional_result(self):
        self.spec.update(profile="preflight-only-normal-dry-run", duration_seconds=10, recovery_seconds=10)
        self.manifest.update(generator_telemetry_contract=2, cgroup="subject", sender_started=False,
                             receiver_started=False, subject_unexpected_exit=True,
                             subject_final_memory_events={"oom": 0, "oom_kill": 0})
        self.sender, self.receiver = [], []
        result = self.result()
        self.assertTrue(result["capture_valid"], result)
        self.assertIn("subject exited unexpectedly", result["functional_errors"])
        self.assertFalse(result["delivery_evaluated"])


if __name__ == "__main__":
    unittest.main()
