import 'dart:io';

import 'package:lxmf_sdk_app/experimental_embedded.dart';
import 'package:lxmf_sdk_app/lxmf_sdk_app.dart';

Future<void> main() async {
  final client = AppClient(
    EmbeddedNodeBridge.open(
        libraryPath: Platform.environment['RNS_EMBEDDED_FFI_LIB']),
  );
  final config = Config.fromProfile(Profile.testingDefault);

  final handle = await client.start(config);
  print('started runtime ${handle.runtimeId} for profile ${handle.profile.id}');

  final status = await client.status();
  print('runtime state: ${status.state.name}');

  await client.stop();
  print('stopped runtime');
}
