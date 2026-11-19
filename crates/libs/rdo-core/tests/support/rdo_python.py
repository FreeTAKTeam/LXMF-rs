"""Python Reticulum side of the RDO owner binding v1 network gate.

Usage (RNS importable via PYTHONPATH):
  rdo_python.py owner  CONFIG_DIR OWNER_PRIVATE_HEX TOKEN_HEX GENESIS_HEX EDITOR_HASH_HEX
  rdo_python.py client CONFIG_DIR CLIENT_PRIVATE_HEX DESTINATION_HEX OWNER_PUBLIC_HEX SUCCESSOR_BODY_HEX

The client starts with only the Destination. It prints one JSON line per
step; the owner serves until killed.
"""

import hashlib
import json
import os
import sys
import time

import RNS

PATH = "/rdo/1/op"


# --- deterministic CBOR subset (uint, bytes, text, array, uint-key map, null)
def enc(value):
    def head(major, n):
        if n < 24:
            return bytes([major << 5 | n])
        for size, code in ((1, 24), (2, 25), (4, 26), (8, 27)):
            if n < 1 << (8 * size):
                return bytes([major << 5 | code]) + n.to_bytes(size, "big")
        raise ValueError("integer too large")

    if value is None:
        return b"\xf6"
    if isinstance(value, bool):
        raise ValueError("bool not allowed")
    if isinstance(value, int):
        return head(0, value)
    if isinstance(value, (bytes, bytearray)):
        return head(2, len(value)) + bytes(value)
    if isinstance(value, str):
        raw = value.encode()
        return head(3, len(raw)) + raw
    if isinstance(value, list):
        return head(4, len(value)) + b"".join(enc(v) for v in value)
    if isinstance(value, dict):
        return head(5, len(value)) + b"".join(enc(k) + enc(value[k]) for k in sorted(value))
    raise ValueError(f"unsupported {type(value)}")


def dec(data):
    def item(pos):
        major, minor = data[pos] >> 5, data[pos] & 31
        pos += 1
        if major == 7:
            if minor != 22:
                raise ValueError("only null allowed")
            return None, pos
        if minor < 24:
            n = minor
        elif minor in (24, 25, 26, 27):
            size = 1 << (minor - 24)
            n = int.from_bytes(data[pos:pos + size], "big")
            pos += size
        else:
            raise ValueError("indefinite or reserved")
        if major == 0:
            return n, pos
        if major in (2, 3):
            raw = data[pos:pos + n]
            if len(raw) != n:
                raise ValueError("truncated")
            return (bytes(raw) if major == 2 else raw.decode()), pos + n
        if major == 4:
            out = []
            for _ in range(n):
                v, pos = item(pos)
                out.append(v)
            return out, pos
        if major == 5:
            out = {}
            for _ in range(n):
                k, pos = item(pos)
                v, pos = item(pos)
                out[k] = v
            return out, pos
        raise ValueError("unsupported major type")

    value, end = item(0)
    if end != len(data) or enc(value) != bytes(data):
        raise ValueError("not one deterministic item")
    return value


def emit(**fields):
    print(json.dumps(fields), flush=True)


def start(config_dir):
    RNS.Reticulum(configdir=config_dir, loglevel=RNS.LOG_WARNING)


# --- owner ----------------------------------------------------------------
def run_owner(config_dir, private_hex, token_hex, genesis_hex, editor_hash_hex):
    start(config_dir)
    identity = RNS.Identity.from_bytes(bytes.fromhex(private_hex))
    token = bytes.fromhex(token_hex)
    destination = RNS.Destination(
        identity, RNS.Destination.IN, RNS.Destination.SINGLE, "rdo", "object", token.hex()
    )
    history = [bytes.fromhex(genesis_hex)]
    retained = {}
    editor = bytes.fromhex(editor_hash_hex)

    def result(rid, status, version=None, value=None):
        return enc([1, rid, status, version, value])

    def handler(path, data, request_id, link_id, remote_identity, requested_at):
        if remote_identity is None:
            return None
        raw = bytes(data)
        try:
            request = dec(raw)
            version, rid, obj, op, expected, args, expires = request
            assert version == 1
        except Exception:
            return None
        allowed = remote_identity.hash == editor
        if not allowed:
            return result(rid, 4)
        key = (remote_identity.hash, rid)
        if key in retained:
            old_raw, old_result = retained[key]
            return old_result if old_raw == raw else result(rid, 1)
        now = int(time.time())
        head = history[-1]
        head_id = hashlib.sha256(head).digest()
        if not (now < expires <= now + 300) or obj != destination.hash:
            out = result(rid, 1)
        elif op == 0 and expected is None and args is None:
            out = result(rid, 0, head_id, head)
        elif op == 1 and expected != head_id:
            out = result(rid, 3, head_id)
        elif op == 1 and isinstance(args, dict):
            _, h_obj, _, h_type, _, _ = dec(head)
            signed = enc(["rdo.revision.v1", 1, h_obj, head_id, h_type, args])
            successor = enc([1, h_obj, head_id, h_type, args, identity.sign(signed)])
            history.append(successor)
            out = result(rid, 0, hashlib.sha256(successor).digest(), successor)
        else:
            out = result(rid, 1)
        retained[key] = (raw, out)
        return out

    destination.register_request_handler(PATH, handler, RNS.Destination.ALLOW_ALL)
    emit(step="owner_ready", destination=destination.hash.hex())
    while True:
        destination.announce(app_data=enc([1, token]))
        time.sleep(1)


# --- client ---------------------------------------------------------------
def run_client(config_dir, private_hex, destination_hex, owner_public_hex, body_hex):
    start(config_dir)
    me = RNS.Identity.from_bytes(bytes.fromhex(private_hex))
    wanted = bytes.fromhex(destination_hex)
    deadline = time.time() + 20
    RNS.Transport.request_path(wanted)
    while not RNS.Transport.has_path(wanted) or RNS.Identity.recall(wanted) is None:
        if time.time() > deadline:
            emit(step="error", reason="path timeout")
            return 1
        time.sleep(0.2)
    owner = RNS.Identity.recall(wanted)
    app_data = RNS.Identity.recall_app_data(wanted)
    version, token = dec(app_data)
    assert version == 1 and len(token) == 16
    out = RNS.Destination(owner, RNS.Destination.OUT, RNS.Destination.SINGLE, "rdo", "object", token.hex())
    bound = out.hash == wanted and owner.get_public_key().hex() == owner_public_hex
    emit(step="resolved", bound=bound, token=token.hex())
    if not bound:
        return 1

    link = RNS.Link(out)
    while link.status != RNS.Link.ACTIVE:
        if time.time() > deadline:
            emit(step="error", reason="link timeout")
            return 1
        time.sleep(0.1)
    link.identify(me)
    time.sleep(0.5)

    def call(request):
        receipt = link.request(PATH, data=enc(request), timeout=10)
        while receipt.get_status() not in (RNS.RequestReceipt.READY, RNS.RequestReceipt.FAILED):
            time.sleep(0.05)
        if receipt.get_status() != RNS.RequestReceipt.READY:
            raise RuntimeError("request failed")
        return dec(bytes(receipt.get_response()))

    expires = int(time.time()) + 60
    read = call([1, os.urandom(16), wanted, 0, None, None, expires])
    _, _, status, head_id, head = read
    revision = dec(head)
    signed = enc(["rdo.revision.v1", 1, *revision[1:5]])
    emit(step="read", status=status, head=head_id.hex(),
         verified=owner.validate(revision[5], signed) and hashlib.sha256(head).digest() == head_id)

    body = dec(bytes.fromhex(body_hex))
    propose = [1, os.urandom(16), wanted, 1, head_id, body, expires]
    applied = call(propose)
    emit(step="propose", status=applied[2], version=applied[3].hex() if applied[3] else None)
    retry = call(propose)
    emit(step="retry", same=retry == applied)
    stale = call([1, os.urandom(16), wanted, 1, head_id, body, expires])
    emit(step="stale", status=stale[2], version=stale[3].hex() if stale[3] else None)
    link.teardown()
    return 0


if __name__ == "__main__":
    mode, *args = sys.argv[1:]
    if mode == "owner":
        run_owner(*args)
    else:
        sys.exit(run_client(*args))
