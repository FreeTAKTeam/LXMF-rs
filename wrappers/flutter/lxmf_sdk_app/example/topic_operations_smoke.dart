import 'package:lxmf_sdk_app/lxmf_sdk_app.dart';

Future<void> main(List<String> args) async {
  if (args.isEmpty) {
    throw ArgumentError(
        'usage: dart run example/topic_operations_smoke.dart <rpc-endpoint>');
  }

  final app = AppClient(
    RpcBinding(
      RpcConnectionOptions(
        endpoint: Uri.parse(args.first),
      ),
    ),
  );
  final topics = TopicClient(OperationClient(app));

  final created = await topics.create(
    topicPath: 'ops/flutter-smoke',
    metadata: const <String, Object?>{'kind': 'smoke'},
  );
  print('created topic ${created.topicId} path=${created.topicPath}');

  final listed = await topics.list(limit: 10);
  print(
      'topic list size=${listed.topics.length} nextCursor=${listed.nextCursor}');

  final subscribed = await topics.subscribe(created.topicId);
  print('subscribed=$subscribed');

  final published = await topics.publish(
    topicId: created.topicId,
    payload: const <String, Object?>{'message': 'hello topic'},
    correlationId: 'topic-smoke-1',
  );
  print('published=$published');

  final unsubscribed = await topics.unsubscribe(created.topicId);
  print('unsubscribed=$unsubscribed');
}
