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
  final operations = OperationClient(app);

  final registry = await operations.registry();
  print('registry groups: ${registry.entriesByGroup().keys.join(', ')}');

  final status = await operations.query<Map<String, Object?>>(
    OperationCall<Map<String, Object?>>(
      operationId: 'sdk_snapshot_v2',
      payload: const <String, Object?>{},
      decode: (payload) => (payload as Map<Object?, Object?>).map(
        (key, value) => MapEntry(key.toString(), value),
      ),
    ),
  );
  print('status op=${status.operationId} accepted=${status.accepted}');
}
