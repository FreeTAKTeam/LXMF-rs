import base64
import json
import os
import tempfile

import LXMF
import RNS
import RNS.vendor.umsgpack as msgpack
from LXMF.LXMessage import LXMessage


def write_minimal_config(config_dir):
    os.makedirs(config_dir, exist_ok=True)
    config_path = os.path.join(config_dir, "config")
    with open(config_path, "w", encoding="utf-8") as handle:
        handle.write(
            "\n".join(
                [
                    "[reticulum]",
                    "  enable_transport = False",
                    "  share_instance = No",
                    "  instance_name = interop-fixture",
                    "",
                    "[interfaces]",
                    "  [[Default Interface]]",
                    "    type = AutoInterface",
                    "    enabled = No",
                    "",
                ]
            )
        )


def main():
    with tempfile.TemporaryDirectory() as tmp:
        config_dir = os.path.join(tmp, ".reticulum")
        write_minimal_config(config_dir)
        RNS.Reticulum(configdir=config_dir, loglevel=RNS.LOG_ERROR)

        source_identity = RNS.Identity()
        source = RNS.Destination(
            source_identity,
            RNS.Destination.OUT,
            RNS.Destination.SINGLE,
            "lxmf",
            "interop",
        )

        dest_identity = RNS.Identity()
        destination = RNS.Destination(
            dest_identity,
            RNS.Destination.IN,
            RNS.Destination.SINGLE,
            "lxmf",
            "interop",
        )

        fixed_timestamp = 1_700_000_000.0
        message = LXMessage(
            destination,
            source,
            content="interop-content",
            title="interop-title",
            fields={"interop": "ok"},
        )
        message.timestamp = fixed_timestamp
        message.pack()

        envelope = msgpack.packb((fixed_timestamp, [message.packed]))
        print(
            json.dumps(
                {
                    "wire_b64": base64.b64encode(message.packed).decode("ascii"),
                    "envelope_b64": base64.b64encode(envelope).decode("ascii"),
                    "content": "interop-content",
                    "title": "interop-title",
                }
            )
        )


if __name__ == "__main__":
    main()
