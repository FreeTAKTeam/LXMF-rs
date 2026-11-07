"""Run the pinned lxmd and record negotiated inbound propagation Link limits."""

import json
from pathlib import Path
import runpy
import sys


def main():
    import LXMF

    limits_path = Path(sys.argv.pop(1))
    original = LXMF.LXMRouter.propagation_link_established
    original_resource = LXMF.LXMRouter.propagation_resource_concluded

    def record(link, resource_hash=None):
        row = {"link_id": link.link_id.hex(), "mtu": link.mtu, "mdu": link.mdu}
        if resource_hash is not None:
            row["resource_hash"] = resource_hash.hex()
        with limits_path.open("a", encoding="utf-8") as output:
            output.write(json.dumps(row) + "\n")

    def established(router, link):
        original(router, link)
        record(link)

    def concluded(router, resource):
        record(resource.link, resource.get_hash())
        return original_resource(router, resource)

    LXMF.LXMRouter.propagation_link_established = established
    LXMF.LXMRouter.propagation_resource_concluded = concluded
    runpy.run_module("LXMF.Utilities.lxmd", run_name="__main__", alter_sys=True)


if __name__ == "__main__":
    main()
