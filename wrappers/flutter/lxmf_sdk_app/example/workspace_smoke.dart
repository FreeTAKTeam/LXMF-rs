import 'package:lxmf_sdk_app/lxmf_sdk_app.dart';

Future<void> main(List<String> args) async {
  if (args.isEmpty) {
    throw ArgumentError(
      'usage: dart run example/workspace_smoke.dart <rpc-endpoint>',
    );
  }

  final workspace = WorkspaceClient.rpc(
    RpcConnectionOptions(endpoint: Uri.parse(args.first)),
  );

  final handle = await workspace.start(
    const Config(
      profile: Profile.desktopDefault,
      requestedCapabilities: <String>[
        'sdk.capability.identity_multi',
        'sdk.capability.contact_management',
      ],
    ),
  );

  final identities = await workspace.discovery.identityList();
  final registry = await workspace.operations.registry();

  print(
    'workspace=${handle.runtimeId} identities=${identities.length} '
    'ops=${registry.entries.length}',
  );

  await workspace.stop();
}
