"""Test-only recorder for advertisements built or received by pinned Python RNS."""

import os
from threading import Lock

from RNS.Link import Link
from RNS.Resource import ResourceAdvertisement

_recorded_outgoing = set()
_recorder_lock = Lock()


def _record(direction, advertisement):
    path = os.environ.get("LXMF_RNCP_RESOURCE_ADVERTISEMENTS")
    if path is None:
        return
    with _recorder_lock:
        if direction == "sent":
            key = (advertisement.h, advertisement.i, advertisement.l)
            if key in _recorded_outgoing:
                return
            _recorded_outgoing.add(key)

        with open(path, "a", encoding="ascii") as output:
            output.write(
                f"{direction}\t{advertisement.t}\t{advertisement.d}\t"
                f"{str(bool(advertisement.c)).lower()}\t"
                f"{str(bool(advertisement.u)).lower()}\t"
                f"{str(bool(advertisement.p)).lower()}\n"
            )


_original_pack = ResourceAdvertisement.pack
_original_set_resource_callback = Link.set_resource_callback


def _recording_pack(self, segment=0):
    packed = _original_pack(self, segment)
    _record("sent", self)
    return packed


def _recording_set_resource_callback(self, callback):
    if callback is None:
        return _original_set_resource_callback(self, callback)

    def _recording_callback(advertisement):
        _record("received", advertisement)
        return callback(advertisement)

    return _original_set_resource_callback(self, _recording_callback)


ResourceAdvertisement.pack = _recording_pack
Link.set_resource_callback = _recording_set_resource_callback
