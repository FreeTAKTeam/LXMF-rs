import 'models.dart';

abstract interface class AppBinding {
  Future<Handle> start(Config config);

  Future<void> stop();

  Future<RuntimeStatus> status();

  Future<SendReceipt> send(SendRequest request);

  Future<SendReport> sendWithProfileDefaults(SendRequest request);

  Future<SendReport> sendWithOptions(
    SendRequest request,
    DeliveryOptions options,
  );

  Stream<AppEvent> subscribeEvents();
}

class AppClient {
  AppClient(this._binding);

  final AppBinding _binding;

  Future<Handle> start(Config config) => _binding.start(config);

  Future<void> stop() => _binding.stop();

  Future<RuntimeStatus> status() => _binding.status();

  Future<SendReceipt> send(SendRequest request) => _binding.send(request);

  Future<SendReport> sendWithProfileDefaults(SendRequest request) {
    return _binding.sendWithProfileDefaults(request);
  }

  Future<SendReport> sendWithOptions(
    SendRequest request,
    DeliveryOptions options,
  ) {
    return _binding.sendWithOptions(request, options);
  }

  Stream<AppEvent> subscribeEvents() => _binding.subscribeEvents();
}
