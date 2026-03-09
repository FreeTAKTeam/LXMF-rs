import 'dart:async';
import 'dart:io';

import 'package:lxmf_sdk_app/lxmf_sdk_app.dart';

Future<void> main(List<String> args) async {
  final endpoint = args.isNotEmpty
      ? Uri.parse(args.first)
      : Uri.parse('http://127.0.0.1:4543/rpc');
  final peer = args.length > 1 ? args[1] : 'ffffffffffffffffffffffffffffffff';
  final client = RpcBinding(
    RpcConnectionOptions(
      endpoint: endpoint,
      pollIdleDelay: const Duration(milliseconds: 100),
    ),
  );
  final app = AppClient(client);
  final chat = RpcConversationClient(client);

  final handle = await app.start(
    const Config(
      profile: Profile.desktopDefault,
      requestedCapabilities: <String>[
        'sdk.capability.identity_multi',
        'sdk.capability.contact_management',
      ],
    ),
  );
  stdout.writeln('started runtime ${handle.runtimeId}');

  final self = await chat.selfAddress();
  stdout.writeln('self address: $self');
  stdout.writeln('peer address: $peer');

  final identities = await client.identityList();
  stdout.writeln('local identities: ${identities.length}');

  final contacts = await client.contactList(limit: 10);
  stdout.writeln('known contacts: ${contacts.contacts.length}');

  final history = await client.messageHistory();
  stdout.writeln('message history entries: ${history.length}');

  final conversation = await chat.loadConversation(peer);
  stdout.writeln('conversation messages: ${conversation.messages.length}');

  final subscription = chat.watchConversation(peer).listen((update) {
    final message = update.appendedMessage;
    if (message != null) {
      stdout.writeln(
        'event ${message.direction.name} ${message.peer}: ${message.content}',
      );
    }
  });

  final receipt = await chat.sendText(peer, 'hello-from-flutter-rpc-chat-smoke');
  stdout.writeln('queued message ${receipt.messageId}');

  final status = await client.deliveryStatus(receipt.messageId);
  stdout.writeln('initial receipt status: ${status?.receiptStatus ?? 'none'}');

  final deliveryWatch = client.watchMessageStatus(receipt.messageId).listen((next) {
    stdout.writeln('receipt ${next.messageId}: ${next.receiptStatus ?? 'unknown'}');
  });

  await Future<void>.delayed(const Duration(seconds: 2));
  await deliveryWatch.cancel();
  await subscription.cancel();
  await app.stop();
  stdout.writeln('requested graceful shutdown');
}
