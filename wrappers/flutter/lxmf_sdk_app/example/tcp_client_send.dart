import 'dart:async';
import 'dart:io';

import 'package:lxmf_sdk_app/lxmf_sdk_app.dart';

Future<void> main(List<String> args) async {
  if (args.length < 4) {
    stderr.writeln(
      'usage: dart run example/tcp_client_send.dart <host> <port> <destination-hex> <message>',
    );
    exitCode = 2;
    return;
  }

  final host = args[0];
  final port = int.tryParse(args[1]);
  final destination = args[2];
  final message = args.sublist(3).join(' ');
  if (port == null || port <= 0 || port > 65535) {
    stderr.writeln('invalid tcp port: ${args[1]}');
    exitCode = 2;
    return;
  }

  final client = AppClient(
    EmbeddedNodeBridge.open(
      libraryPath: Platform.environment['RNS_EMBEDDED_FFI_LIB'],
    ),
  );

  await client.start(
    Config(
      profile: Profile.testingDefault,
      transportMode: TransportMode.tcpClient,
      tcpHost: host,
      tcpPort: port,
      eventBatchSize: 16,
    ),
  );

  final receipt = await client.send(
    SendRequest(
      source: 'flutter-smoke',
      destination: destination,
      payload: message,
      correlationId: 'tcp-client-send',
    ),
  );
  await Future<void>.delayed(const Duration(milliseconds: 300));
  await client.stop();

  stdout.writeln(
      'SENT messageId=${receipt.messageId} runtime=${receipt.runtimeId}');
}
