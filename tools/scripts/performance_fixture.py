"""Exact-size benchmark content and opportunistic delivery evidence checks."""

from __future__ import annotations

import sys
from typing import Any


def payload_content(marker: str, size: int) -> str:
    if size == 0:
        return marker
    prefix = marker + ":"
    prefix_size = len(prefix.encode("utf-8"))
    if size < prefix_size:
        raise ValueError(f"payload size {size} cannot contain the {prefix_size}-byte smoke marker")
    return prefix + "x" * (size - prefix_size)


def validate_opportunistic_sender(sender: dict[str, Any]) -> None:
    assert sender["method"] == 1, (
        f"expected OPPORTUNISTIC delivery (1), observed method {sender['method']}; "
        "check the encoded payload against the single-packet size limit"
    )
    assert sender["validated_proofs"], "opportunistic delivery has no validated packet proof"
    for proof in sender["validated_proofs"]:
        assert proof["proof_destination"] == proof["packet_hash"][:32], "proof reverse-routing destination mismatch"
        assert proof["proof_data_hex"][:64] == proof["packet_hash"], "proof hash mismatch"


if __name__ == "__main__":
    try:
        print(payload_content(sys.argv[1], int(sys.argv[2])))
    except (IndexError, ValueError) as error:
        raise SystemExit(f"performance_fixture: {error}") from error
