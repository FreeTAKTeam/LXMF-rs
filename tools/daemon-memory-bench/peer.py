#!/usr/bin/env python3
"""Disposable LXMF sender and offline receiver for the daemon smoke test."""
import hashlib
import ctypes
import json
import os
import signal
import sys
import threading
import time
from pathlib import Path

import LXMF
import RNS


class CaptureError(Exception):
    pass


def record_inbound_queue(record, stage):
    total, heights, dropped = RNS.Transport.inbound_queues.snapshot()
    record("inbound_queue", stage=stage, total=total, heights=heights, dropped=dropped)


def fence_router_callbacks(router, guard, record, errors, active_link):
    for name in ("message_list_response", "message_get_response", "message_get_failed", "message_get_progress"):
        callback = getattr(router, name)
        def fenced(receipt, callback=callback, name=name):
            with guard:
                try:
                    if active_link[0] is False:
                        return
                    if receipt.link is not None and receipt.link is active_link[0] and receipt.link is router.outbound_propagation_link:
                        return callback(receipt)
                    record("poll_stale_callback", callback=name)
                except Exception as error:
                    errors.append(f"poll callback {name} failed: {error}")
        setattr(router, name, fenced)
    clean_links = router.clean_links
    def guarded_clean_links():
        with guard:
            return clean_links()
    router.clean_links = guarded_clean_links


def begin_retry_link(router, receipts, submission_errors):
    node = router.outbound_propagation_node
    identity = RNS.Identity.recall(node)
    if not RNS.Transport.has_path(node) or identity is None:
        RNS.Transport.request_path(node)
        return False
    destination = RNS.Destination(identity, RNS.Destination.OUT, RNS.Destination.SINGLE, "lxmf", "propagation")
    router.propagation_transfer_state = LXMF.LXMRouter.PR_LINK_ESTABLISHING
    router.outbound_propagation_link = RNS.Link(destination)
    link = router.outbound_propagation_link
    request = link.request
    def track_request(*args, **kwargs):
        receipt = request(*args, **kwargs)
        if receipt is None or receipt is False:
            submission_errors.append(True)
        else:
            receipts.append(receipt)
        return receipt
    link.request = track_request
    return True


def issue_retry_poll(router, link, identity):
    router.request_messages_from_propagation_node(identity)


def payload(sequence, size, seed):
    prefix = f"{seed}:{sequence:06d}:".encode()
    assert len(prefix) <= size
    padding = hashlib.sha256(prefix).hexdigest().encode()
    return (prefix + padding * ((size // len(padding)) + 1))[:size]


def observe(pending, record):
    count = 0
    for number, message in list(pending.items()):
        if message.state in (LXMF.LXMessage.SENT, LXMF.LXMessage.DELIVERED, LXMF.LXMessage.FAILED,
                             LXMF.LXMessage.REJECTED, LXMF.LXMessage.CANCELLED):
            record("sender_final", sequence=number, state=message.state, message_hash=message.hash.hex(),
                   transient_id=message.transient_id.hex() if message.transient_id is not None else None)
            del pending[number]
            count += 1
    return count


class ReceiverProgress:
    def __init__(self, record):
        self.record = record
        self.lock = threading.Lock()
        self.count = 0
        self.last_received = None
        self.failure = None
        self.finished = False

    def receive(self, message):
        with self.lock:
            try:
                content = message.content if isinstance(message.content, bytes) else message.content.encode()
                self.record("received", sha256=hashlib.sha256(content).hexdigest(),
                            message_hash=message.hash.hex(), source=message.source_hash.hex(),
                            destination=message.destination_hash.hex())
                self.count += 1
                self.last_received = time.monotonic()
            except Exception as error:
                self.failure = f"receiver callback could not record delivery: {type(error).__name__}: {error}"

    def finish_if_quiet(self, expected, quiet_seconds, deadline, router):
        with self.lock:
            if self.failure:
                raise CaptureError(self.failure)
            now = time.monotonic()
            if not self.finished and self.count >= expected and now <= deadline and now >= self.last_received + quiet_seconds:
                self.record("receiver_complete", count=self.count, sync_state=router.propagation_transfer_state,
                            quiet_interval_completed=True)
                self.finished = True
            return self.finished

    def finish_timeout(self, router):
        with self.lock:
            if self.failure:
                raise CaptureError(self.failure)
            self.record("receiver_complete", count=self.count, sync_state=router.propagation_transfer_state,
                        quiet_interval_completed=False)


def wait_normal_start(root, timeout_seconds):
    barrier = root / "normal-start.json"
    deadline = time.monotonic() + timeout_seconds
    while not barrier.exists():
        if time.monotonic() >= deadline:
            raise CaptureError("normal traffic start barrier timed out")
        time.sleep(0.1)
    start = json.loads(barrier.read_text())["start_monotonic_ns"]
    if type(start) is not int or start <= time.monotonic_ns():
        raise CaptureError("normal traffic start barrier is invalid or late")
    return start / 1e9


def normal_receive(router, identity, receiver, record, spec, start, root=None, poll_lock=None, callback_errors=None, active_link=None):
    end = start + spec["duration_seconds"]
    deadline = end + spec["receiver_drain_seconds"]
    interval = spec["receiver_poll_interval_seconds"]
    next_poll, slot, active_slot, poll_issued = start, 0, None, False
    active_poll_started = last_completed_poll_started = None
    sender_done = None
    sender_done_observed = None
    next_queue_sample = start
    poll_receipts, poll_submission_errors = [], []
    poll_lock = poll_lock or threading.RLock()
    callback_errors = callback_errors if callback_errors is not None else []
    active_link = active_link if active_link is not None else [None]
    while time.monotonic() < deadline:
        now = time.monotonic()
        with poll_lock:
            if callback_errors:
                raise CaptureError(callback_errors[0])
        if spec.get("generator_mode") == "prepared_packets" and now >= next_queue_sample:
            record_inbound_queue(record, "periodic")
            next_queue_sample += 5
        with poll_lock:
            if callback_errors:
                raise CaptureError(callback_errors[0])
            now = time.monotonic()
            link = router.outbound_propagation_link
            timeout = spec.get("receiver_poll_timeout_seconds")
            if active_slot is not None and timeout is not None and not poll_issued and link is not None and link.status == RNS.Link.ACTIVE:
                issue_retry_poll(router, link, identity)
                poll_issued = True
            if (active_slot is not None and (timeout is None or poll_issued)
                    and router.propagation_transfer_state == LXMF.LXMRouter.PR_COMPLETE
                    and link is not None and link.status == RNS.Link.ACTIVE and not link.pending_requests
                    and (timeout is None or now - active_poll_started <= timeout)
                    and (timeout is None or (poll_receipts and all(receipt.status == RNS.RequestReceipt.READY for receipt in poll_receipts)))):
                record("poll_complete", slot=active_slot, sync_state=router.propagation_transfer_state,
                       link_status=link.status, pending_requests=len(link.pending_requests),
                       request_statuses=[receipt.status for receipt in poll_receipts] if timeout is not None else None)
                last_completed_poll_started = active_poll_started
                active_slot = None
                active_link[0] = None
            if active_slot is not None and timeout is not None:
                terminal = router.propagation_transfer_state >= LXMF.LXMRouter.PR_NO_PATH
                closed = link is not None and link.status == RNS.Link.CLOSED
                request_failed = any(receipt.status == RNS.RequestReceipt.FAILED for receipt in poll_receipts)
                if terminal or closed or request_failed or poll_submission_errors or now - active_poll_started >= timeout:
                    observation_late = (now - active_poll_started >= timeout and poll_receipts
                                        and all(receipt.status == RNS.RequestReceipt.READY for receipt in poll_receipts)
                                        and router.propagation_transfer_state == LXMF.LXMRouter.PR_COMPLETE)
                    reason = ("terminal" if terminal else "link_closed" if closed else "request_failed" if request_failed
                              else "submission" if poll_submission_errors else "observation_late" if observation_late else "timeout")
                    record("poll_failed", slot=active_slot, reason=reason,
                           sync_state=router.propagation_transfer_state,
                           link_status=link.status if link is not None else None,
                           pending_requests=len(link.pending_requests) if link is not None else 0,
                           request_statuses=[receipt.status for receipt in poll_receipts],
                           submission_failures=len(poll_submission_errors))
                    active_link[0] = None
                    router.cancel_propagation_node_requests()
                    active_slot = None
                    active_poll_started = None
        if now >= next_poll:
            late = now - next_poll > spec.get("maximum_poll_lateness_seconds", spec["maximum_schedule_lateness_seconds"])
            if active_slot is not None or late:
                record("poll_missed", slot=slot, scheduled_monotonic_ns=int(next_poll * 1e9),
                       previous_pending=active_slot is not None, schedule_late=late,
                       sync_state=router.propagation_transfer_state,
                       link_status=link.status if link is not None else None,
                       pending_requests=len(link.pending_requests) if link is not None else 0)
                if late:
                    receiver.finish_timeout(router)
                    return
            else:
                active_poll_started = time.monotonic()
                record("poll_attempt", slot=slot, scheduled_monotonic_ns=int(next_poll * 1e9),
                       poll_started_monotonic_ns=int(active_poll_started * 1e9))
                active_slot = slot
                with poll_lock:
                    if timeout is None:
                        router.request_messages_from_propagation_node(identity)
                    else:
                        poll_issued = False
                        poll_receipts, poll_submission_errors = [], []
                        active_link[0] = None
                        if router.outbound_propagation_link is not None:
                            router.cancel_propagation_node_requests()
                        if not begin_retry_link(router, poll_receipts, poll_submission_errors):
                            record("poll_failed", slot=active_slot, reason="route", sync_state=router.propagation_transfer_state,
                                   link_status=None, pending_requests=0, request_statuses=[], submission_failures=0)
                            active_slot = None
                            active_poll_started = None
                        else:
                            active_link[0] = router.outbound_propagation_link
            slot += 1
            next_poll = start + slot * interval
        if spec.get("generator_mode") == "prepared_packets" and sender_done is None and (root / "sender-complete.json").exists():
            sender_done_ns = json.loads((root / "sender-complete.json").read_text())["monotonic_ns"]
            sender_done = sender_done_ns / 1e9
            record("sender_done_observed", sender_done_ns=sender_done_ns)
            sender_done_observed = time.monotonic()
        fresh_poll = True
        if spec.get("generator_mode") == "prepared_packets":
            with receiver.lock:
                last_received = receiver.last_received
            fresh_poll = (sender_done is not None and last_completed_poll_started is not None
                          and last_completed_poll_started >= sender_done_observed
                          and (last_received is None or last_completed_poll_started >= last_received))
        if now >= end and fresh_poll and active_slot is None:
            with poll_lock:
                if callback_errors:
                    raise CaptureError(callback_errors[0])
                if (router.propagation_transfer_state == LXMF.LXMRouter.PR_COMPLETE
                        and receiver.finish_if_quiet(spec["message_count"], spec["receiver_quiet_seconds"], deadline, router)):
                    if spec.get("generator_mode") == "prepared_packets":
                        record_inbound_queue(record, "final")
                    active_link[0] = False
                    return
        time.sleep(0.1)
    with poll_lock:
        if callback_errors:
            raise CaptureError(callback_errors[0])
        if active_slot is not None and spec.get("receiver_poll_timeout_seconds") is not None:
            link = router.outbound_propagation_link
            record("poll_failed", slot=active_slot, reason="drain_deadline",
                   sync_state=router.propagation_transfer_state,
                   link_status=link.status if link is not None else None,
                   pending_requests=len(link.pending_requests) if link is not None else 0,
                   request_statuses=[receipt.status for receipt in poll_receipts],
                   submission_failures=len(poll_submission_errors))
            active_link[0] = None
            router.cancel_propagation_node_requests()
        if spec.get("generator_mode") == "prepared_packets":
            record_inbound_queue(record, "final")
        receiver.finish_timeout(router)
        active_link[0] = False


def main():
    expected_parent = int(sys.argv[3])
    if ctypes.CDLL(None, use_errno=True).prctl(1, signal.SIGTERM) != 0:
        raise OSError(ctypes.get_errno(), "failed to register parent-death signal")
    if os.getppid() != expected_parent:
        raise RuntimeError("runner exited before peer startup")
    root = Path(sys.argv[1])
    role = sys.argv[2]
    if role not in ("sender", "receiver"):
        raise SystemExit("expected sender or receiver")
    spec = json.loads((root / "experiment.json").read_text())
    fixture = root / "fixture"
    node = RNS.Identity.from_file(str(fixture / "node.key"))
    identity = RNS.Identity.from_file(str(fixture / (role + ".key")))
    assert node is not None and identity is not None
    node_hash = RNS.Destination.hash_from_name_and_identity("lxmf.propagation", node)
    RNS.Reticulum(configdir=str(root / role / "rns"), loglevel=0)
    router = LXMF.LXMRouter(storagepath=str(root / role / "store"), enforce_stamps=False)
    local = router.register_delivery_identity(identity, display_name="Benchmark peer")
    router.set_outbound_propagation_node(node_hash)
    lock = threading.Lock()
    try:
        ledger = (root / (role + ".jsonl")).open("a", buffering=1)
    except OSError as error:
        raise CaptureError(f"could not open {role} ledger: {error}") from error

    def record(event, **values):
        try:
            with lock:
                ledger.write(json.dumps({"event": event, "monotonic_ns": time.monotonic_ns(), **values}, sort_keys=True) + "\n")
        except (OSError, ValueError) as error:
            raise CaptureError(f"could not write {role} ledger: {error}") from error

    if spec.get("generator_mode") == "prepared_packets":
        record_inbound_queue(record, "baseline")

    receiver = ReceiverProgress(record)
    poll_lock, callback_errors, active_link = threading.RLock(), [], [None]
    if role == "receiver":
        router.register_delivery_callback(receiver.receive)
        if spec.get("receiver_poll_timeout_seconds") is not None:
            fence_router_callbacks(router, poll_lock, record, callback_errors, active_link)
    normal = spec["profile"] in ("normal", "high-load", "preflight-only-normal-dry-run")
    deadline = time.monotonic() + spec["route_deadline_seconds"]
    if normal:
        record("node_discovery_start", node=node_hash.hex())
    while time.monotonic() < deadline:
        if RNS.Transport.has_path(node_hash) and RNS.Identity.recall(node_hash) is not None:
            break
        RNS.Transport.request_path(node_hash)
        time.sleep(1)
    else:
        if normal:
            record("node_timeout", node=node_hash.hex())
            raise SystemExit(71)
        raise SystemExit("propagation node route and identity not discovered")
    record("node_ready", node=node_hash.hex())
    shared_start = wait_normal_start(root, spec["route_deadline_seconds"] * 2 + 30) if normal else None

    if role == "sender":
        recipient = RNS.Identity(create_keys=False)
        assert recipient.load_public_key((fixture / "receiver.pub").read_bytes())
        destination = RNS.Destination(recipient, RNS.Destination.OUT, RNS.Destination.SINGLE, "lxmf", "delivery")
        start = shared_start if normal else time.monotonic() + 1
        pending = {}
        attempted = 0
        final = 0

        for number in range(spec["message_count"]):
            scheduled = start + number * spec["interval_seconds"]
            while time.monotonic() < scheduled:
                final += observe(pending, record)
                time.sleep(min(0.25, max(0.0, scheduled - time.monotonic())))
            content = payload(number, spec["payload_bytes"], spec["seed"])
            message = LXMF.LXMessage(destination, local, content=content, desired_method=LXMF.LXMessage.PROPAGATED)
            record("attempted", sequence=number, scheduled_monotonic_ns=int(scheduled * 1e9),
                   sha256=hashlib.sha256(content).hexdigest())
            pending[number] = message
            attempted += 1
            router.handle_outbound(message)
        end = start + spec["duration_seconds"]
        while time.monotonic() < end:
            final += observe(pending, record)
            time.sleep(min(0.25, max(0.0, end - time.monotonic())))
        deadline = time.monotonic() + spec["sender_drain_seconds"]
        while pending and time.monotonic() < deadline:
            final += observe(pending, record)
            time.sleep(0.25)
        final += observe(pending, record)
        record("sender_complete", attempted=attempted, final=final)
    else:
        if normal:
            normal_receive(router, identity, receiver, record, spec, shared_start, root, poll_lock, callback_errors, active_link)
            return
        router.request_messages_from_propagation_node(identity)
        deadline = time.monotonic() + spec["receiver_drain_seconds"]
        completed = False
        while time.monotonic() < deadline:
            if receiver.finish_if_quiet(spec["message_count"], spec["receiver_quiet_seconds"], deadline, router):
                completed = True
                break
            time.sleep(0.25)
        if not completed:
            receiver.finish_timeout(router)


if __name__ == "__main__":
    try:
        main()
    except CaptureError as error:
        print(f"BENCH_CAPTURE_ERROR: {error}", file=sys.stderr, flush=True)
        raise SystemExit(70)
