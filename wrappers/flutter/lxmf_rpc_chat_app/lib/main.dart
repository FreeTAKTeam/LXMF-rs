import 'dart:async';

import 'package:flutter/material.dart';
import 'package:lxmf_sdk_app/lxmf_sdk_app.dart';

void main() {
  runApp(const SignalDeskApp());
}

class SignalDeskApp extends StatelessWidget {
  const SignalDeskApp({super.key});

  @override
  Widget build(BuildContext context) {
    const parchment = Color(0xFFF2E9D8);
    const ink = Color(0xFF171717);
    const copper = Color(0xFFB86B2F);
    const moss = Color(0xFF3D5641);

    final base = ThemeData(
      colorScheme: ColorScheme.fromSeed(
        seedColor: copper,
        brightness: Brightness.light,
        surface: parchment,
      ),
      useMaterial3: true,
    );

    return MaterialApp(
      title: 'LXMF Signal Desk',
      debugShowCheckedModeBanner: false,
      theme: base.copyWith(
        scaffoldBackgroundColor: parchment,
        textTheme: base.textTheme.copyWith(
          displayLarge: const TextStyle(
            fontFamily: 'Baskerville',
            fontSize: 54,
            fontWeight: FontWeight.w700,
            color: ink,
            height: 0.95,
          ),
          displayMedium: const TextStyle(
            fontFamily: 'Baskerville',
            fontSize: 34,
            fontWeight: FontWeight.w700,
            color: ink,
          ),
          headlineSmall: const TextStyle(
            fontFamily: 'Baskerville',
            fontSize: 24,
            fontWeight: FontWeight.w700,
            color: ink,
          ),
          bodyLarge: TextStyle(
            fontFamily: 'Menlo',
            fontSize: 14,
            height: 1.45,
            color: ink,
          ),
          bodyMedium: TextStyle(
            fontFamily: 'Menlo',
            fontSize: 12,
            height: 1.4,
            color: ink.withValues(alpha: 0.8),
          ),
        ),
        cardTheme: CardThemeData(
          color: Colors.white.withValues(alpha: 0.72),
          elevation: 0,
          shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.circular(28),
            side: BorderSide(color: ink.withValues(alpha: 0.08)),
          ),
        ),
      ),
      home: const SignalDeskScreen(
        endpointSeed: 'http://127.0.0.1:4543/rpc',
        peerSeed: '0123456789abcdef0123456789abcdef',
        accent: copper,
        ink: ink,
        moss: moss,
      ),
    );
  }
}

class SignalDeskScreen extends StatefulWidget {
  const SignalDeskScreen({
    super.key,
    required this.endpointSeed,
    required this.peerSeed,
    required this.accent,
    required this.ink,
    required this.moss,
  });

  final String endpointSeed;
  final String peerSeed;
  final Color accent;
  final Color ink;
  final Color moss;

  @override
  State<SignalDeskScreen> createState() => _SignalDeskScreenState();
}

class _SignalDeskScreenState extends State<SignalDeskScreen> {
  late final TextEditingController _endpointController;
  late final TextEditingController _peerController;
  late final TextEditingController _messageController;

  AppClient? _app;
  RpcConversationClient? _chat;
  StreamSubscription<ConversationUpdate>? _conversationSubscription;

  bool _busy = false;
  String _statusLine = 'Idle. Start reticulumd separately, then connect.';
  String? _runtimeId;
  String? _selfAddress;
  String? _lastReceiptStatus;
  int _contactCount = 0;
  int _historyCount = 0;
  List<ChatMessage> _messages = const <ChatMessage>[];

  @override
  void initState() {
    super.initState();
    _endpointController = TextEditingController(text: widget.endpointSeed);
    _peerController = TextEditingController(text: widget.peerSeed);
    _messageController = TextEditingController(
      text: 'hello-from-lxmf-signal-desk',
    );
  }

  @override
  void dispose() {
    _endpointController.dispose();
    _peerController.dispose();
    _messageController.dispose();
    unawaited(_conversationSubscription?.cancel());
    unawaited(_disconnectInternal());
    super.dispose();
  }

  Future<void> _connect() async {
    await _runGuarded(() async {
      await _disconnectInternal();
      final endpoint = Uri.parse(_endpointController.text.trim());
      await _connectToEndpoint(endpoint);
    });
  }

  Future<void> _refresh() async {
    await _runGuarded(() async {
      if (_app == null || _chat == null) {
        throw const AppError(
          code: ErrorCode.runtimeNotStarted,
          category: ErrorCategory.runtime,
          message: 'connect before refreshing the conversation',
        );
      }
      final contacts = await _app!.contactList(limit: 10);
      final history = await _app!.messageHistory();
      final snapshot = await _chat!.loadConversation(_peerController.text.trim());
      _contactCount = contacts.contacts.length;
      _historyCount = history.length;
      _messages = snapshot.messages;
      _statusLine = 'Conversation refreshed from daemon state.';
      if (mounted) {
        setState(() {});
      }
    });
  }

  Future<void> _send() async {
    await _runGuarded(() async {
      if (_app == null || _chat == null) {
        throw const AppError(
          code: ErrorCode.runtimeNotStarted,
          category: ErrorCategory.runtime,
          message: 'connect before sending a message',
        );
      }
      final peer = _peerController.text.trim();
      final content = _messageController.text.trim();
      if (peer.isEmpty || content.isEmpty) {
        throw const AppError(
          code: ErrorCode.validationInvalidArgument,
          category: ErrorCategory.validation,
          message: 'peer and message content must not be empty',
          userActionRequired: true,
        );
      }

      final receipt = await _chat!.sendText(peer, content);
      final status = await _app!.deliveryStatus(receipt.messageId);
      _lastReceiptStatus = status?.receiptStatus ?? 'queued';
      _statusLine = 'Queued ${receipt.messageId}.';
      _messageController.clear();
      if (mounted) {
        setState(() {});
      }
    });
  }

  Future<void> _disconnect() async {
    await _runGuarded(() async {
      await _disconnectInternal();
      _statusLine = 'Disconnected.';
      if (mounted) {
        setState(() {});
      }
    });
  }

  Future<void> _disconnectInternal() async {
    await _conversationSubscription?.cancel();
    _conversationSubscription = null;
    if (_app != null) {
      await _app!.stop();
    }
    _app = null;
    _chat = null;
    _runtimeId = null;
    _selfAddress = null;
    _lastReceiptStatus = null;
    _contactCount = 0;
    _historyCount = 0;
    _messages = const <ChatMessage>[];
  }

  Future<void> _connectToEndpoint(Uri endpoint) async {
    final binding = RpcBinding(
      RpcConnectionOptions(
        endpoint: endpoint,
        pollIdleDelay: const Duration(milliseconds: 150),
      ),
    );
    final app = AppClient(binding);
    final chat = RpcConversationClient(binding);

    final handle = await app.start(
      const Config(
        profile: Profile.desktopDefault,
        requestedCapabilities: <String>[
          'sdk.capability.identity_multi',
          'sdk.capability.contact_management',
        ],
      ),
    );

    final self = await chat.selfAddress();
    final contacts = await app.contactList(limit: 10);
    final history = await app.messageHistory();

    _app = app;
    _chat = chat;
    _runtimeId = handle.runtimeId;
    _selfAddress = self;
    _contactCount = contacts.contacts.length;
    _historyCount = history.length;
    _statusLine = 'Connected. Monitoring ${_peerController.text.trim()}.';

    await _watchConversation();
    if (mounted) {
      setState(() {});
    }
  }

  Future<void> _watchConversation() async {
    await _conversationSubscription?.cancel();
    final chat = _chat;
    if (chat == null) {
      return;
    }

    _conversationSubscription = chat
        .watchConversation(_peerController.text.trim())
        .listen((update) {
      if (!mounted) {
        return;
      }
      setState(() {
        _messages = update.snapshot.messages;
        if (update.appendedMessage != null) {
          _statusLine =
              'Stream updated with ${update.appendedMessage!.direction.name} traffic.';
        }
      });
    }, onError: (Object error) {
      if (!mounted) {
        return;
      }
      setState(() {
        _statusLine = error.toString();
      });
    });
  }

  Future<void> _runGuarded(Future<void> Function() action) async {
    if (_busy) {
      return;
    }
    setState(() {
      _busy = true;
    });
    try {
      await action();
    } catch (error) {
      if (mounted) {
        setState(() {
          _statusLine = error.toString();
        });
      }
    } finally {
      if (mounted) {
        setState(() {
          _busy = false;
        });
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Scaffold(
      body: DecoratedBox(
        decoration: BoxDecoration(
          gradient: LinearGradient(
            begin: Alignment.topLeft,
            end: Alignment.bottomRight,
            colors: <Color>[
              theme.scaffoldBackgroundColor,
              const Color(0xFFE5D7C3),
              const Color(0xFFDCC4A6),
            ],
          ),
        ),
        child: SafeArea(
          child: Padding(
            padding: const EdgeInsets.all(24),
            child: LayoutBuilder(
              builder: (context, constraints) {
                final wide = constraints.maxWidth >= 980;
                final controls = _buildControlDeck(theme);
                final feed = _buildFeed(theme);
                if (wide) {
                  return Row(
                    children: <Widget>[
                      SizedBox(width: 360, child: controls),
                      const SizedBox(width: 24),
                      Expanded(child: feed),
                    ],
                  );
                }
                return Column(
                  children: <Widget>[
                    Expanded(
                      flex: 4,
                      child: controls,
                    ),
                    const SizedBox(height: 20),
                    Expanded(
                      flex: 5,
                      child: feed,
                    ),
                  ],
                );
              },
            ),
          ),
        ),
      ),
    );
  }

  Widget _buildControlDeck(ThemeData theme) {
    return Card(
      clipBehavior: Clip.antiAlias,
      child: SingleChildScrollView(
        padding: const EdgeInsets.all(24),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: <Widget>[
            Text('Signal Desk', style: theme.textTheme.displayLarge),
            const SizedBox(height: 8),
            Text(
              'A field-facing RPC console for the public LXMF app layer.',
              style: theme.textTheme.bodyLarge,
            ),
            const SizedBox(height: 24),
            _LabeledField(
              label: 'RPC endpoint',
              controller: _endpointController,
            ),
            const SizedBox(height: 16),
            _LabeledField(
              label: 'Peer destination',
              controller: _peerController,
            ),
            const SizedBox(height: 16),
            _LabeledField(
              label: 'Outgoing line',
              controller: _messageController,
              maxLines: 3,
            ),
            const SizedBox(height: 18),
            Wrap(
              spacing: 10,
              runSpacing: 10,
              children: <Widget>[
                _ActionButton(
                  label: _runtimeId == null ? 'Connect' : 'Reconnect',
                  onPressed: _connect,
                  busy: _busy,
                  background: widget.ink,
                  foreground: Colors.white,
                ),
                _ActionButton(
                  label: 'Refresh',
                  onPressed: _refresh,
                  busy: _busy,
                  background: Colors.white,
                  foreground: widget.ink,
                ),
                _ActionButton(
                  label: 'Send',
                  onPressed: _send,
                  busy: _busy,
                  background: widget.accent,
                  foreground: Colors.white,
                ),
                _ActionButton(
                  label: 'Disconnect',
                  onPressed: _disconnect,
                  busy: _busy,
                  background: widget.moss,
                  foreground: Colors.white,
                ),
              ],
            ),
            const SizedBox(height: 24),
            Wrap(
              spacing: 8,
              runSpacing: 8,
              children: <Widget>[
                _StatChip(
                  label: 'runtime',
                  value: _runtimeId == null ? 'offline' : 'online',
                  tone: widget.ink,
                ),
                _StatChip(
                  label: 'contacts',
                  value: '$_contactCount',
                  tone: widget.moss,
                ),
                _StatChip(
                  label: 'history',
                  value: '$_historyCount',
                  tone: widget.accent,
                ),
                if (_lastReceiptStatus != null)
                  _StatChip(
                    label: 'receipt',
                    value: _lastReceiptStatus!,
                    tone: widget.ink,
                  ),
              ],
            ),
            const SizedBox(height: 24),
            Text(
              'Station',
              style: theme.textTheme.headlineSmall,
            ),
            const SizedBox(height: 8),
            Text(
              _selfAddress ?? 'Not connected yet',
              style: theme.textTheme.bodyLarge,
            ),
            const SizedBox(height: 18),
            Text(
              'Status',
              style: theme.textTheme.headlineSmall,
            ),
            const SizedBox(height: 8),
            Container(
              width: double.infinity,
              decoration: BoxDecoration(
                color: widget.ink.withValues(alpha: 0.05),
                borderRadius: BorderRadius.circular(18),
              ),
              padding: const EdgeInsets.all(14),
              child: Text(_statusLine, style: theme.textTheme.bodyLarge),
            ),
          ],
        ),
      ),
    );
  }

  Widget _buildFeed(ThemeData theme) {
    return Card(
      color: widget.ink,
      child: Padding(
        padding: const EdgeInsets.all(24),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: <Widget>[
            Row(
              children: <Widget>[
                Expanded(
                  child: Text(
                    'Live Feed',
                    style: theme.textTheme.displayMedium?.copyWith(
                      color: Colors.white,
                    ),
                  ),
                ),
                Text(
                  _peerController.text.trim(),
                  style: theme.textTheme.bodyLarge?.copyWith(
                    color: Colors.white.withValues(alpha: 0.8),
                  ),
                ),
              ],
            ),
            const SizedBox(height: 12),
            Text(
              'Messages arrive here as the daemon-backed conversation stream advances.',
              style: theme.textTheme.bodyMedium?.copyWith(
                color: Colors.white.withValues(alpha: 0.72),
              ),
            ),
            const SizedBox(height: 24),
            Expanded(
              child: _messages.isEmpty
                  ? Center(
                      child: Text(
                        'No traffic yet. Connect, pick a peer, and send the first line.',
                        textAlign: TextAlign.center,
                        style: theme.textTheme.headlineSmall?.copyWith(
                          color: Colors.white.withValues(alpha: 0.82),
                        ),
                      ),
                    )
                  : ListView.separated(
                      itemCount: _messages.length,
                      separatorBuilder: (_, _) => const SizedBox(height: 12),
                      itemBuilder: (context, index) {
                        final message = _messages[index];
                        final outbound = message.direction == ChatDirection.outbound;
                        final tone = outbound ? widget.accent : widget.moss;
                        return Align(
                          alignment: outbound
                              ? Alignment.centerRight
                              : Alignment.centerLeft,
                          child: ConstrainedBox(
                            constraints: const BoxConstraints(maxWidth: 520),
                            child: DecoratedBox(
                              decoration: BoxDecoration(
                                color: tone.withValues(alpha: 0.94),
                                borderRadius: BorderRadius.circular(24),
                              ),
                              child: Padding(
                                padding: const EdgeInsets.all(16),
                                child: Column(
                                  crossAxisAlignment: CrossAxisAlignment.start,
                                  children: <Widget>[
                                    Text(
                                      outbound ? 'Outbound' : 'Inbound',
                                      style: theme.textTheme.bodyMedium?.copyWith(
                                        color: Colors.white.withValues(alpha: 0.78),
                                      ),
                                    ),
                                    const SizedBox(height: 6),
                                    Text(
                                      message.content,
                                      style: theme.textTheme.bodyLarge?.copyWith(
                                        color: Colors.white,
                                      ),
                                    ),
                                    const SizedBox(height: 10),
                                    Text(
                                      message.receiptStatus ?? 'no receipt yet',
                                      style: theme.textTheme.bodyMedium?.copyWith(
                                        color: Colors.white.withValues(alpha: 0.72),
                                      ),
                                    ),
                                  ],
                                ),
                              ),
                            ),
                          ),
                        );
                      },
                    ),
            ),
          ],
        ),
      ),
    );
  }
}

class _LabeledField extends StatelessWidget {
  const _LabeledField({
    required this.label,
    required this.controller,
    this.maxLines = 1,
  });

  final String label;
  final TextEditingController controller;
  final int maxLines;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: <Widget>[
        Text(label, style: theme.textTheme.bodyMedium),
        const SizedBox(height: 8),
        TextField(
          controller: controller,
          maxLines: maxLines,
          style: theme.textTheme.bodyLarge,
          decoration: InputDecoration(
            filled: true,
            fillColor: Colors.white.withValues(alpha: 0.75),
            border: OutlineInputBorder(
              borderRadius: BorderRadius.circular(18),
              borderSide: BorderSide.none,
            ),
            contentPadding: const EdgeInsets.symmetric(
              horizontal: 16,
              vertical: 14,
            ),
          ),
        ),
      ],
    );
  }
}

class _ActionButton extends StatelessWidget {
  const _ActionButton({
    required this.label,
    required this.onPressed,
    required this.busy,
    required this.background,
    required this.foreground,
  });

  final String label;
  final Future<void> Function() onPressed;
  final bool busy;
  final Color background;
  final Color foreground;

  @override
  Widget build(BuildContext context) {
    return FilledButton(
      onPressed: busy ? null : () => unawaited(onPressed()),
      style: FilledButton.styleFrom(
        backgroundColor: background,
        foregroundColor: foreground,
        padding: const EdgeInsets.symmetric(horizontal: 18, vertical: 16),
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(18),
        ),
      ),
      child: Text(label),
    );
  }
}

class _StatChip extends StatelessWidget {
  const _StatChip({
    required this.label,
    required this.value,
    required this.tone,
  });

  final String label;
  final String value;
  final Color tone;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 10),
      decoration: BoxDecoration(
        color: tone.withValues(alpha: 0.10),
        borderRadius: BorderRadius.circular(999),
        border: Border.all(color: tone.withValues(alpha: 0.16)),
      ),
      child: RichText(
        text: TextSpan(
          style: theme.textTheme.bodyMedium,
          children: <InlineSpan>[
            TextSpan(text: '$label ', style: TextStyle(color: tone)),
            TextSpan(
              text: value,
              style: TextStyle(
                color: tone,
                fontWeight: FontWeight.w700,
              ),
            ),
          ],
        ),
      ),
    );
  }
}
