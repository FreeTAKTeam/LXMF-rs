import 'dart:ffi';

import '../client.dart';
import '../models.dart';

/// Placeholder native bridge for the first Flutter wrapper slice.
///
/// The next implementation step should translate the stable `rns-embedded-ffi`
/// v1 node-centric API into the `sdk-app` Dart model exposed by this package.
class EmbeddedNodeBridge implements AppBinding {
  EmbeddedNodeBridge(this.library);

  final DynamicLibrary library;

  @override
  Future<Handle> start(Config config) {
    throw UnimplementedError(
      'Native Flutter bridge is not implemented yet. '
      'Map rns_embedded_v1_node_start() into the sdk-app Handle model here.',
    );
  }

  @override
  Future<void> stop() {
    throw UnimplementedError(
      'Native Flutter bridge is not implemented yet. '
      'Map rns_embedded_v1_node_stop() here.',
    );
  }

  @override
  Future<RuntimeStatus> status() {
    throw UnimplementedError(
      'Native Flutter bridge is not implemented yet. '
      'Map rns_embedded_v1_node_get_status() here.',
    );
  }

  @override
  Future<SendReceipt> send(SendRequest request) {
    throw UnimplementedError(
      'Native Flutter bridge is not implemented yet. '
      'Map rns_embedded_v1_node_send() here.',
    );
  }

  @override
  Future<SendReport> sendWithProfileDefaults(SendRequest request) {
    throw UnimplementedError(
      'Native Flutter bridge is not implemented yet. '
      'Use Dart-side delivery policy or native app-surface helpers here.',
    );
  }

  @override
  Future<SendReport> sendWithOptions(
    SendRequest request,
    DeliveryOptions options,
  ) {
    throw UnimplementedError(
      'Native Flutter bridge is not implemented yet. '
      'Use Dart-side delivery policy or native app-surface helpers here.',
    );
  }

  @override
  Stream<AppEvent> subscribeEvents() {
    throw UnimplementedError(
      'Native Flutter bridge is not implemented yet. '
      'Map rns_embedded_v1_node_subscribe_events()/subscription_next() here.',
    );
  }
}
