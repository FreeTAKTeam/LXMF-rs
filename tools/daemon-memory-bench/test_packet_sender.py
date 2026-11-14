import importlib.util
import sys
import types
import unittest
from pathlib import Path
from unittest.mock import patch

lxmf = types.ModuleType('LXMF')
lxmf.LXMessage = types.SimpleNamespace(FAILED=255, SENT=4)
rns = types.ModuleType('RNS')
rns.Packet = lambda *args: types.SimpleNamespace(send=lambda: False)
rns.PacketReceipt = types.SimpleNamespace(DELIVERED=2, FAILED=0, CULLED=255)
vendor = types.ModuleType('RNS.vendor')
vendor.umsgpack = object()
peer = types.ModuleType('peer')
peer.CaptureError = type('CaptureError', (Exception,), {})
peer.wait_normal_start = lambda *args: 0
peer.record_inbound_queue = lambda *args: None
with patch.dict(sys.modules, {'LXMF': lxmf, 'RNS': rns, 'RNS.vendor': vendor, 'peer': peer}):
    module_spec = importlib.util.spec_from_file_location('packet_sender_test', Path(__file__).with_name('packet_sender.py'))
    module = importlib.util.module_from_spec(module_spec)
    module_spec.loader.exec_module(module)


class PacketSenderTest(unittest.TestCase):
    def test_false_send_is_recorded_as_a_transport_failure(self):
        events, completed = [], []
        row = {'sequence': 1, 'packet_base64': 'YQ=='}
        module.submit_packet(object(), row, lambda event, **fields: events.append((event, fields)),
                             lambda item, state, receipt: completed.append((item, state, receipt)), 30)
        self.assertEqual(events, [('protocol_send', {'sequence': 1, 'packet_bytes': 1, 'submitted': False})])
        self.assertEqual(completed, [(row, 255, None)])

    def test_proof_wait_uses_the_frozen_latency_target(self):
        timeouts = []
        receipt = types.SimpleNamespace(status=1, set_timeout=timeouts.append,
                                        set_delivery_callback=lambda callback: None,
                                        set_timeout_callback=lambda callback: None)
        with patch.object(module.RNS, 'Packet', return_value=types.SimpleNamespace(send=lambda: receipt)):
            module.submit_packet(object(), {'sequence': 0, 'packet_base64': 'YQ=='},
                                 lambda *args, **kwargs: None, lambda *args: None, 30)
        self.assertEqual(timeouts, [30])


if __name__ == '__main__':
    unittest.main()
