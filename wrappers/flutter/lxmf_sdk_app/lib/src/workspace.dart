import 'client.dart';
import 'models.dart';
import 'operations.dart';
import 'rpc/binding.dart';
import 'rpc/chat.dart';

class WorkspaceClient {
  WorkspaceClient(this.app);

  factory WorkspaceClient.fromBinding(AppBinding binding) {
    return WorkspaceClient(AppClient(binding));
  }

  factory WorkspaceClient.rpc(RpcConnectionOptions options) {
    return WorkspaceClient.fromBinding(RpcBinding(options));
  }

  final AppClient app;

  late final OperationClient operations = OperationClient(app);
  late final DiscoveryClient discovery = DiscoveryClient(operations);
  late final CustomCommandClient commands = CustomCommandClient(operations);
  late final ConversationClient conversations = ConversationClient(app);
  late final VoiceSessionClient voice = VoiceSessionClient(operations);
  late final TopicClient topics = TopicClient(operations);
  late final TelemetryClient telemetry = TelemetryClient(operations);
  late final MarkerClient markers = MarkerClient(operations);
  late final AttachmentClient attachments = AttachmentClient(operations);

  Future<Handle> start(Config config) => app.start(config);

  Future<void> stop() => app.stop();

  Future<RuntimeStatus> status() => app.status();

  Future<SendReceipt> send(SendRequest request) => app.send(request);

  Future<SendReport> sendWithProfileDefaults(SendRequest request) {
    return app.sendWithProfileDefaults(request);
  }

  Future<SendReport> sendWithOptions(
    SendRequest request,
    DeliveryOptions options,
  ) {
    return app.sendWithOptions(request, options);
  }

  Stream<AppEvent> subscribeEvents() => app.subscribeEvents();
}
