import unittest

from run import check_spec


class IdleSpecTest(unittest.TestCase):
    def setUp(self):
        self.spec = {"state": "frozen", "profile": "idle", "message_count": 0,
                     "payload_bytes": 256, "interval_seconds": 30, "duration_seconds": 1800,
                     "warmup_seconds": 60, "route_deadline_seconds": 45, "sender_drain_seconds": 60,
                     "receiver_drain_seconds": 215, "receiver_quiet_seconds": 30,
                     "sample_interval_seconds": 5, "seed": "idle"}

    def test_idle_requires_zero_work_and_full_duration(self):
        check_spec(self.spec)
        self.spec["message_count"] = 1
        with self.assertRaises(ValueError):
            check_spec(self.spec)
        self.spec["message_count"] = 0
        self.spec["duration_seconds"] = 10
        with self.assertRaises(ValueError):
            check_spec(self.spec)

    def test_short_idle_preflight_can_exercise_phase_logic(self):
        self.spec.update(profile="preflight-only-idle-dry-run", duration_seconds=10)
        check_spec(self.spec)

    def test_offline_requires_two_hours_and_one_hour_recovery(self):
        self.spec.update(profile="offline", message_count=240, duration_seconds=7200,
                         recovery_seconds=3600, acceptance_latency_target_seconds=30,
                         retrieval_latency_target_seconds=180)
        check_spec(self.spec)
        self.spec["recovery_seconds"] = 3599
        with self.assertRaises(ValueError):
            check_spec(self.spec)

    def test_normal_requires_two_hours_recovery_and_poll_targets(self):
        self.spec.update(profile="normal", message_count=240, duration_seconds=7200, recovery_seconds=3600,
                         receiver_poll_interval_seconds=30, normal_delivery_target_seconds=120,
                         acceptance_latency_target_seconds=30, max_guest_cpu_psi_some_fraction=0.10)
        check_spec(self.spec)
        self.spec["max_guest_cpu_psi_some_fraction"] = float("nan")
        with self.assertRaises(ValueError):
            check_spec(self.spec)
        self.spec["max_guest_cpu_psi_some_fraction"] = 0.10
        self.spec["receiver_poll_interval_seconds"] = 0
        with self.assertRaises(ValueError):
            check_spec(self.spec)
        self.spec["receiver_poll_interval_seconds"] = 30
        self.spec["duration_seconds"] = 7199
        with self.assertRaises(ValueError):
            check_spec(self.spec)

    def test_prepared_packet_preflight_requires_precise_schedule_and_matching_cost(self):
        self.spec.update(profile="preflight-only-normal-dry-run", message_count=2, interval_seconds=0.2,
                         duration_seconds=10, recovery_seconds=10, receiver_poll_interval_seconds=5,
                         normal_delivery_target_seconds=30, acceptance_latency_target_seconds=30,
                         max_guest_cpu_psi_some_fraction=0.1, generator_mode="prepared_packets",
                         propagation_stamp_target=16, propagation_stamp_flexibility=3,
                         propagation_transfer_kb=256, propagation_sync_kb=10240, preparation_deadline_seconds=120,
                         maximum_schedule_lateness_seconds=0.1, maximum_poll_lateness_seconds=1)
        check_spec(self.spec)
        self.spec['maximum_schedule_lateness_seconds'] = 0.2
        with self.assertRaises(ValueError):
            check_spec(self.spec)
        self.spec.update(profile='high-load', duration_seconds=300, recovery_seconds=60,
                         message_count=1500, maximum_schedule_lateness_seconds=0.1,
                         receiver_poll_timeout_seconds=10)
        check_spec(self.spec)
        self.spec['recovery_seconds'] = 59
        with self.assertRaises(ValueError):
            check_spec(self.spec)


if __name__ == "__main__":
    unittest.main()
