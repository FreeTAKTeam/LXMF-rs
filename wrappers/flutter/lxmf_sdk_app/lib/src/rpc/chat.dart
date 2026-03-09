import 'dart:async';

import '../models.dart';
import 'binding.dart';

enum ChatDirection { inbound, outbound }

class ChatMessage {
  const ChatMessage({
    required this.id,
    required this.peer,
    required this.content,
    required this.timestampMs,
    required this.direction,
    this.receiptStatus,
    this.title,
    this.raw = const {},
  });

  final String id;
  final String peer;
  final String content;
  final int timestampMs;
  final ChatDirection direction;
  final String? receiptStatus;
  final String? title;
  final Map<String, Object?> raw;

  bool get isInbound => direction == ChatDirection.inbound;
}

class ConversationSnapshot {
  const ConversationSnapshot({
    required this.selfAddress,
    required this.peerAddress,
    required this.messages,
  });

  final String selfAddress;
  final String peerAddress;
  final List<ChatMessage> messages;
}

class ConversationUpdate {
  const ConversationUpdate({
    required this.snapshot,
    this.appendedMessage,
  });

  final ConversationSnapshot snapshot;
  final ChatMessage? appendedMessage;
}

class RpcConversationClient {
  RpcConversationClient(this._binding);

  final RpcBinding _binding;
  final Map<String, List<ChatMessage>> _messageCache = <String, List<ChatMessage>>{};

  Future<String> selfAddress() => _binding.deliveryDestinationHash();

  Future<ConversationSnapshot> loadConversation(String peerAddress) async {
    final self = await selfAddress();
    final history = await _binding.messageHistory();
    final messages = history
        .map(ChatMessageMapper.fromMessageRecord)
        .where((message) => _matchesConversation(message, self, peerAddress))
        .toList()
      ..sort((left, right) => left.timestampMs.compareTo(right.timestampMs));
    _messageCache[peerAddress] = List<ChatMessage>.from(messages);
    return ConversationSnapshot(
      selfAddress: self,
      peerAddress: peerAddress,
      messages: List<ChatMessage>.unmodifiable(messages),
    );
  }

  Future<SendReceipt> sendText(
    String peerAddress,
    String content, {
    String? correlationId,
    String? idempotencyKey,
  }) async {
    final self = await selfAddress();
    return _binding.send(
      SendRequest(
        source: self,
        destination: peerAddress,
        payload: content,
        correlationId: correlationId,
        idempotencyKey: idempotencyKey,
      ),
    );
  }

  Stream<ConversationUpdate> watchConversation(String peerAddress) {
    return Stream<ConversationUpdate>.multi((controller) {
      StreamSubscription<AppEvent>? eventsSubscription;
      var cancelled = false;

      Future<void>(() async {
        try {
          final initial = await loadConversation(peerAddress);
          if (cancelled) {
            return;
          }
          controller.add(ConversationUpdate(snapshot: initial));

          eventsSubscription = _binding.subscribeEvents().listen(
            (event) {
              final message = ChatMessageMapper.fromEvent(event);
              if (message == null) {
                return;
              }
              if (!_matchesConversation(message, initial.selfAddress, peerAddress)) {
                return;
              }
              final existing =
                  _messageCache.putIfAbsent(peerAddress, () => <ChatMessage>[]);
              final index = existing.indexWhere((entry) => entry.id == message.id);
              if (index >= 0) {
                existing[index] = message;
              } else {
                existing.add(message);
                existing.sort(
                  (left, right) => left.timestampMs.compareTo(right.timestampMs),
                );
              }
              controller.add(
                ConversationUpdate(
                  snapshot: ConversationSnapshot(
                    selfAddress: initial.selfAddress,
                    peerAddress: peerAddress,
                    messages: List<ChatMessage>.unmodifiable(existing),
                  ),
                  appendedMessage: index >= 0 ? null : message,
                ),
              );
            },
            onError: controller.addError,
          );
        } catch (error, stackTrace) {
          if (!cancelled) {
            controller.addError(error, stackTrace);
            await controller.close();
          }
        }
      });

      controller.onCancel = () async {
        cancelled = true;
        await eventsSubscription?.cancel();
      };
    });
  }

  static bool _matchesConversation(
    ChatMessage message,
    String selfAddress,
    String peerAddress,
  ) {
    final peer = peerAddress.toLowerCase();
    final self = selfAddress.toLowerCase();
    final messagePeer = message.peer.toLowerCase();
    if (messagePeer != peer) {
      return false;
    }
    return self.isNotEmpty;
  }
}

class ChatMessageMapper {
  static ChatMessage fromMessageRecord(MessageRecord record) {
    final directionRaw = record.direction ?? 'out';
    final direction =
        directionRaw == 'in' ? ChatDirection.inbound : ChatDirection.outbound;
    final peer = direction == ChatDirection.inbound
        ? (record.source ?? '')
        : (record.destination ?? '');
    return ChatMessage(
      id: record.id,
      peer: peer,
      content: record.content ?? '',
      timestampMs: record.timestampMs ?? DateTime.now().millisecondsSinceEpoch,
      direction: direction,
      receiptStatus: record.receiptStatus,
      title: record.title,
      raw: record.raw,
    );
  }

  static ChatMessage fromRecord(Map<String, Object?> record) {
    return fromMessageRecord(
      MessageRecord(
        id: (record['id'] ?? '').toString(),
        source: record['source']?.toString(),
        destination: record['destination']?.toString(),
        title: record['title']?.toString(),
        content: record['content']?.toString(),
        timestampMs: _timestampMs(record['timestamp']),
        direction: record['direction']?.toString(),
        fields: record['fields'] is Map
            ? (record['fields'] as Map)
                .map((key, value) => MapEntry(key.toString(), value))
            : const <String, Object?>{},
        receiptStatus: record['receipt_status']?.toString(),
        raw: record,
      ),
    );
  }

  static ChatMessage? fromEvent(AppEvent event) {
    if (event.rawEventType != 'inbound' && event.rawEventType != 'outbound') {
      return null;
    }
    if (event.details is! Map<String, Object?>) {
      return null;
    }
    final payload = event.details! as Map<String, Object?>;
    final messageValue = payload['message'];
    if (messageValue is! Map) {
      return null;
    }
    final message = messageValue.map(
      (key, value) => MapEntry(key.toString(), value),
    );
    return fromRecord(message);
  }

  static int _timestampMs(Object? raw) {
    if (raw is int) {
      return raw < 1000000000000 ? raw * 1000 : raw;
    }
    if (raw is num) {
      final value = raw.toInt();
      return value < 1000000000000 ? value * 1000 : value;
    }
    return DateTime.now().millisecondsSinceEpoch;
  }
}
