import 'package:lxmf_sdk_app/lxmf_sdk_app.dart';

Future<void> main(List<String> args) async {
  final endpoint = args.isNotEmpty
      ? Uri.parse(args.first)
      : Uri.parse('http://127.0.0.1:4243/rpc');

  final binding = RpcBinding(
    RpcConnectionOptions(endpoint: endpoint),
  );
  final client = AppClient(binding);
  final handle = await client.start(
    const Config(
      profile: Profile.desktopDefault,
      eventBatchSize: 32,
    ),
  );

  final status = await client.status();
  print('started runtime ${handle.runtimeId} with state ${status.state.name}');

  final receipt = await client.send(
    const SendRequest(
      source: 'flutter-smoke-src',
      destination: 'flutter-smoke-dst',
      payload: 'hello from flutter rpc smoke',
    ),
  );
  print('queued message ${receipt.messageId}');

  await client.stop();
  print('requested graceful shutdown');
}
