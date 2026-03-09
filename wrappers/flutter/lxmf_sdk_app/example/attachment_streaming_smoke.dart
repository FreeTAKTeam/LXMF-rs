import 'package:lxmf_sdk_app/lxmf_sdk_app.dart';

Future<void> main(List<String> args) async {
  if (args.isEmpty) {
    throw ArgumentError(
      'usage: dart run example/attachment_streaming_smoke.dart <rpc-endpoint>',
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
  const checksum =
      '64ec88ca00b268e5ba1a35678a1b5316d212f4f366b2477232534a8aeca37f3c';

  final session = await attachments.uploadStart(
    name: 'chunked.bin',
    contentType: 'application/octet-stream',
    totalSize: 11,
    checksumSha256: checksum,
  );
  print(
      'upload session ${session.uploadId} attachment=${session.attachmentId}');

  final ack = await attachments.uploadChunk(
    uploadId: session.uploadId,
    offset: 0,
    bytesBase64: 'aGVsbG8gd29ybGQ=',
  );
  print('chunk accepted=${ack.accepted} nextOffset=${ack.nextOffset}');

  final committed = await attachments.uploadCommit(uploadId: session.uploadId);
  print('committed attachment ${committed.attachmentId}');

  final chunk = await attachments.downloadChunk(
    attachmentId: committed.attachmentId,
    offset: 0,
    maxBytes: 5,
  );
  print('downloaded chunk nextOffset=${chunk.nextOffset} done=${chunk.done}');
}
