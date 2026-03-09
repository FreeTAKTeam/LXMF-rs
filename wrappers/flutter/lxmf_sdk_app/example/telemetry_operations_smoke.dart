import 'package:lxmf_sdk_app/lxmf_sdk_app.dart';

Future<void> main(List<String> args) async {
  if (args.isEmpty) {
    throw ArgumentError(
      'usage: dart run example/telemetry_operations_smoke.dart <rpc-endpoint> [topic-id]',
    );
  }

  final app = AppClient(
    RpcBinding(
      RpcConnectionOptions(
        endpoint: Uri.parse(args.first),
      ),
    ),
  );
  final telemetry = TelemetryClient(OperationClient(app));
  final topicId = args.length > 1 ? args[1] : null;

  final points = await telemetry.query(
    topicId: topicId,
    fromTsMs: 0,
    limit: 10,
  );
  print('telemetry points=${points.length}');

  final subscribed = await telemetry.subscribe(
    topicId: topicId,
    fromTsMs: 0,
    limit: 10,
  );
  print('subscribed=$subscribed');
}
