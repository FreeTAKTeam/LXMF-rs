"""Pinned stock Python relay fixture for the embedded Rust recipient client."""
import json
import pathlib
import sys
import time

import LXMF
import RNS

root = pathlib.Path(sys.argv[1])
port = int(sys.argv[2])


def publish(name, value):
    target = root / name
    pending = target.with_suffix(".tmp")
    pending.write_text(json.dumps(value))
    pending.replace(target)


profile = root / "python"
profile.mkdir()
(profile / "config").write_text(f"""[reticulum]
  enable_transport = Yes
  share_instance = No
[interfaces]
  [[Native test]]
    type = TCPServerInterface
    enabled = Yes
    listen_ip = 127.0.0.1
    listen_port = {port}
""")
RNS.Reticulum(configdir=str(profile), loglevel=RNS.LOG_ERROR)
router = LXMF.LXMRouter(storagepath=str(root / "lxmf"), autopeer=False)
router.enable_propagation()
router.lxmf_propagation((root / "transient").read_bytes(), stamp_data=bytes(32), stamp_value=0)
assert len(router.propagation_entries) == 1
publish("ready.json", {"relay": router.propagation_destination.hexhash})
deadline = time.monotonic() + 45
while time.monotonic() < deadline:
    router.propagation_destination.announce(app_data=router.get_propagation_node_app_data())
    if (root / "done").exists():
        publish("result.json", {"remaining": len(router.propagation_entries)})
        break
    time.sleep(1)
else:
    raise RuntimeError("native client did not finish")
