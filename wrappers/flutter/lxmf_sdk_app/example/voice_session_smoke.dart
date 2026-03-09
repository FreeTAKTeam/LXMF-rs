import 'package:lxmf_sdk_app/lxmf_sdk_app.dart';

Future<void> main(List<String> args) async {
  final endpoint = args.isNotEmpty
      ? Uri.parse(args.first)
      : Uri.parse('http://127.0.0.1:4543/rpc');

  final voice = VoiceSessionClient(
    OperationClient(
      AppClient(
        RpcBinding(
          RpcConnectionOptions(endpoint: endpoint),
        ),
      ),
    ),
  );

  final sessionId = await voice.open(peerId: 'node-b', codecHint: 'opus');
  print('opened voice session: $sessionId');

  final state = await voice.update(
    sessionId: sessionId,
    state: VoiceSessionState.active,
  );
  print('voice session state: ${state.name}');

  final closed = await voice.close(sessionId);
  print('voice session closed: $closed');
}
