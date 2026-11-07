"""Production LXMF Link retry/callback traces with controlled transport effects.

Imported only after the parent generator verifies exact clean reference pins.
No socket, Resource worker or cryptographic delivery proof is simulated as real.
"""

from contextlib import ExitStack
from types import SimpleNamespace
import struct
import threading
from unittest.mock import patch


def generate_link_traces(RNS, LXMF, source, recipient):
    specs = []
    for mode in ["direct", "propagated"]:
        for speed, medium, bitrate, first in [("normal", 8, 2000, 6), ("slow", 86, 100, 46)]:
            specs.append(dict(name=f"{mode}_{speed}_pathless_exhaustion", mode=mode,
                              path=False, medium=medium, bitrate=bitrate, first=first,
                              events=[{"at": medium}, {"at": medium + .001},
                                      {"at": medium * 2 + .002},
                                      {"at": medium * 3 + .003},
                                      {"at": medium * 4 + .004}]))
        specs.extend([
            dict(name=f"{mode}_slow_known_closure_exhaustion", mode=mode,
                 medium=86, bitrate=100, first=46,
                 events=[{"at": 20}, {"at": 100, "close": True}, {"at": 186},
                         {"at": 186.001}, {"at": 187, "close": True}, {"at": 272.002}]),
            dict(name=f"{mode}_changed_medium_after_closure", mode=mode,
                 events=[{"at": 1, "close": True, "path": False, "medium": 86, "bitrate": 100, "first": 46},
                         {"at": 2, "path": True}, {"at": 87}, {"at": 87.001},
                         {"at": 88, "activate": True}, {"at": 89, "proof": 0}]),
            dict(name=f"{mode}_external_close_while_sending", mode=mode, initial="active",
                 events=[{"at": 1, "close": True}, {"at": 11.001},
                         {"at": 12, "activate": True}, {"at": 13}]),
            dict(name=f"{mode}_shared_link_two_messages", mode=mode, shared=True,
                 events=[{"at": 21}, {"at": 22, "activate": True},
                         {"at": 23, "proof": 0}, {"at": 24, "proof": 1}]),
            dict(name=f"{mode}_pending_then_late_activation", mode=mode,
                 events=[{"at": 10}, {"at": 20}, {"at": 30},
                         {"at": 31, "activate": True}, {"at": 32},
                         {"at": 33, "proof": 0}, {"at": 34}]),
            dict(name=f"{mode}_never_activated_closure_exhaustion", mode=mode,
                 events=[{"at": 1, "close": True}, {"at": 11}, {"at": 11.001},
                         {"at": 12, "close": True}, {"at": 22.001},
                         {"at": 23, "close": True}, {"at": 33.001},
                         {"at": 34, "close": True}, {"at": 43.001}, {"at": 43.002}]),
            dict(name=f"{mode}_packet_timeout_retry", mode=mode,
                 events=[{"at": 1, "activate": True}, {"at": 2, "timeout": 0},
                         {"at": 12}, {"at": 12.001}, {"at": 13, "activate": True},
                         {"at": 14, "timeout": 1}, {"at": 24.001},
                         {"at": 25, "activate": True}, {"at": 26, "proof": 2}]),
            dict(name=f"{mode}_packet_no_receipt", mode=mode, initial="active", effect="none",
                 events=[{"at": 1}, {"at": 11.001}, {"at": 12, "activate": True}, {"at": 13}]),
            dict(name=f"{mode}_send_raises", mode=mode, initial="active", effect="raise", events=[{"at": 1}]),
            dict(name=f"{mode}_cancel_pending", mode=mode,
                 events=[{"at": 1, "cancel": True}, {"at": 31, "activate": True}]),
            dict(name=f"{mode}_path_appears_before_deadline", mode=mode, path=False,
                 events=[{"at": 1, "path": True}, {"at": 2, "activate": True},
                         {"at": 3}, {"at": 4, "proof": 0}]),
            dict(name=f"{mode}_late_proof_after_timeout", mode=mode,
                 events=[{"at": 1, "activate": True}, {"at": 2, "timeout": 0},
                         {"at": 12.001}, {"at": 13, "activate": True},
                         {"at": 14, "proof": 0}, {"at": 15, "proof": 1}]),
            dict(name=f"{mode}_proof_after_cancel", mode=mode, initial="active",
                 events=[{"at": 1, "cancel": True}, {"at": 2, "proof": 0}]),
        ])
        for outcome in ["complete", "rejected", "failed"]:
            events = [{"at": 1, "resource": outcome}, {"at": 2}]
            if outcome == "failed" or (mode == "propagated" and outcome == "rejected"):
                events += [{"at": 11.001}, {"at": 12, "activate": True},
                           {"at": 13, "resource": "complete"}]
            specs.append(dict(name=f"{mode}_resource_{outcome}", mode=mode, large=True,
                              initial="active", events=events))
        specs.append(dict(name=f"{mode}_resource_failure_after_cancel", mode=mode,
                          large=True, initial="active",
                          events=[{"at": 1, "cancel": True, "resource": "failed"}, {"at": 2}]))
    specs += [
        dict(name="direct_outbound_before_backchannel", mode="direct", initial="both_active", events=[{"at": 1}, {"at": 2, "proof": 0}]),
        dict(name="direct_pending_before_active_backchannel", mode="direct", initial="pending_and_backchannel", events=[{"at": 21}, {"at": 22, "activate": True}, {"at": 23, "proof": 0}]),
        dict(name="direct_backchannel_only", mode="direct", initial="backchannel", events=[{"at": 1, "proof": 0}]),
        dict(name="oversized_opportunistic_uses_direct_resource", mode="opportunistic", large=True,
             events=[{"at": 21}, {"at": 22, "activate": True}, {"at": 23, "resource": "complete"}]),
    ]
    malformed = ["92ccf5", "92ccf5c1", "92ccf5a1ff", "ccf5", "90", "91ccf4",
                 "92ccf58201000100", "92ccf5818000", "92ccf5d4ff00"]
    for name, events in [
        ("active_creator", [{"at": 1, "activate": True}, {"at": 2, "signal": "91ccf5"}, {"at": 3, "proof": 1}]),
        ("retired_creator", [{"at": 1, "activate": True}, {"at": 2, "proof": 0}, {"at": 3, "signal": "91ccf5"}, {"at": 4, "proof": 1}]),
        ("sent_pending_creator", [{"at": 1, "activate": True}, {"at": 2, "proof": 0, "process": False}, {"at": 2, "signal": "91ccf5"}, {"at": 3, "proof": 1}]),
        ("replacement_creator", [{"at": 1, "activate": True}, {"at": 2, "proof": 0}, {"at": 3, "proof": 1}, {"at": 4, "close": True},
             {"at": 5, "admit_new": True}, {"at": 15.001}, {"at": 16, "activate": True},
             {"at": 17, "signal": "91ccf5", "signal_link": 0}, {"at": 18, "signal": "91ccf5", "signal_link": 1}]),
        ("malformed_then_valid", [{"at": 1, "activate": True}] + [{"at": 2 + i/10, "signal": value} for i, value in enumerate(malformed)]
             + [{"at": 3, "signal": "91cd00f5"}, {"at": 4, "proof": 1}]),
    ]:
        specs.append(dict(name=f"propagated_signal_{name}", mode="propagated", shared=True, events=events))
    traces = [_trace(RNS, LXMF, source, recipient, spec) for spec in specs]
    _validate_traces(traces)
    return traces


def _trace(RNS, LXMF, source, recipient, spec):
    router = object.__new__(LXMF.LXMRouter)
    router.outbound_processing_lock = threading.Lock()
    router.path_request_lock = threading.Lock()
    router.path_request_times = {}
    router.outbound_stamp_costs = {}
    router.pending_outbound = []
    router.pending_deferred_stamps = {}
    router.get_outbound_ticket = lambda _: None
    router.direct_links = {}
    router.backchannel_links = {}
    router.delivery_destinations = {}
    router.outbound_propagation_link = None
    relay = RNS.Destination(recipient.identity, RNS.Destination.OUT,
                            RNS.Destination.SINGLE, "lxmf", "propagation")
    router.outbound_propagation_node = relay.hash if spec["mode"] == "propagated" else None
    now, path = [1000.0], [spec.get("path", True)]
    medium, bitrate, first = spec.get("medium", 8), spec.get("bitrate", 2000), spec.get("first", 6)
    actions, links, receipts, resources, transmissions, rows = [], [], [], [], [], []
    link_type, resource_type = RNS.Link, RNS.Resource
    state_names = {getattr(LXMF.LXMessage, key): key.lower() for key in
                   ["OUTBOUND", "SENDING", "SENT", "DELIVERED", "REJECTED", "CANCELLED", "FAILED"]}
    link_names = {getattr(link_type, key): key.lower() for key in ["PENDING", "HANDSHAKE", "ACTIVE", "CLOSED"]}

    class ControlledLink:
        PENDING, HANDSHAKE, ACTIVE, CLOSED = link_type.PENDING, link_type.HANDSHAKE, link_type.ACTIVE, link_type.CLOSED

        def __init__(self, destination, established_callback=None, label=None):
            self.destination, self.status = destination, self.PENDING
            self.link_id = bytes([len(links) + 1]) * 16
            self.hash, self.type, self.mtu = self.link_id, RNS.Destination.LINK, 500
            self.initiator, self.activated_at, self.established_callback = True, None, established_callback
            self.label = label or f"outbound-{len(links) + 1}"
            links.append(self)
            actions.append(f"create:{self.label}")

        def set_link_established_callback(self, callback):
            self.established_callback = callback

        def set_packet_callback(self, callback):
            self.packet_callback = callback

        def teardown(self):
            self.status = self.CLOSED
            actions.append(f"teardown:{self.label}")

    class ControlledResource:
        COMPLETE, REJECTED, FAILED = resource_type.COMPLETE, resource_type.REJECTED, resource_type.FAILED

        def __init__(self, data, link, callback, progress_callback, **options):
            self.link, self.callback, self.status = link, callback, resource_type.TRANSFERRING
            resources.append(self)
            transmissions.append(("resource", link, bytes(data)))
            actions.append(f"resource:{link.label}")

    class ControlledThread:
        def __init__(self, target, daemon):
            if target != router.process_outbound or not daemon:
                raise RuntimeError("unexpected reference worker")
            self.target = target

        def start(self):
            self.target()

    method = getattr(LXMF.LXMessage, spec["mode"].upper())
    message = LXMF.LXMessage(recipient, source,
                             content=b"x" * (1024 if spec.get("large") else 12), desired_method=method)
    message.timestamp = 1773999000.25
    message.defer_stamp = message.defer_propagation_stamp = False
    message.register_failed_callback(lambda _: actions.append("failed_callback"))
    message.register_delivery_callback(lambda _: actions.append("delivery_callback"))

    messages = [message]
    if spec.get("shared"):
        other_recipient = recipient
        if spec["mode"] == "propagated":
            other_recipient = RNS.Destination(RNS.Identity.from_bytes(bytes([0x33]) * 64),
                RNS.Destination.OUT, RNS.Destination.SINGLE, "lxmf", "delivery")
        other = LXMF.LXMessage(other_recipient, source, content=b"second shared message", desired_method=method)
        other.timestamp = message.timestamp + 1
        other.defer_stamp = other.defer_propagation_stamp = False
        other.register_delivery_callback(lambda _: actions.append("second_delivery_callback"))
        other.register_failed_callback(lambda _: actions.append("second_failed_callback"))
        messages.append(other)

    def packet_send(packet):
        link = packet.destination
        actions.append(f"packet:{link.label}")
        transmissions.append(("packet", link, bytes(packet.data)))
        if spec.get("effect") == "raise":
            raise RuntimeError("injected packet send failure")
        if spec.get("effect") == "none":
            return None
        receipt = SimpleNamespace(destination=link)
        receipt.set_delivery_callback = lambda callback: setattr(receipt, "delivered", callback)
        receipt.set_timeout_callback = lambda callback: setattr(receipt, "timed_out", callback)
        receipts.append(receipt)
        return receipt

    def selected_link():
        if spec["mode"] == "propagated":
            return router.outbound_propagation_link
        return router.direct_links.get(recipient.hash) or router.backchannel_links.get(recipient.hash)

    def snapshot(event):
        selected = selected_link()
        row = dict(event=event, time=now[0], path_known=path[0],
                   medium_timeout=medium, lowest_bitrate=bitrate, first_hop=first,
                   attempts=message.delivery_attempts, max_attempts=router.attempt_limit(message),
                   deadline=getattr(message, "next_delivery_attempt", None),
                   awaiting_path=message.awaiting_path,
                   path_request_retried=getattr(message, "path_request_retried", False),
                   state=state_names[message.state], pending=message in router.pending_outbound,
                   selected_link=selected.label if selected else None,
                   delivery_link=getattr(message._LXMessage__delivery_destination, "label", None),
                   links=[dict(label=link.label, status=link_names[link.status],
                               activated=link.activated_at is not None,
                               creator=next((i for i, item in enumerate(messages) if getattr(link, "for_lxmessage", None) is item), None)) for link in links],
                   actions=actions.copy(), receipt_count=len(receipts),
                   transmission_count=len(transmissions), resource_count=len(resources),
                   request_time=router.path_request_times.get(relay.hash if spec["mode"] == "propagated" else recipient.hash))
        if spec.get("shared"):
            row["messages"] = [dict(state=state_names[item.state], attempts=item.delivery_attempts,
                                    pending=item in router.pending_outbound,
                                    destination=item.destination_hash.hex()) for item in messages]
        for field in ["time", "deadline", "request_time"]:
            value = row[field]
            row[field + "_bits"] = None if value is None else int.from_bytes(struct.pack(">d", float(value)), "big")
        rows.append(row)
        actions.clear()

    instance = SimpleNamespace(get_medium_path_timeout=lambda: medium,
                               get_lowest_interface_bitrate=lambda: bitrate,
                               get_first_hop_timeout=lambda _: first,
                               _retain_destination_data=lambda _: actions.append("retain_destination"))
    with ExitStack() as stack:
        stack.enter_context(patch.object(RNS.Reticulum, "get_instance", return_value=instance))
        stack.enter_context(patch.object(RNS.Transport, "has_path", side_effect=lambda _: path[0]))
        stack.enter_context(patch.object(RNS.Transport, "request_path", side_effect=lambda _: actions.append("request_path")))
        stack.enter_context(patch.object(RNS.Identity, "recall", return_value=recipient.identity))
        stack.enter_context(patch.object(RNS.Identity, "recall_app_data", return_value=None))
        stack.enter_context(patch.object(RNS, "Link", ControlledLink))
        stack.enter_context(patch.object(RNS, "Resource", ControlledResource))
        stack.enter_context(patch.object(RNS.Packet, "send", new=packet_send))
        stack.enter_context(patch("time.time", side_effect=lambda: now[0]))
        stack.enter_context(patch("threading.Thread", ControlledThread))
        initial = spec.get("initial")
        if initial:
            destination = relay if spec["mode"] == "propagated" else recipient
            if initial != "backchannel":
                link = ControlledLink(destination, label="existing-outbound")
                if initial != "pending_and_backchannel":
                    link.status, link.activated_at = link.ACTIVE, now[0] - 1
                if spec["mode"] == "propagated": router.outbound_propagation_link = link
                else: router.direct_links[recipient.hash] = link
            if initial in ["backchannel", "both_active", "pending_and_backchannel"]:
                back = ControlledLink(recipient, label="existing-backchannel")
                back.status, back.activated_at, back.initiator = back.ACTIVE, now[0] - 1, False
                router.backchannel_links[recipient.hash] = back
            actions.clear()
        for item in messages:
            router.handle_outbound(item)
        snapshot({"admit": True})
        prepared = [(bytes(item.packed), item.propagation_packed) for item in messages]
        for event in spec["events"]:
            now[0] = 1000.0 + event["at"]
            if "path" in event: path[0] = event["path"]
            if "medium" in event: medium = event["medium"]
            if "bitrate" in event: bitrate = event["bitrate"]
            if "first" in event: first = event["first"]
            if event.get("admit_new"):
                third_recipient = RNS.Destination(RNS.Identity.from_bytes(bytes([0x44]) * 64),
                    RNS.Destination.OUT, RNS.Destination.SINGLE, "lxmf", "delivery")
                third = LXMF.LXMessage(third_recipient, source, content=b"third shared message", desired_method=method)
                third.timestamp = message.timestamp + 2
                third.defer_stamp = third.defer_propagation_stamp = False
                third.register_delivery_callback(lambda _: actions.append("third_delivery_callback"))
                third.register_failed_callback(lambda _: actions.append("third_failed_callback"))
                messages.append(third)
                router.handle_outbound(third)
                prepared.append((bytes(third.packed), third.propagation_packed))
            if "signal" in event:
                link = links[event.get("signal_link", 0)]
                link.packet_callback(bytes.fromhex(event["signal"]), SimpleNamespace(link=link))
            if event.get("cancel"): message.state = LXMF.LXMessage.CANCELLED
            if event.get("activate"):
                link = selected_link() or links[-1]
                link.status, link.activated_at = link.ACTIVE, now[0]
                if link.established_callback:
                    actions.append("activation_callback")
                    link.established_callback(link)
            if event.get("close"):
                selected_link().status = ControlledLink.CLOSED
            if "proof" in event:
                receipt = receipts[event["proof"]]
                receipt.delivered(receipt)
            if "timeout" in event:
                receipt = receipts[event["timeout"]]
                receipt.timed_out(receipt)
            if "resource" in event:
                resource = resources[-1]
                resource.status = getattr(ControlledResource, event["resource"].upper())
                resource.callback(resource)
            if event.get("process", True):
                router.process_outbound()
            snapshot(event)
            # Outside router exception handlers: a swallowed callback/send
            # AssertionError must never make a corrupted trace look successful.
            for item, (signed, propagated) in zip(messages, prepared):
                assert bytes(item.packed) == signed, spec["name"]
                assert item.propagation_packed == propagated, spec["name"]
    expected = [propagated if spec["mode"] == "propagated" else signed for signed, propagated in prepared]
    for _, _, data in transmissions:
        assert data in expected, f"{spec['name']}: transmission changed prepared bytes"
    if spec.get("shared"):
        assert len(transmissions) == len(expected) and sorted(data for _, _, data in transmissions) == sorted(expected), spec["name"]
    if spec.get("large"):
        assert message.representation == LXMF.LXMessage.RESOURCE, spec["name"]
    else:
        assert message.representation == LXMF.LXMessage.PACKET, spec["name"]
    return dict(name=spec["name"], initial_links=spec.get("initial"), initial_messages=2 if spec.get("shared") else 1, requested_method=spec["mode"],
                effective_method="propagated" if message.method == LXMF.LXMessage.PROPAGATED else "direct",
                representation="resource" if message.representation == LXMF.LXMessage.RESOURCE else "packet",
                initial_path_known=spec.get("path", True), medium_timeout=spec.get("medium", 8),
                lowest_bitrate=spec.get("bitrate", 2000), first_hop=spec.get("first", 6), send_effect=spec.get("effect", "receipt"),
                signed_wire_hex=[signed.hex() for signed, _ in prepared],
                prepared_transfer_bytes=[len(value) for value in expected], rows=rows)


def _validate_traces(traces):
    """Check scenario intent outside production exception handlers.

    The snapshots still come from reference code; these expectations prevent an
    unexercised seam or swallowed exception from silently producing a new oracle.
    """
    by_name = {case["name"]: case for case in traces}
    assert len(by_name) == 47
    for mode in ["direct", "propagated"]:
        complete = "delivered" if mode == "direct" else "sent"
        for speed, limit, requests in [("normal", 5, 4), ("slow", 3, 2)]:
            rows = by_name[f"{mode}_{speed}_pathless_exhaustion"]["rows"]
            assert rows[-1]["state"] == "failed" and not rows[-1]["pending"]
            assert rows[-1]["attempts"] == limit
            assert sum(row["actions"].count("request_path") for row in rows) == requests
            assert all(row["transmission_count"] == 0 and not row["links"] for row in rows)
        rows = by_name[f"{mode}_pending_then_late_activation"]["rows"]
        assert all(row["attempts"] == 1 and row["transmission_count"] == 0 for row in rows[:4])
        assert rows[-1]["state"] == complete and rows[-1]["transmission_count"] == 1
        rows = by_name[f"{mode}_packet_timeout_retry"]["rows"]
        assert rows[-1]["state"] == complete and rows[-1]["attempts"] == 3
        assert rows[-1]["transmission_count"] == 3
        assert sum(row["actions"].count("request_path") for row in rows) == (2 if mode == "direct" else 0)
        rows = by_name[f"{mode}_never_activated_closure_exhaustion"]["rows"]
        assert rows[-1]["state"] == "failed" and rows[-1]["attempts"] == 5
        assert sum(row["actions"].count("request_path") for row in rows) == (1 if mode == "direct" else 0)
        for suffix in ["pending_then_late_activation", "packet_timeout_retry", "path_appears_before_deadline",
                       "late_proof_after_timeout", "proof_after_cancel", "resource_complete", "resource_failed"]:
            row = by_name[f"{mode}_{suffix}"]["rows"][-1]
            assert row["state"] == complete and not row["pending"], (mode, suffix)
        rows = by_name[f"{mode}_resource_rejected"]["rows"]
        assert rows[-1]["state"] == ("rejected" if mode == "direct" else "sent")
        assert rows[-1]["transmission_count"] == (1 if mode == "direct" else 2)
        rows = by_name[f"{mode}_send_raises"]["rows"]
        assert rows[-1]["state"] == "failed" and rows[-1]["transmission_count"] == 1
        assert sum(action.startswith("packet:") for row in rows for action in row["actions"]) == 1
        assert sum(row["actions"].count("failed_callback") for row in rows) == 1
        rows = by_name[f"{mode}_cancel_pending"]["rows"]
        assert rows[-1]["state"] == "cancelled" and not rows[-1]["pending"]
        assert rows[-1]["transmission_count"] == 0
        assert sum(row["actions"].count("failed_callback") for row in rows) == 1
        rows = by_name[f"{mode}_slow_known_closure_exhaustion"]["rows"]
        assert rows[-1]["attempts"] == 3 and rows[-1]["state"] == "failed"
        assert len(rows[-1]["links"]) == 2 and rows[-1]["transmission_count"] == 0
        rows = by_name[f"{mode}_changed_medium_after_closure"]["rows"]
        assert rows[2]["attempts"] == 1 and rows[2]["max_attempts"] == 3
        assert rows[-1]["attempts"] == 2 and rows[-1]["state"] == complete
        rows = by_name[f"{mode}_external_close_while_sending"]["rows"]
        assert rows[-1]["state"] == "sending" and rows[-1]["pending"]
        assert rows[-1]["transmission_count"] == 1 and len(rows[-1]["links"]) == 2
        rows = by_name[f"{mode}_shared_link_two_messages"]["rows"]
        assert len(rows[-1]["links"]) == 1 and rows[-1]["transmission_count"] == 2
        assert all(item["state"] == complete and not item["pending"] for item in rows[-1]["messages"])
        if mode == "propagated":
            assert len({item["destination"] for item in rows[-1]["messages"]}) == 2
        # This is a reference observation, not a claim of successful recovery:
        # no receipt closes the Link without resetting LXMessage.SENDING.
        row = by_name[f"{mode}_packet_no_receipt"]["rows"][-1]
        assert row["state"] == "sending" and row["pending"] and row["transmission_count"] == 1
        assert by_name[f"{mode}_resource_failure_after_cancel"]["rows"][-1]["state"] == "cancelled"
    for name, selected in [("direct_outbound_before_backchannel", "existing-outbound"),
                           ("direct_backchannel_only", "existing-backchannel")]:
        row = by_name[name]["rows"][0]
        assert row["delivery_link"] == selected and row["attempts"] == 0
        assert row["transmission_count"] == 1
    rows = by_name["direct_pending_before_active_backchannel"]["rows"]
    assert rows[1]["transmission_count"] == 0 and rows[2]["delivery_link"] == "existing-outbound"
    escalated = by_name["oversized_opportunistic_uses_direct_resource"]
    assert escalated["effective_method"] == "direct" and escalated["representation"] == "resource"
    assert escalated["rows"][-1]["state"] == "delivered"

    for name in ["active_creator", "sent_pending_creator", "malformed_then_valid"]:
        rows = by_name[f"propagated_signal_{name}"]["rows"]
        assert rows[-1]["messages"][0]["state"] == "rejected"
        assert rows[-1]["messages"][1]["state"] == "sent"
        assert rows[-1]["links"][0]["creator"] == 0
        assert sum(row["actions"].count("failed_callback") for row in rows) == 1
        assert sum(row["actions"].count("second_failed_callback") for row in rows) == 0
    rows = by_name["propagated_signal_retired_creator"]["rows"]
    assert all(item["state"] == "sent" and not item["pending"] for item in rows[-1]["messages"])
    assert all("failed_callback" not in row["actions"] for row in rows)
    pending = by_name["propagated_signal_sent_pending_creator"]["rows"][2]["messages"][0]
    assert pending["state"] == "sent" and pending["pending"]
    rows = by_name["propagated_signal_replacement_creator"]["rows"]
    assert [link["creator"] for link in rows[-1]["links"]] == [0, 2]
    assert rows[-2]["messages"][2]["state"] == "sending"
    assert rows[-1]["messages"][2]["state"] == "rejected"
    rows = by_name["propagated_signal_malformed_then_valid"]["rows"]
    assert all(row["messages"][0]["state"] == "sending" for row in rows[2:-2])
