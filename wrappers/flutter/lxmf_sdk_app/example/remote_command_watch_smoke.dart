import 'package:lxmf_sdk_app/lxmf_sdk_app.dart';

Future<void> main(List<String> args) async {
  final endpoint = args.isNotEmpty
      ? Uri.parse(args.first)
      : Uri.parse('http://127.0.0.1:4543/rpc');

  final client = AppClient(
    RpcBinding(
      RpcConnectionOptions(endpoint: endpoint),
    ),
  );
  final operations = OperationClient(client);
  final commands = CustomCommandClient(operations);
  final remote = RemoteCommandClient(client);

  final dispatched = await commands.invoke<Map<String, Object?>>(
    CustomCommandCall<Map<String, Object?>>(
      operationId: 'vendor.example.custom',
      target: 'node-b',
      timeoutMs: 1000,
      payload: const <String, Object?>{'body': 'hello'},
      decodeEcho: (payload) => (payload as Map<Object?, Object?>).map(
        (key, value) => MapEntry(key.toString(), value),
      ),
    ),
  );

  print(
    'dispatched command=${dispatched.command} '
    'correlation=${dispatched.correlationId}',
  );

  final correlationId = dispatched.correlationId;
  if (correlationId == null) {
    print('daemon did not return a remote command correlation id');
    return;
  }

  await for (final session in remote.watch(correlationId)) {
    print(
      'session=${session.commandId} '
      'state=${session.commandState.name} '
      'accepted=${session.accepted}',
    );
    if (session.isTerminal) {
      break;
    }
  }
}
