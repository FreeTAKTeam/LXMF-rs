import 'package:lxmf_sdk_app/lxmf_sdk_app.dart';

Future<void> main(List<String> args) async {
  final endpoint = args.isNotEmpty
      ? Uri.parse(args.first)
      : Uri.parse('http://127.0.0.1:4543/rpc');

  final binding = RpcBinding(
    RpcConnectionOptions(endpoint: endpoint),
  );
  final client = AppClient(binding);

  final registry = await client.operationRegistry();
  print('catalog entries: ${registry.entries.length}');
  print(
      'sdk_identity_list_v2 => ${registry.canonicalize('sdk_identity_list_v2')}');

  final response = await client.queryOperation(
    'sdk_identity_list_v2',
    const <String, Object?>{},
    correlationId: 'flutter-op-smoke',
  );
  print(
    'operation ${response.operationId} accepted=${response.accepted} payloadType=${response.payload.runtimeType}',
  );
}
