import 'package:lxmf_sdk_app/lxmf_sdk_app.dart';

Future<void> main(List<String> args) async {
  if (args.isEmpty) {
    throw ArgumentError(
      'usage: dart run example/marker_operations_smoke.dart <rpc-endpoint> [topic-id]',
    );
  }

  final app = AppClient(
    RpcBinding(
      RpcConnectionOptions(
        endpoint: Uri.parse(args.first),
      ),
    ),
  );
  final markers = MarkerClient(OperationClient(app));
  final topicId = args.length > 1 ? args[1] : null;

  final created = await markers.create(
    label: 'Alpha',
    position: const GeoPoint(lat: 35.0, lon: -115.0, altM: 1200.0),
    topicId: topicId,
  );
  print('created marker ${created.markerId} revision=${created.revision}');

  final listed = await markers.list(topicId: topicId, limit: 10);
  print('marker list size=${listed.markers.length}');

  final updated = await markers.updatePosition(
    markerId: created.markerId,
    expectedRevision: created.revision,
    position: const GeoPoint(lat: 36.0, lon: -116.0),
  );
  print('updated marker revision=${updated.revision}');

  final deleted = await markers.delete(
    markerId: updated.markerId,
    expectedRevision: updated.revision,
  );
  print('deleted=$deleted');
}
