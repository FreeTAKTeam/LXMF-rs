import 'package:lxmf_sdk_app/lxmf_sdk_app.dart';

Future<void> main(List<String> args) async {
  final endpoint = args.isNotEmpty
      ? Uri.parse(args.first)
      : Uri.parse('http://127.0.0.1:4543/rpc');

  final app = AppClient(
    RpcBinding(
      RpcConnectionOptions(endpoint: endpoint),
    ),
  );
  final commands = CustomCommandClient(OperationClient(app));

  final result = await commands.invoke<Map<String, Object?>>(
    CustomCommandCall<Map<String, Object?>>(
      operationId: 'vendor.example.custom',
      target: 'node-b',
      timeoutMs: 500,
      payload: const <String, Object?>{'body': 'hello'},
      decodeEcho: (payload) => (payload as Map<Object?, Object?>).map(
        (key, value) => MapEntry(key.toString(), value),
      ),
    ),
  );

  print(
    'custom command ${result.command} accepted=${result.accepted} correlation=${result.correlationId}',
  );
  print('echo body=${result.echo['body']}');
}
