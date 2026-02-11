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
from LXMF import LXStamper  # noqa: E402


OUT = os.path.join(ROOT, "tests", "fixtures", "python", "lxmf")
os.makedirs(OUT, exist_ok=True)

# Stamp case
material = b"lxmf-stamp-material-0001"
workblock = LXStamper.stamp_workblock(material)
stamp, _ = LXStamper.generate_stamp(material, 4)
valid_case = {
    "material": material,
    "target_cost": 4,
    "stamp": stamp,
    "expected_value": LXStamper.stamp_value(workblock, stamp),
}
invalid_case = dict(valid_case)
invalid_case["stamp"] = bytes([b ^ 0xFF for b in stamp])

# PN stamp case
lxm_data = b"lxmf-transient-0001"
transient_id = RNS.Identity.full_hash(lxm_data)
pn_stamp, _ = LXStamper.generate_stamp(
    transient_id, 4, expand_rounds=LXStamper.WORKBLOCK_EXPAND_ROUNDS_PN
)
pn_case = {
    "transient_data": lxm_data + pn_stamp,
    "target_cost": 4,
}

# Ticket cases
now = time.time()
expires = now + 60
expired = now - 60
valid_ticket = {
    "expires": expires,
    "ticket": os.urandom(LXMF.LXMessage.TICKET_LENGTH),
    "now": now,
}
expired_ticket = {
    "expires": expired,
    "ticket": os.urandom(LXMF.LXMessage.TICKET_LENGTH),
    "now": now,
}

with open(os.path.join(OUT, "stamp_valid.msgpack"), "wb") as f:
    f.write(msgpack.packb(valid_case))
with open(os.path.join(OUT, "stamp_invalid.msgpack"), "wb") as f:
    f.write(msgpack.packb(invalid_case))
with open(os.path.join(OUT, "pn_stamp_valid.msgpack"), "wb") as f:
    f.write(msgpack.packb(pn_case))
with open(os.path.join(OUT, "ticket_valid.msgpack"), "wb") as f:
    f.write(msgpack.packb(valid_ticket))
with open(os.path.join(OUT, "ticket_expired.msgpack"), "wb") as f:
    f.write(msgpack.packb(expired_ticket))
