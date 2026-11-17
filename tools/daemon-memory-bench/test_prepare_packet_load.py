import hashlib
import importlib.util
import sys
import types
import unittest
from pathlib import Path
from unittest.mock import patch

block = b'fixed-work-block'
stamper = types.SimpleNamespace(STAMP_SIZE=32, WORKBLOCK_EXPAND_ROUNDS_PN=1000,
    stamp_workblock=lambda material, expand_rounds: block,
    stamp_valid=lambda stamp, cost, workblock: int.from_bytes(hashlib.sha256(workblock + stamp).digest(), 'big') <= 1 << (256-cost))
lxmf = types.ModuleType('LXMF')
lxmf.LXStamper = stamper
rns = types.ModuleType('RNS')
vendor = types.ModuleType('RNS.vendor')
vendor.umsgpack = object()
peer = types.ModuleType('peer')
peer.payload = lambda *args: b''
with patch.dict(sys.modules, {'LXMF': lxmf, 'RNS': rns, 'RNS.vendor': vendor, 'peer': peer}):
    module_spec = importlib.util.spec_from_file_location('prepared_packets', Path(__file__).with_name('prepare_packet_load.py'))
    module = importlib.util.module_from_spec(module_spec)
    module_spec.loader.exec_module(module)


class PreparationTest(unittest.TestCase):
    def test_prefix_hash_uses_the_reference_stamp_rule(self):
        stamp, attempts = module.stamp_for(b'id', 4)
        self.assertGreaterEqual(attempts, 1)
        self.assertEqual(len(stamp), 32)
        self.assertTrue(stamper.stamp_valid(stamp, 4, block))


if __name__ == '__main__':
    unittest.main()
