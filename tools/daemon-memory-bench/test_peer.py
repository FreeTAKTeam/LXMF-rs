import importlib.util
import sys
import threading
import tempfile
import time
import types
import unittest
from pathlib import Path
from unittest.mock import patch


lxmf = types.ModuleType("LXMF")
lxmf.LXMessage = types.SimpleNamespace(SENT=4, DELIVERED=8, FAILED=255, REJECTED=253, CANCELLED=254)
with patch.dict(sys.modules, {"LXMF": lxmf, "RNS": types.ModuleType("RNS")}):
    spec = importlib.util.spec_from_file_location("bench_peer", Path(__file__).with_name("peer.py"))
    peer = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(peer)
peer.RNS.Transport = types.SimpleNamespace(inbound_queues=types.SimpleNamespace(snapshot=lambda: (0, (0, 0, 0, 0), (0, 0, 0, 0))))


class PendingMessagesTest(unittest.TestCase):
    def test_old_link_callback_cannot_tear_down_the_replacement(self):
        old, replacement, events, errors = object(), object(), [], []
        router = types.SimpleNamespace(outbound_propagation_link=replacement, clean_links=lambda: None)
        router.message_list_response = lambda receipt: None
        router.message_get_response = lambda receipt: None
        router.message_get_progress = lambda receipt: None
        router.message_get_failed = lambda receipt: setattr(router, 'outbound_propagation_link', None)
        active = [replacement]
        peer.fence_router_callbacks(router, threading.RLock(), lambda event, **fields: events.append((event, fields)), errors, active)
        router.message_get_failed(types.SimpleNamespace(link=old))
        self.assertIs(router.outbound_propagation_link, replacement)
        self.assertEqual(events[0][0], 'poll_stale_callback')
        active[0] = None
        router.message_get_failed(types.SimpleNamespace(link=replacement))
        self.assertIs(router.outbound_propagation_link, replacement)
        active[0] = replacement
        router.message_get_failed(types.SimpleNamespace(link=replacement))
        self.assertIsNone(router.outbound_propagation_link)
        self.assertEqual(errors, [])

    def test_retry_link_has_no_automatic_establishment_callback_and_tracks_requests(self):
        calls, receipts, errors = [], [], []
        class Destination:
            OUT, SINGLE = 1, 2
            def __init__(self, *args):
                calls.append(('destination', args))
        class Link:
            def __init__(self, *args, **kwargs):
                calls.append(('link', args, kwargs))
            def request(self, *args, **kwargs):
                return types.SimpleNamespace(status=4)
        transport = types.SimpleNamespace(has_path=lambda node: True, request_path=lambda node: None)
        identity = types.SimpleNamespace(recall=lambda node: object())
        router = types.SimpleNamespace(outbound_propagation_node=b'node', outbound_propagation_link=None)
        with patch.object(peer.RNS, 'Transport', transport), patch.object(peer.RNS, 'Identity', identity, create=True), \
             patch.object(peer.RNS, 'Destination', Destination, create=True), patch.object(peer.RNS, 'Link', Link, create=True), \
             patch.object(peer.LXMF, 'LXMRouter', types.SimpleNamespace(PR_LINK_ESTABLISHING=2), create=True):
            self.assertTrue(peer.begin_retry_link(router, receipts, errors))
            self.assertEqual(calls[-1][2], {})
            router.outbound_propagation_link.request('/get')
        self.assertEqual(len(receipts), 1)
        self.assertEqual(errors, [])

    def test_completed_messages_are_recorded_once_and_released(self):
        pending = {1: types.SimpleNamespace(state=1, hash=b"a", transient_id=b"1"),
                   2: types.SimpleNamespace(state=4, hash=b"b", transient_id=b"2"),
                   3: types.SimpleNamespace(state=253, hash=b"c", transient_id=None),
                   4: types.SimpleNamespace(state=254, hash=b"d", transient_id=None)}
        events = []
        record = lambda event, **values: events.append((event, values))
        self.assertEqual(peer.observe(pending, record), 3)
        self.assertEqual(set(pending), {1})
        self.assertEqual([values["sequence"] for _, values in events], [2, 3, 4])
        self.assertEqual(events[0][1]["transient_id"], b"2".hex())
        self.assertEqual(peer.observe(pending, record), 0)
        pending[1].state = 8
        self.assertEqual(peer.observe(pending, record), 1)
        self.assertFalse(pending)
        self.assertEqual(len(events), 4)

    def test_recording_failure_does_not_drop_pending_message(self):
        pending = {1: types.SimpleNamespace(state=4, hash=b"a", transient_id=None)}

        def fail(*args, **kwargs):
            raise OSError("ledger failure")

        with self.assertRaises(OSError):
            peer.observe(pending, fail)
        self.assertIn(1, pending)

    def test_delivery_and_quiet_completion_are_serialized(self):
        callback_entered = threading.Event()
        callback_continue = threading.Event()
        events = []

        def record(event, **values):
            if event == "received":
                callback_entered.set()
                if not callback_continue.wait(2):
                    raise RuntimeError("test callback timed out")
            events.append(event)

        progress = peer.ReceiverProgress(record)
        progress.count = 1
        progress.last_received = time.monotonic() - 10
        message = types.SimpleNamespace(content=b"data", hash=b"h", source_hash=b"s", destination_hash=b"d")
        worker = threading.Thread(target=progress.receive, args=(message,))
        worker.start()
        self.assertTrue(callback_entered.wait(2))
        result = []
        attempting = threading.Event()
        finished = threading.Event()
        router = types.SimpleNamespace(propagation_transfer_state=7)

        def finish():
            attempting.set()
            result.append(progress.finish_if_quiet(1, 5, time.monotonic() + 10, router))
            finished.set()

        finisher = threading.Thread(target=finish)
        finisher.start()
        self.assertTrue(attempting.wait(2))
        self.assertFalse(finished.wait(0.05))
        callback_continue.set()
        worker.join(2)
        finisher.join(2)
        self.assertEqual(result, [False])
        self.assertEqual(events, ["received"])

    def test_callback_recording_error_reaches_receiver_main(self):
        def fail(*args, **kwargs):
            raise OSError("ledger write failed")

        progress = peer.ReceiverProgress(fail)
        message = types.SimpleNamespace(content=b"data", hash=b"h", source_hash=b"s", destination_hash=b"d")
        progress.receive(message)
        router = types.SimpleNamespace(propagation_transfer_state=7)
        with self.assertRaises(peer.CaptureError):
            progress.finish_if_quiet(1, 1, time.monotonic() + 10, router)

    def test_normal_receiver_keeps_polling_until_window_end(self):
        now = [10.0]
        events = []
        link = types.SimpleNamespace(status=2, pending_requests=[])
        router = types.SimpleNamespace(outbound_propagation_link=link, propagation_transfer_state=0)
        router.request_messages_from_propagation_node = lambda identity: setattr(router, 'propagation_transfer_state', 7)
        receiver = types.SimpleNamespace(finish_if_quiet=lambda *args: True,
                                         finish_timeout=lambda *args: self.fail('unexpected timeout'))
        spec = {'duration_seconds': 2, 'receiver_drain_seconds': 4, 'receiver_poll_interval_seconds': 1,
                'maximum_schedule_lateness_seconds': 0.5, 'message_count': 1, 'receiver_quiet_seconds': 1}
        with patch.object(peer.time, 'monotonic', side_effect=lambda: now[0]), \
             patch.object(peer.time, 'sleep', side_effect=lambda delay: now.__setitem__(0, now[0] + delay)), \
             patch.object(peer.LXMF, 'LXMRouter', types.SimpleNamespace(PR_COMPLETE=7), create=True), \
             patch.object(peer.RNS, 'Link', types.SimpleNamespace(ACTIVE=2), create=True):
            peer.normal_receive(router, object(), receiver, lambda event, **values: events.append((event, values)), spec, 10.0)
        attempts = [values['slot'] for event, values in events if event == 'poll_attempt']
        self.assertEqual(attempts, [0, 1, 2])
        self.assertEqual([values['slot'] for event, values in events if event == 'poll_complete'], attempts)

    def test_normal_receiver_records_overlapping_poll_as_failure(self):
        now, events, timed_out = [10.0], [], []
        router = types.SimpleNamespace(outbound_propagation_link=None, propagation_transfer_state=1,
                                       request_messages_from_propagation_node=lambda identity: None)
        receiver = types.SimpleNamespace(finish_timeout=lambda router: timed_out.append(True))
        spec = {'duration_seconds': 2, 'receiver_drain_seconds': 4, 'receiver_poll_interval_seconds': 1,
                'maximum_schedule_lateness_seconds': 0.5, 'message_count': 1, 'receiver_quiet_seconds': 1}
        with patch.object(peer.time, 'monotonic', side_effect=lambda: now[0]), \
             patch.object(peer.time, 'sleep', side_effect=lambda delay: now.__setitem__(0, now[0] + delay)), \
             patch.object(peer.LXMF, 'LXMRouter', types.SimpleNamespace(PR_COMPLETE=7), create=True):
            peer.normal_receive(router, object(), receiver, lambda event, **values: events.append((event, values)), spec, 10.0)
        self.assertTrue(timed_out)
        missed = [values['slot'] for event, values in events if event == 'poll_missed' and values['previous_pending']]
        self.assertEqual(missed[:2], [1, 2])

    def test_terminal_poll_failure_is_recorded_and_next_slot_retries(self):
        now, events, attempts = [10.0], [], []
        current = {}
        router = types.SimpleNamespace(outbound_propagation_link=None, propagation_transfer_state=0)
        def begin(router, receipts, _errors):
            current['receipts'] = receipts
            router.outbound_propagation_link = types.SimpleNamespace(status=2, pending_requests=[])
            return True
        def request(_identity):
            attempts.append(now[0])
            if len(attempts) == 1:
                router.propagation_transfer_state = 242
            else:
                current['receipts'].append(types.SimpleNamespace(status=4))
                router.propagation_transfer_state = 7
        def cancel():
            router.outbound_propagation_link = None
            router.propagation_transfer_state = 0
        router.request_messages_from_propagation_node = request
        router.cancel_propagation_node_requests = cancel
        receiver = types.SimpleNamespace(finish_if_quiet=lambda *args: True,
                                         finish_timeout=lambda *args: self.fail('unexpected timeout'))
        spec = {'duration_seconds': 2, 'receiver_drain_seconds': 4, 'receiver_poll_interval_seconds': 1,
                'receiver_poll_timeout_seconds': 2, 'maximum_schedule_lateness_seconds': 0.5,
                'message_count': 1, 'receiver_quiet_seconds': 1}
        with patch.object(peer.time, 'monotonic', side_effect=lambda: now[0]), \
             patch.object(peer.time, 'sleep', side_effect=lambda delay: now.__setitem__(0, now[0] + delay)), \
             patch.object(peer.LXMF, 'LXMRouter', types.SimpleNamespace(PR_COMPLETE=7, PR_NO_PATH=240), create=True), \
             patch.object(peer.RNS, 'Link', types.SimpleNamespace(ACTIVE=2, CLOSED=4), create=True), \
             patch.object(peer.RNS, 'RequestReceipt', types.SimpleNamespace(READY=4, FAILED=0), create=True), \
             patch.object(peer, 'begin_retry_link', side_effect=begin):
            peer.normal_receive(router, object(), receiver, lambda event, **values: events.append((event, values)), spec, 10.0)
        self.assertEqual([row['slot'] for event, row in events if event == 'poll_attempt'], [0, 1, 2])
        self.assertEqual([(row['slot'], row['reason']) for event, row in events if event == 'poll_failed'], [(0, 'terminal')])
        self.assertEqual([row['slot'] for event, row in events if event == 'poll_complete'], [1, 2])

    def test_stalled_poll_times_out_before_next_retry(self):
        now, events, attempts = [10.0], [], []
        current = {}
        router = types.SimpleNamespace(outbound_propagation_link=None, propagation_transfer_state=0)
        def begin(router, receipts, _errors):
            current['receipts'] = receipts
            router.outbound_propagation_link = types.SimpleNamespace(status=2, pending_requests=[])
            return True
        def request(_identity):
            attempts.append(now[0])
            router.outbound_propagation_link.pending_requests = [1] if len(attempts) == 1 else []
            current['receipts'].append(types.SimpleNamespace(status=1 if len(attempts) == 1 else 4))
            router.propagation_transfer_state = 4 if len(attempts) == 1 else 7
        def cancel():
            router.outbound_propagation_link = None
            router.propagation_transfer_state = 0
        router.request_messages_from_propagation_node = request
        router.cancel_propagation_node_requests = cancel
        receiver = types.SimpleNamespace(finish_if_quiet=lambda *args: True,
                                         finish_timeout=lambda *args: self.fail('unexpected timeout'))
        spec = {'duration_seconds': 3, 'receiver_drain_seconds': 4, 'receiver_poll_interval_seconds': 1,
                'receiver_poll_timeout_seconds': 2, 'maximum_schedule_lateness_seconds': 0.5,
                'message_count': 1, 'receiver_quiet_seconds': 1}
        with patch.object(peer.time, 'monotonic', side_effect=lambda: now[0]), \
             patch.object(peer.time, 'sleep', side_effect=lambda delay: now.__setitem__(0, now[0] + delay)), \
             patch.object(peer.LXMF, 'LXMRouter', types.SimpleNamespace(PR_COMPLETE=7, PR_NO_PATH=240), create=True), \
             patch.object(peer.RNS, 'Link', types.SimpleNamespace(ACTIVE=2, CLOSED=4), create=True), \
             patch.object(peer.RNS, 'RequestReceipt', types.SimpleNamespace(READY=4, FAILED=0), create=True), \
             patch.object(peer, 'begin_retry_link', side_effect=begin):
            peer.normal_receive(router, object(), receiver, lambda event, **values: events.append((event, values)), spec, 10.0)
        self.assertEqual([(row['slot'], row['reason']) for event, row in events if event == 'poll_failed'], [(0, 'timeout')])
        self.assertEqual([row['slot'] for event, row in events if event == 'poll_missed'], [1])
        self.assertEqual([row['slot'] for event, row in events if event == 'poll_complete'], [2, 3])

    def test_prepared_receiver_polls_after_sender_and_last_delivery(self):
        now, events = [10.0], []
        link = types.SimpleNamespace(status=2, pending_requests=[])
        router = types.SimpleNamespace(outbound_propagation_link=link, propagation_transfer_state=0)
        router.request_messages_from_propagation_node = lambda identity: setattr(router, 'propagation_transfer_state', 7)
        receiver = types.SimpleNamespace(lock=threading.Lock(), last_received=12.3,
                                         finish_if_quiet=lambda *args: True,
                                         finish_timeout=lambda *args: self.fail('unexpected timeout'))
        config = {'generator_mode': 'prepared_packets', 'duration_seconds': 2, 'receiver_drain_seconds': 4,
                  'receiver_poll_interval_seconds': 1, 'maximum_schedule_lateness_seconds': 0.5,
                  'message_count': 1, 'receiver_quiet_seconds': 1}
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            def sleep(delay):
                now[0] += delay
                if now[0] >= 12.2 and not (root / 'sender-complete.json').exists():
                    (root / 'sender-complete.json').write_text('{"monotonic_ns": 12200000000}')
            with patch.object(peer.time, 'monotonic', side_effect=lambda: now[0]), \
                 patch.object(peer.time, 'sleep', side_effect=sleep), \
                 patch.object(peer.LXMF, 'LXMRouter', types.SimpleNamespace(PR_COMPLETE=7), create=True), \
                 patch.object(peer.RNS, 'Link', types.SimpleNamespace(ACTIVE=2), create=True):
                peer.normal_receive(router, object(), receiver, lambda event, **values: events.append((event, values)), config, 10.0, root)
        self.assertGreaterEqual(now[0], 13.0)
        self.assertTrue(any(event == 'sender_done_observed' for event, _ in events))

    def test_receiver_requires_poll_after_observing_sender_marker(self):
        now, events = [10.0], []
        link = types.SimpleNamespace(status=2, pending_requests=[])
        router = types.SimpleNamespace(outbound_propagation_link=link, propagation_transfer_state=0)
        receiver = types.SimpleNamespace(lock=threading.Lock(), last_received=10.1,
                                         finish_if_quiet=lambda *args: True,
                                         finish_timeout=lambda *args: self.fail('unexpected timeout'))
        config = {'generator_mode': 'prepared_packets', 'duration_seconds': 2, 'receiver_drain_seconds': 4,
                  'receiver_poll_interval_seconds': 1, 'maximum_schedule_lateness_seconds': 0.5,
                  'message_count': 1, 'receiver_quiet_seconds': 1}
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            def request(_identity):
                router.propagation_transfer_state = 7
                if now[0] >= 12 and not (root / 'sender-complete.json').exists():
                    (root / 'sender-complete.json').write_text('{"monotonic_ns": 11900000000}')
            router.request_messages_from_propagation_node = request
            def record(event, **values):
                events.append((event, values))
                now[0] += 0.001
            with patch.object(peer.time, 'monotonic', side_effect=lambda: now[0]), \
                 patch.object(peer.time, 'sleep', side_effect=lambda delay: now.__setitem__(0, now[0] + delay)), \
                 patch.object(peer.LXMF, 'LXMRouter', types.SimpleNamespace(PR_COMPLETE=7), create=True), \
                 patch.object(peer.RNS, 'Link', types.SimpleNamespace(ACTIVE=2), create=True):
                peer.normal_receive(router, object(), receiver, record, config, 10.0, root)
        self.assertGreaterEqual(now[0], 13.0)
        self.assertTrue(any(event == 'sender_done_observed' for event, _ in events))


if __name__ == "__main__":
    unittest.main()
