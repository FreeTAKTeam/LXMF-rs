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

original_urandom = os.urandom

def fixed_urandom(n):
    return bytes([0x42] * n)

try:
    os.urandom = fixed_urandom
    prop_msg.pack()
finally:
    os.urandom = original_urandom

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
with open(os.path.join(OUT, "storage_unsigned.bin"), "wb") as f:
    f.write(storage_msg.packed_container())
with open(os.path.join(OUT, "storage_signed.bin"), "wb") as f:
    f.write(storage_msg_alt.packed_container())
with open(os.path.join(OUT, "propagation.bin"), "wb") as f:
    f.write(prop_msg.propagation_packed)
with open(os.path.join(OUT, "propagation_dest_pubkey.bin"), "wb") as f:
    f.write(dest_identity.get_public_key())
