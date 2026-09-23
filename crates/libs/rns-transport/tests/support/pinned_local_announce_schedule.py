import ast
import inspect
import threading
import textwrap
import types
from RNS.Transport import Transport

tree = ast.parse(inspect.getsource(Transport))
assert any(
    isinstance(node, ast.If)
    and "transport_enabled" in ast.unparse(node.test)
    and "is_from_local_client" in ast.unparse(node.test)
    for node in ast.walk(tree)
), "local-client announces must be queued even when transport forwarding is disabled"
local_client_due_entry = False
for node in ast.walk(tree):
    if isinstance(node, ast.If) and isinstance(node.test, ast.Name) and node.test.id == "is_from_local_client":
        assignments = [ast.unparse(child) for child in ast.walk(node) if isinstance(child, ast.Assign)]
        if "retransmit_timeout = now" in assignments and "retries = Transport.PATHFINDER_R" in assignments:
            local_client_due_entry = True
assert local_client_due_entry, "local-client announce must be due at now with PATHFINDER_R retries"
assert Transport.PATHFINDER_R == 1
assert Transport.announces_check_interval == 1.0
assert Transport.job_interval == 0.25

jobs_tree = ast.parse(textwrap.dedent(inspect.getsource(Transport.jobs)))
announce_check = next(
    node for node in ast.walk(jobs_tree)
    if isinstance(node, ast.If) and "announces_last_checked" in ast.unparse(node.test)
)
assert "time.time() > announce_entry[IDX_AT_RTRNS_TMO]" in ast.unparse(announce_check)
assert "announce_entry[IDX_AT_RETRIES] > Transport.PATHFINDER_R" in ast.unparse(announce_check)

class FakeClock:
    current = 10.0

    @classmethod
    def time(cls):
        return cls.current

class FakePacket:
    NONE, PATH_RESPONSE, ANNOUNCE, HEADER_2 = 0, 1, 2, 2

    def __init__(self, *args, **kwargs):
        self.args, self.kwargs = args, kwargs

class FakeDestination:
    OUT, SINGLE = 1, 1

    def __init__(self, *args):
        self.args = args

def run_python_announce_check(now):
    destination_hash = b"destination"
    announce = types.SimpleNamespace(
        data=b"announce", destination_hash=destination_hash, context_flag=0
    )
    entry = [0, 10.0, Transport.PATHFINDER_R, None, 3, announce, 0, False, None]
    fake_transport = types.SimpleNamespace(
        announces_last_checked=0.0,
        announces_check_interval=Transport.announces_check_interval,
        announce_table_lock=threading.RLock(),
        announce_table={destination_hash: entry},
        held_announces={},
        PATHFINDER_R=Transport.PATHFINDER_R,
        LOCAL_REBROADCASTS_MAX=Transport.LOCAL_REBROADCASTS_MAX,
        PATHFINDER_G=Transport.PATHFINDER_G,
        PATHFINDER_RW=Transport.PATHFINDER_RW,
        TRANSPORT=3,
        identity=types.SimpleNamespace(hash=b"transport"),
    )
    fake_rns = types.SimpleNamespace(
        Packet=FakePacket,
        Identity=types.SimpleNamespace(recall=lambda *_args, **_kwargs: object()),
        Destination=FakeDestination,
        LOG_EXTREME=0,
        LOG_PATHING=0,
        log=lambda *_args, **_kwargs: None,
        sl=lambda *_args, **_kwargs: False,
    )
    namespace = {
        "Transport": fake_transport, "RNS": fake_rns, "time": FakeClock,
        "outgoing": [], "completed_announces": [], "IDX_AT_RETRIES": 2,
        "IDX_AT_RTRNS_TMO": 1, "IDX_AT_PACKET": 5,
        "IDX_AT_BLCK_RBRD": 7, "IDX_AT_ATTCHD_IF": 8,
    }
    FakeClock.current = now
    code = compile(ast.Module(body=[announce_check], type_ignores=[]), "pinned-Transport.jobs", "exec")
    exec(code, namespace)
    return namespace["outgoing"], entry, fake_transport.announce_table, namespace, code

at_deadline, equal_entry, _, _, _ = run_python_announce_check(10.0)
assert not at_deadline and equal_entry[2] == Transport.PATHFINDER_R
after_deadline, after_entry, after_table, after_namespace, code = run_python_announce_check(10.000001)
assert len(after_deadline) == 1 and after_entry[2] == Transport.LOCAL_REBROADCASTS_MAX
after_namespace["outgoing"] = []
FakeClock.current = 11.000002
exec(code, after_namespace)
assert not after_namespace["outgoing"] and not after_table
print(f"{Transport.PATHFINDER_R},{int(Transport.announces_check_interval * 1000)},{int(Transport.job_interval * 1000)},strict-boundary,one-retransmit")
