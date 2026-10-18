"""Assertions shared by the live LXMF compatibility smoke lanes."""

import json
from pathlib import Path
import sys


def merge_event_page(page, retained):
    assert page.get("dropped_count", 0) == 0, "SDK event history was truncated"
    assert not any(row["event_type"] == "StreamGap" for row in page["events"]), "SDK stream gap"
    if retained:
        for field in ("runtime_id", "stream_id"):
            assert page[field] == retained[field], "SDK event stream changed"
        previous = {row["event_id"] for row in retained["events"]}
        assert not any(row["event_id"] in previous for row in page["events"]), "replayed event page"
        assert not page["events"] or page["next_cursor"] != retained["next_cursor"], "SDK cursor stalled"
        page["events"] = retained["events"] + page["events"]
    return page


def native_completion(batch, message_id, status, kind, method, peer, link_mdu=None):
    receipts = [row["payload"] for row in batch["events"]
                if row["event_type"] == "receipt"
                and row["payload"].get("message_id") == message_id
                and row["payload"].get("updated") is True]
    observed_statuses = [row["status"].strip().lower() for row in receipts]
    assert not any(status.startswith("failed") or status in {"rejected", "cancelled", "expired"}
                   for status in observed_statuses), receipts
    statuses = {"sent: propagated", "sent: propagated resource"} if status == "custody" else {status}
    completed = [row for row in receipts if row["status"] in statuses]
    assert len(completed) == 1, receipts
    receipt = completed[0]
    if kind == "auto":
        assert method == "propagation" and link_mdu is not None
        resource = receipt["bytes"] > link_mdu
        kind = "link-resource" if resource else "link-packet"
        expected = "sent: propagated resource" if resource else "sent: propagated"
        assert receipt["status"] == expected, receipt
    assert receipt["delivery_kind"] == kind, receipt
    assert receipt["method"] == method and receipt["peer"] == peer, receipt
    hash_field = "resource_hash" if kind == "link-resource" else "packet_hash"
    assert len(bytes.fromhex(receipt[hash_field])) == 32, receipt
    assert receipt["bytes"] > 0, receipt
    return receipt


def propagation_metadata(raw, implementation, version):
    # Validate with the actual selected Python reference, including the old pin.
    import LXMF
    from RNS.vendor import umsgpack

    assert raw is not None and LXMF.pn_announce_data_is_valid(raw), raw
    data = umsgpack.unpackb(raw)
    metadata = data[6]
    assert isinstance(metadata, dict), metadata
    if implementation is None:
        assert 0xFE not in metadata and 0x00 not in metadata, metadata
    else:
        assert metadata[0xFE] == implementation, metadata
        assert metadata[0x00] == version, metadata
    return {
        "app_data_hex": raw.hex(), "implementation": metadata.get(0xFE),
        "version": metadata.get(0x00), "stamp_costs": data[5],
    }


if __name__ == "__main__":
    if sys.argv[1] == "merge-events":
        page_path, aggregate_path = map(Path, sys.argv[2:4])
        page = json.loads(page_path.read_text())
        count = len(page["events"])
        retained = json.loads(aggregate_path.read_text()) if aggregate_path.is_file() else None
        aggregate = merge_event_page(page, retained)
        aggregate_path.write_text(json.dumps(aggregate) + "\n")
        print(aggregate["next_cursor"], count)
    else:
        import RNS

        path, message_id, status, kind, method, peer = sys.argv[1:7]
        print(json.dumps(native_completion(json.loads(Path(path).read_text()),
                                           message_id, status, kind, method, peer, RNS.Link.MDU)))
