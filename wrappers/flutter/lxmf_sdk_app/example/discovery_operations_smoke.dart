import 'package:lxmf_sdk_app/lxmf_sdk_app.dart';

Future<void> main(List<String> args) async {
  if (args.isEmpty) {
    throw ArgumentError(
      'usage: dart run example/discovery_operations_smoke.dart <rpc-endpoint>',
    );
  }

  final app = AppClient(
    RpcBinding(
      RpcConnectionOptions(
        endpoint: Uri.parse(args.first),
      ),
    ),
  );
  final discovery = DiscoveryClient(OperationClient(app));

  final identities = await discovery.identityList();
  print('identities=${identities.length}');

  final announced = await discovery.announceNow();
  print('announce=$announced');

  final presence = await discovery.presenceList(limit: 10);
  print('presence=${presence.peers.length}');

  final contacts = await discovery.contactList(limit: 10);
  print('contacts=${contacts.contacts.length}');

  final directory = await discovery.peerDirectory(limit: 10);
  print('directory=${directory.length}');
}
