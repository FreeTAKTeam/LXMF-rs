#!/usr/bin/env python3
"""Bridge two raw PTY pairs so separate serial KISS stacks can interoperate."""

import errno
import json
import os
import pty
import select
import tty


rust_master, rust_slave = pty.openpty()
python_master, python_slave = pty.openpty()
tty.setraw(rust_slave)
tty.setraw(python_slave)

print(
    json.dumps(
        {
            "rust_device": os.ttyname(rust_slave),
            "python_device": os.ttyname(python_slave),
        }
    ),
    flush=True,
)

masters = {rust_master: python_master, python_master: rust_master}
while True:
    readable, _, _ = select.select((rust_master, python_master), (), (), 0.5)
    for source in readable:
        try:
            data = os.read(source, 65_536)
        except OSError as error:
            if error.errno in (errno.EAGAIN, errno.EIO):
                continue
            raise
        if not data:
            continue
        target = masters[source]
        offset = 0
        while offset < len(data):
            offset += os.write(target, data[offset:])
