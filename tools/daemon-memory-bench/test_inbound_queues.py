import unittest

from validate import inbound_queue_errors, proof_timeout_errors


def snapshot(stage, heights=None, dropped=None):
    heights = heights or [0, 0, 0, 0]
    return {"event": "inbound_queue", "stage": stage, "total": sum(heights),
            "heights": heights, "dropped": dropped or [0, 0, 0, 0]}


class InboundQueueTest(unittest.TestCase):
    def test_completed_peer_requires_well_formed_drop_free_snapshots(self):
        baseline, final = snapshot("baseline"), snapshot("final")
        self.assertEqual(inbound_queue_errors("sender", [baseline, final], True), [])
        self.assertTrue(inbound_queue_errors("sender", [baseline], True))
        self.assertTrue(inbound_queue_errors("sender", [baseline, snapshot("final", dropped=[0, 1, 0, 0])], True))
        self.assertTrue(inbound_queue_errors("sender", [baseline, dict(final, total=2)], True))

    def test_interrupted_peer_does_not_need_a_final_snapshot(self):
        self.assertEqual(inbound_queue_errors("receiver", [snapshot("baseline")], False), [])
        self.assertTrue(inbound_queue_errors("receiver", [snapshot("baseline", dropped=[1, 0, 0, 0])], False))

    def test_proof_timeout_rejects_early_or_culled_receipts(self):
        sends = {0: {"submitted": True}}
        failure = {"sequence": 0, "state": 255, "receipt_status": 0, "proof_elapsed_seconds": 30.1}
        self.assertEqual(proof_timeout_errors([failure], sends, 30), [])
        self.assertTrue(proof_timeout_errors([dict(failure, proof_elapsed_seconds=0.02)], sends, 30))
        self.assertTrue(proof_timeout_errors([dict(failure, receipt_status=255)], sends, 30))
        self.assertEqual(proof_timeout_errors([failure], {0: {"submitted": False}}, 30), [])


if __name__ == "__main__":
    unittest.main()
