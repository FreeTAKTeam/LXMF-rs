"""Trace the pinned production router; imported only after reference verification."""

from contextlib import ExitStack
from types import SimpleNamespace
import struct
import threading
from unittest.mock import patch


def generate_retry_traces(RNS, LXMF, source, recipient):
    scenarios = [
        ("pathless_exhaustion", False, 8, 2000, 6, "none", [
            {"at": 8}, {"at": 8.001}, {"at": 18.001}, {"at": 18.002},
            {"at": 26.003}, {"at": 34.004}, {"at": 42.005},
            {"at": 50.006}, {"at": 58.006}, {"at": 58.007},
        ]),
        ("known_rediscovery_and_announce", True, 8, 2000, 6, "receipt", [
            {"at": 10}, {"at": 10.001}, {"at": 20.002},
            {"at": 20.501}, {"at": 20.502}, {"at": 20.6, "path": True},
            {"at": 20.6}, {"at": 20.7, "delivered": True}, {"at": 40},
        ]),
        ("rediscovery_already_pending", True, 8, 2000, 6, "none", [
            {"at": 10.001}, {"at": 20.002, "request_age": 1},
            {"at": 20.003},
        ]),
        ("slow_pending_outlives_debounce", False, 86, 100, 46, "none", [
            {"at": 60}, {"at": 86}, {"at": 86.001}, {"at": 172.002},
            {"at": 258.003}, {"at": 344.004}, {"at": 430.005},
        ]),
        ("slow_debounce_outlives_pending", False, 8, 1999, 6, "none", [
            {"at": 8.001}, {"at": 18.002}, {"at": 26.003},
            {"at": 34.004}, {"at": 42.005}, {"at": 50.006}, {"at": 58.007},
        ]),
        ("path_appears_after_pathless_send", False, 8, 2000, 6, "none", [
            {"at": 8.001}, {"at": 18.002}, {"at": 19, "path": True},
            {"at": 29.001},
        ]),
        ("rediscovery_preserves_prior_limit", True, 8, 2000, 9, "none", [
            {"at": 12.001}, {"at": 24.002}, {"at": 24.502},
            {"at": 25, "path": True, "first_hop": 46}, {"at": 111.001},
        ]),
        ("send_raises", True, 8, 2000, 6, "raise", [{"at": 20}]),
        ("late_delivery_during_rediscovery", True, 8, 2000, 6, "receipt", [
            {"at": 10.001}, {"at": 20.002}, {"at": 20.1, "delivered": True},
            {"at": 20.502}, {"at": 40},
        ]),
        ("cancel_during_rediscovery", True, 8, 2000, 6, "none", [
            {"at": 10.001}, {"at": 20.002}, {"at": 20.1, "cancel": True},
            {"at": 20.502}, {"at": 40},
        ]),
    ]
    result = []
    for name, known, medium, bitrate, first_hop, send_effect, events in scenarios:
        router = object.__new__(LXMF.LXMRouter)
        router.outbound_processing_lock = threading.Lock()
        router.path_request_lock = threading.Lock()
        router.path_request_times = {}
        router.outbound_stamp_costs = {}
        router.pending_outbound = []
        router.pending_deferred_stamps = {}
        router.get_outbound_ticket = lambda _: None
        router.direct_links = {}
        now, path, first = [1000.0], [known], [first_hop]
        actions, delayed, callbacks = [], [], []
        message = LXMF.LXMessage(recipient, source, content=b"retry trace",
                                 desired_method=LXMF.LXMessage.OPPORTUNISTIC)
        message.timestamp = 1773999000.25
        message.defer_stamp = False
        message.failed_callback = lambda _: actions.append("failed_callback")
        sent_bytes = []

        def drop_path(_):
            path[0] = False
            actions.append("drop_path")

        def send_packet(packet):
            actions.append("send")
            # Validate afterward: process_outbound catches send exceptions,
            # including AssertionError, so assertions here would be swallowed.
            sent_bytes.append((bytes(packet.data), bytes(message.packed)))
            if send_effect == "raise":
                raise IOError("injected packet-send failure")
            if send_effect == "receipt":
                return SimpleNamespace(set_delivery_callback=callbacks.append)
            return None

        class ControlledThread:
            def __init__(self, target, daemon):
                assert daemon
                self.target = target

            def start(self):
                if self.target == router.process_outbound:
                    self.target()
                else:
                    # Run the actual reference closure when its sleep is due.
                    assert self.target.__name__ == "rediscover_job"
                    delayed.append((now[0] + 0.5, self.target))

        instance = SimpleNamespace(
            get_medium_path_timeout=lambda: medium,
            get_lowest_interface_bitrate=lambda: bitrate,
            get_first_hop_timeout=lambda _: first[0],
            drop_path=drop_path,
            _retain_destination_data=lambda _: actions.append("retain_destination"),
        )
        rows = []

        def snapshot(event):
            rows.append({
                "event": event, "time": now[0], "path_known": path[0],
                "first_hop": first[0], "attempts": message.delivery_attempts,
                "max_attempts": router.attempt_limit(message),
                "deadline": getattr(message, "next_delivery_attempt", None),
                "awaiting_path": message.awaiting_path, "sent_on_path": message.sent_on_path,
                "state": message.state, "pending": message in router.pending_outbound,
                "request_time": router.path_request_times.get(recipient.hash),
                "actions": actions.copy(), "delayed_jobs": len(delayed),
            })
            for field in ["time", "deadline", "request_time"]:
                value = rows[-1][field]
                rows[-1][field + "_bits"] = (None if value is None else
                    int.from_bytes(struct.pack(">d", float(value)), "big"))
            actions.clear()

        with ExitStack() as stack:
            stack.enter_context(patch.object(RNS.Reticulum, "get_instance", return_value=instance))
            stack.enter_context(patch.object(RNS.Transport, "has_path", side_effect=lambda _: path[0]))
            stack.enter_context(patch.object(RNS.Transport, "request_path", side_effect=lambda _: actions.append("request_path")))
            stack.enter_context(patch.object(RNS.Packet, "send", new=send_packet))
            stack.enter_context(patch("time.time", side_effect=lambda: now[0]))
            stack.enter_context(patch("time.sleep", side_effect=lambda seconds: actions.append(f"sleep:{seconds}")))
            stack.enter_context(patch("threading.Thread", ControlledThread))
            router.handle_outbound(message)
            snapshot({"admit": True})
            for event in events:
                now[0] = 1000.0 + event["at"]
                for deadline, target in delayed.copy():
                    if deadline <= now[0]:
                        delayed.remove((deadline, target))
                        target()
                if "path" in event:
                    path[0] = event["path"]
                if "first_hop" in event:
                    first[0] = event["first_hop"]
                if "request_age" in event:
                    router.path_request_times[recipient.hash] = now[0] - event["request_age"]
                if event.get("delivered"):
                    callbacks[0]()
                if event.get("cancel"):
                    message.state = LXMF.LXMessage.CANCELLED
                router.process_outbound()
                snapshot(event)
        assert sent_bytes, f"{name}: scenario did not exercise LXMessage.send"
        for packet_data, packed in sent_bytes:
            assert packet_data == packed[LXMF.LXMessage.DESTINATION_LENGTH:], name
            assert packed == sent_bytes[0][1], f"{name}: signed bytes changed on retry"
        result.append({
            "name": name, "initial_path_known": known, "medium_timeout": medium,
            "lowest_bitrate": bitrate, "initial_first_hop": first_hop,
            "send_effect": send_effect, "rows": rows,
        })
    return result
