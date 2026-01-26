import os
import sys
import time

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", "..", ".."))
RETICULUM = os.path.abspath(os.path.join(ROOT, "..", "Reticulum"))
sys.path.insert(0, ROOT)
sys.path.insert(0, RETICULUM)

import RNS  # noqa: E402
import RNS.vendor.umsgpack as msgpack  # noqa: E402
import LXMF  # noqa: E402
from LXMF.LXMessage import LXMessage  # noqa: E402

OUT = os.path.join(ROOT, "tests", "fixtures", "python", "lxmf")
os.makedirs(OUT, exist_ok=True)

config_dir = os.path.join(OUT, ".reticulum")
os.makedirs(config_dir, exist_ok=True)

RNS.Reticulum(configdir=config_dir, loglevel=RNS.LOG_ERROR)

identity = RNS.Identity()
source = RNS.Destination(identity, RNS.Destination.OUT, RNS.Destination.SINGLE, "lxmf", "test")

dest_identity = RNS.Identity()
destination = RNS.Destination(dest_identity, RNS.Destination.IN, RNS.Destination.SINGLE, "lxmf", "test")

fixed_timestamp = 1_700_000_000.0

msg_bytes = LXMessage(destination, source, content=b"hello-bytes", title=b"bytes-title", fields={"k": b"v"})
msg_bytes.timestamp = fixed_timestamp
msg_bytes.payload = msgpack.packb((msg_bytes.timestamp, msg_bytes.content, msg_bytes.title, msg_bytes.fields))

msg_strings = LXMessage(destination, source, content="hello", title="title", fields={"k": "v"})
msg_strings.timestamp = fixed_timestamp
msg_strings.payload = msgpack.packb((msg_strings.timestamp, msg_strings.content, msg_strings.title, msg_strings.fields))

wire_msg = LXMessage(destination, source, content="wire", title="wire", fields=None)
wire_msg.timestamp = fixed_timestamp
wire_msg.pack()

packed_msg = LXMessage(destination, source, content="packed", title="packed", fields={"a": "b"})
packed_msg.timestamp = fixed_timestamp
packed_msg.pack()

storage_msg = LXMessage(destination, source, content="storage", title="", fields=None)
storage_msg.timestamp = fixed_timestamp
storage_msg.pack()

storage_msg_alt = LXMessage(destination, source, content="storage", title="", fields=None)
storage_msg_alt.timestamp = fixed_timestamp
storage_msg_alt.pack()
storage_msg_alt.state = LXMessage.DELIVERED
storage_msg_alt.transport_encrypted = True
storage_msg_alt.transport_encryption = LXMessage.ENCRYPTION_DESCRIPTION_AES
storage_msg_alt.method = LXMessage.DIRECT

prop_msg = LXMessage(destination, source, content="prop", title="", fields=None, desired_method=LXMessage.PROPAGATED)
prop_msg.timestamp = fixed_timestamp

paper_msg = LXMessage(destination, source, content="paper", title="", fields=None, desired_method=LXMessage.PAPER)
paper_msg.timestamp = fixed_timestamp

original_urandom = os.urandom
original_time = time.time

def fixed_urandom(n):
    return bytes([0x42] * n)

try:
    os.urandom = fixed_urandom
    time.time = lambda: fixed_timestamp
    prop_msg.pack()
    paper_msg.pack()
finally:
    os.urandom = original_urandom
    time.time = original_time

with open(os.path.join(OUT, "payload_bytes.bin"), "wb") as f:
    f.write(msg_bytes.payload)
with open(os.path.join(OUT, "payload_strings.bin"), "wb") as f:
    f.write(msg_strings.payload)
with open(os.path.join(OUT, "wire_signed.bin"), "wb") as f:
    f.write(wire_msg.packed)
with open(os.path.join(OUT, "message_packed.bin"), "wb") as f:
    f.write(packed_msg.packed)
with open(os.path.join(OUT, "propagation_message.bin"), "wb") as f:
    f.write(prop_msg.packed)
with open(os.path.join(OUT, "paper_message.bin"), "wb") as f:
    f.write(paper_msg.packed)
with open(os.path.join(OUT, "storage_unsigned.bin"), "wb") as f:
    f.write(storage_msg.packed_container())
with open(os.path.join(OUT, "storage_signed.bin"), "wb") as f:
    f.write(storage_msg_alt.packed_container())
with open(os.path.join(OUT, "propagation.bin"), "wb") as f:
    f.write(prop_msg.propagation_packed)
with open(os.path.join(OUT, "paper.bin"), "wb") as f:
    f.write(paper_msg.paper_packed)

# Delivery method/representation matrix
def content_size_for(message):
    packed_payload = msgpack.packb([message.timestamp, message.title, message.content, message.fields])
    return len(packed_payload) - LXMessage.TIMESTAMP_SIZE - LXMessage.STRUCT_OVERHEAD

def find_length_at_most(limit, destination, desired_method):
    best_length = 0
    best_size = 0
    for length in range(0, limit + 128):
        msg = LXMessage(destination, source, content=b"a" * length, title=b"", fields=None, desired_method=desired_method)
        msg.timestamp = fixed_timestamp
        size = content_size_for(msg)
        if size <= limit:
            best_length = length
            best_size = size
        else:
            break
    return best_length, best_size

def find_length_over(limit, destination, desired_method):
    for length in range(0, limit + 128):
        msg = LXMessage(destination, source, content=b"a" * length, title=b"", fields=None, desired_method=desired_method)
        msg.timestamp = fixed_timestamp
        size = content_size_for(msg)
        if size > limit:
            return length, size
    return limit + 1, limit + 1

plain_destination = RNS.Destination(None, RNS.Destination.IN, RNS.Destination.PLAIN, "lxmf", "plain")

delivery_cases = []

# Opportunistic (single) within encrypted max
length, size = find_length_at_most(LXMessage.ENCRYPTED_PACKET_MAX_CONTENT, destination, LXMessage.OPPORTUNISTIC)
msg = LXMessage(destination, source, content=b"a" * length, title=b"", fields=None, desired_method=LXMessage.OPPORTUNISTIC)
msg.timestamp = fixed_timestamp
msg.pack()
delivery_cases.append({
    "desired_method": LXMessage.OPPORTUNISTIC,
    "destination_plain": False,
    "content_size": size,
    "expected_method": msg.method,
    "expected_representation": msg.representation,
})

# Opportunistic (single) over encrypted max -> direct
length, size = find_length_over(LXMessage.ENCRYPTED_PACKET_MAX_CONTENT, destination, LXMessage.OPPORTUNISTIC)
msg = LXMessage(destination, source, content=b"a" * length, title=b"", fields=None, desired_method=LXMessage.OPPORTUNISTIC)
msg.timestamp = fixed_timestamp
msg.pack()
delivery_cases.append({
    "desired_method": LXMessage.OPPORTUNISTIC,
    "destination_plain": False,
    "content_size": size,
    "expected_method": msg.method,
    "expected_representation": msg.representation,
})

# Opportunistic (plain) within plain max
length, size = find_length_at_most(LXMessage.PLAIN_PACKET_MAX_CONTENT, plain_destination, LXMessage.OPPORTUNISTIC)
msg = LXMessage(plain_destination, source, content=b"a" * length, title=b"", fields=None, desired_method=LXMessage.OPPORTUNISTIC)
msg.timestamp = fixed_timestamp
msg.pack()
delivery_cases.append({
    "desired_method": LXMessage.OPPORTUNISTIC,
    "destination_plain": True,
    "content_size": size,
    "expected_method": msg.method,
    "expected_representation": msg.representation,
})

# Direct over link max -> resource
length, size = find_length_over(LXMessage.LINK_PACKET_MAX_CONTENT, destination, LXMessage.DIRECT)
msg = LXMessage(destination, source, content=b"a" * length, title=b"", fields=None, desired_method=LXMessage.DIRECT)
msg.timestamp = fixed_timestamp
msg.pack()
delivery_cases.append({
    "desired_method": LXMessage.DIRECT,
    "destination_plain": False,
    "content_size": size,
    "expected_method": msg.method,
    "expected_representation": msg.representation,
})

# Propagated over link max -> resource
length, size = find_length_over(LXMessage.LINK_PACKET_MAX_CONTENT, destination, LXMessage.PROPAGATED)
msg = LXMessage(destination, source, content=b"a" * length, title=b"", fields=None, desired_method=LXMessage.PROPAGATED)
msg.timestamp = fixed_timestamp
msg.pack()
delivery_cases.append({
    "desired_method": LXMessage.PROPAGATED,
    "destination_plain": False,
    "content_size": size,
    "expected_method": msg.method,
    "expected_representation": msg.representation,
})

# Paper within limit
best_msg = None
best_size = 0
for length in range(0, LXMessage.PAPER_MDU + 128):
    msg = LXMessage(destination, source, content=b"a" * length, title=b"", fields=None, desired_method=LXMessage.PAPER)
    msg.timestamp = fixed_timestamp
    try:
        msg.pack()
        best_msg = msg
        best_size = len(msg.paper_packed)
    except TypeError:
        break

if best_msg is not None:
    delivery_cases.append({
        "desired_method": LXMessage.PAPER,
        "destination_plain": False,
        "content_size": best_size,
        "expected_method": best_msg.method,
        "expected_representation": best_msg.representation,
    })

with open(os.path.join(OUT, "delivery_matrix.msgpack"), "wb") as f:
    f.write(msgpack.packb(delivery_cases))
with open(os.path.join(OUT, "propagation_dest_pubkey.bin"), "wb") as f:
    f.write(dest_identity.get_public_key())
