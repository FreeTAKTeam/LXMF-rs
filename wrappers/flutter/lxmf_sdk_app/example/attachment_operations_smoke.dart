import 'package:lxmf_sdk_app/lxmf_sdk_app.dart';

Future<void> main(List<String> args) async {
  if (args.isEmpty) {
    throw ArgumentError(
      'usage: dart run example/attachment_operations_smoke.dart <rpc-endpoint> [topic-id]',
    );
  }

  final app = AppClient(
    RpcBinding(
      RpcConnectionOptions(
        endpoint: Uri.parse(args.first),
      ),
    ),
  );
  final attachments = AttachmentClient(OperationClient(app));
  final topicId = args.length > 1 ? args[1] : null;

  final stored = await attachments.store(
    name: 'sample.txt',
    contentType: 'text/plain',
    bytesBase64: 'aGVsbG8gd29ybGQ=',
    topicIds: topicId == null ? const <String>[] : <String>[topicId],
  );
  print('stored attachment ${stored.attachmentId} bytes=${stored.byteLen}');

  final fetched = await attachments.get(stored.attachmentId);
  print('fetched attachment name=${fetched?.name}');

  final listed = await attachments.list(topicId: topicId, limit: 10);
  print('attachment list size=${listed.attachments.length}');

  if (topicId != null) {
    final associated = await attachments.associateTopic(
      attachmentId: stored.attachmentId,
      topicId: topicId,
    );
    print('associated=$associated');
  }

  final deleted = await attachments.delete(stored.attachmentId);
  print('deleted=$deleted');
}
